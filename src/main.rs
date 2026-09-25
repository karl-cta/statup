//! Statup server entry point and maintenance commands.

use std::net::SocketAddr;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use anyhow::Context;
use axum::Router;
use tokio::task::{AbortHandle, JoinError};

use statup::config::{Config, init_logging};
use statup::db::{self, DbPool};
use statup::middleware::rate_limit::RateLimit;
use statup::routes::create_router;
use statup::services::{
    AuthService, DashboardLayoutService, LoginRateLimiter, SettingsService,
    spawn_maintenance_schedule,
};
use statup::session;
use statup::state::AppState;

/// How long open connections get to finish once a shutdown is asked for.
const DRAIN_TIMEOUT: Duration = Duration::from_secs(10);

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if !args.is_empty() {
        std::process::exit(statup::cli::run(&args).await);
    }

    let config = Config::from_env().context("invalid configuration")?;
    init_logging(config.log_level);
    statup::init_asset_versions();
    tracing::info!("Statup starting on {}", config.bind_addr());
    warn_on_plain_proxy(&config);

    let pool = open_database(&config).await?;
    let state = build_state(&config, pool.clone()).await?;
    let session_store = session::create_session_store(&pool)
        .await
        .context("cannot create the session store")?;
    let rate_limit = RateLimit::new(config.client_ip_source())?;
    let tasks = spawn_background_tasks(&pool, &rate_limit);

    let sessions = session::session_layer(
        session_store,
        config.session_expiry,
        config.secure_cookies(),
    );
    let app = create_router(state, &rate_limit).layer(sessions);
    let served = serve(app, &config.bind_addr()).await;
    stop(tasks, &pool).await;
    served
}

/// Behind a proxy that terminates TLS, `PUBLIC_URL` is what marks the
/// cookies `Secure` and turns HSTS on.
fn warn_on_plain_proxy(config: &Config) {
    if config.public_url.is_none() {
        tracing::info!("PUBLIC_URL is not set: links in the feed use the host of each request");
    }
    if config.trust_proxy_headers && !config.secure_cookies() {
        tracing::warn!(
            "TRUST_PROXY_HEADERS is on but PUBLIC_URL is not an https address: \
             cookies are sent without the Secure flag and no HSTS header is set"
        );
    }
}

/// Stops the background work, then closes the database.
async fn stop(tasks: Vec<AbortHandle>, pool: &DbPool) {
    for task in tasks {
        task.abort();
    }
    // A request cut by the drain timeout may still hold a connection.
    if tokio::time::timeout(DRAIN_TIMEOUT, pool.close())
        .await
        .is_err()
    {
        tracing::warn!("Database connections still in use, closing anyway");
    }
    tracing::info!("Statup stopped");
}

/// Opens and migrates the database, then creates the first administrator
/// when the environment names one.
async fn open_database(config: &Config) -> anyhow::Result<DbPool> {
    let pool = db::create_pool(&config.database_url, config.db_max_connections)
        .await
        .with_context(|| format!("cannot open the database at {}", config.database_url))?;
    db::run_migrations(&pool)
        .await
        .context("database migrations failed")?;
    if let Err(e) = db::optimize(&pool).await {
        tracing::warn!(error = %e, "Planner statistics were not updated");
    }
    DashboardLayoutService::reconcile(&pool)
        .await
        .context("cannot prepare the dashboard layouts")?;

    AuthService::bootstrap_admin(
        &pool,
        config.admin_email.as_deref(),
        config.admin_password.as_deref(),
    )
    .await
    .context(
        "cannot create the administrator: ADMIN_EMAIL must be an email address, \
         and ADMIN_PASSWORD 12 characters with upper and lower case letters, a digit \
         and a symbol, or at least 20 characters",
    )?;
    tracing::info!("Database ready");
    Ok(pool)
}

/// The state shared by the handlers. The access and the name chosen in the
/// settings win over the environment.
async fn build_state(config: &Config, pool: DbPool) -> anyhow::Result<AppState> {
    let icons_dir = Path::new(&config.upload_dir).join("icons");
    std::fs::create_dir_all(&icons_dir)
        .with_context(|| format!("cannot create {}", icons_dir.display()))?;
    tracing::info!("Upload directory ready: {}", config.upload_dir);

    let public_mode = SettingsService::load(&pool, config.public_mode)
        .await
        .context("cannot read the settings")?;
    tracing::info!(public_mode, "Public mode");
    tracing::info!(time_zone = statup::clock::zone().name(), "Time zone");

    Ok(AppState {
        pool,
        login_limiter: Arc::new(LoginRateLimiter::default()),
        upload_dir: config.upload_dir.clone(),
        public_mode: Arc::new(AtomicBool::new(public_mode)),
        trust_proxy_headers: config.trust_proxy_headers,
        client_ip_source: config.client_ip_source(),
        public_url: config.public_url.clone(),
    })
}

/// Periodic work beside the server, stopped on shutdown.
fn spawn_background_tasks(pool: &DbPool, rate_limit: &RateLimit) -> Vec<AbortHandle> {
    vec![
        session::spawn_cleanup_task(pool.clone()),
        rate_limit.spawn_cleanup_task(),
        spawn_maintenance_schedule(pool.clone()),
    ]
}

/// Serves until a shutdown signal, then gives open connections
/// [`DRAIN_TIMEOUT`] to finish.
async fn serve(app: Router, bind_addr: &str) -> anyhow::Result<()> {
    let listener = tokio::net::TcpListener::bind(bind_addr)
        .await
        .with_context(|| format!("cannot listen on {bind_addr}"))?;
    tracing::info!("Listening on {bind_addr}");

    let (stop, stopped) = tokio::sync::oneshot::channel::<()>();
    let server = axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(async {
        stopped.await.ok();
    });
    let mut server = tokio::spawn(server.into_future());

    tokio::select! {
        result = &mut server => return server_outcome(result),
        () = shutdown_signal() => {}
    }
    stop.send(()).ok();
    if let Ok(result) = tokio::time::timeout(DRAIN_TIMEOUT, &mut server).await {
        return server_outcome(result);
    }
    tracing::warn!(
        "Connections still open after {} s, closing them",
        DRAIN_TIMEOUT.as_secs()
    );
    server.abort();
    Ok(())
}

fn server_outcome(result: Result<std::io::Result<()>, JoinError>) -> anyhow::Result<()> {
    result
        .context("the server task failed")?
        .context("the server stopped on an error")
}

/// Wait for SIGINT (Ctrl+C) or SIGTERM, then return. A signal that cannot
/// be listened for is logged and never fires.
async fn shutdown_signal() {
    let ctrl_c = async {
        if let Err(e) = tokio::signal::ctrl_c().await {
            tracing::error!(error = %e, "Cannot listen for Ctrl+C");
            std::future::pending::<()>().await;
        }
    };

    #[cfg(unix)]
    let terminate = async {
        use tokio::signal::unix::{SignalKind, signal};
        match signal(SignalKind::terminate()) {
            Ok(mut sigterm) => {
                sigterm.recv().await;
            }
            Err(e) => {
                tracing::error!(error = %e, "Cannot listen for SIGTERM");
                std::future::pending::<()>().await;
            }
        }
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        () = ctrl_c => tracing::info!("Received SIGINT, shutting down"),
        () = terminate => tracing::info!("Received SIGTERM, shutting down"),
    }
}
