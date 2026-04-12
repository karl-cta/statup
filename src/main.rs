//! Statup - Internal IT/Ops status page.

use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use statup::config::{Config, init_logging};
use statup::db;
use statup::repositories::SettingsRepository;
use statup::routes::create_router;
use statup::services::{AuthService, LoginRateLimiter};
use statup::session;
use statup::state::AppState;

#[tokio::main]
async fn main() {
    let config = Config::from_env().expect("Failed to load configuration");

    init_logging(config.log_level);

    statup::init_css_version();
    tracing::info!("Statup starting on {}", config.bind_addr());

    let pool = db::create_pool(&config.database_url, config.db_max_connections)
        .await
        .expect("Failed to create database pool");

    db::run_migrations(&pool)
        .await
        .expect("Failed to run database migrations");

    tracing::info!("Database ready");

    AuthService::bootstrap_admin(
        &pool,
        config.admin_email.as_deref(),
        config.admin_password.as_deref(),
    )
    .await
    .expect("Failed to bootstrap admin user");

    let session_store = session::create_session_store(&pool)
        .await
        .expect("Failed to create session store");

    let cleanup_handle = session::spawn_cleanup_task(session_store.clone());
    let session_layer = session::session_layer(session_store, &config);

    // Ensure upload directories exist
    let icons_dir = format!("{}/icons", config.upload_dir);
    std::fs::create_dir_all(&icons_dir).expect("Failed to create upload directory");
    tracing::info!("Upload directory ready: {}", config.upload_dir);

    let bind_addr = config.bind_addr();

    // Load public_mode from DB (persisted toggle), fallback to env var
    let public_mode = match SettingsRepository::get(&pool, "public_mode").await {
        Ok(Some(v)) => v == "true",
        _ => config.public_mode,
    };
    tracing::info!(public_mode, "Public mode");

    let state = AppState {
        pool,
        login_limiter: Arc::new(LoginRateLimiter::default()),
        upload_dir: config.upload_dir,
        public_mode: Arc::new(AtomicBool::new(public_mode)),
    };

    let app = create_router(state).layer(session_layer);
    let listener = tokio::net::TcpListener::bind(&bind_addr)
        .await
        .expect("Failed to bind to address");

    tracing::info!("Listening on {bind_addr}");

    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown_signal())
    .await
    .expect("Server error");

    cleanup_handle.abort();
    tracing::info!("Statup stopped");
}

/// Wait for SIGINT (Ctrl+C) or SIGTERM, then return.
async fn shutdown_signal() {
    use tokio::signal;

    let ctrl_c = async {
        signal::ctrl_c()
            .await
            .expect("Failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("Failed to install SIGTERM handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        () = ctrl_c => tracing::info!("Received SIGINT, shutting down"),
        () = terminate => tracing::info!("Received SIGTERM, shutting down"),
    }
}
