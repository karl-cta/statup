//! `statup service`: Statup run by the Windows service manager.

use std::ffi::OsString;
use std::sync::Arc;
use std::time::Duration;

use ::windows_service::service::{
    ServiceControl, ServiceControlAccept, ServiceExitCode, ServiceState, ServiceStatus, ServiceType,
};
use ::windows_service::service_control_handler::{
    self, ServiceControlHandlerResult, ServiceStatusHandle,
};
use ::windows_service::{define_windows_service, service_dispatcher};
use anyhow::Context;
use tokio::sync::Notify;

use statup::config::{DEFAULT_DATABASE_URL, load_dotenv};
use statup::log_file;

/// Windows ignores the name for a service alone in its process: one
/// registered under another name runs the same.
const SERVICE_NAME: &str = "Statup";

/// How long Windows waits for the stop before it ends the process: open
/// connections, then the database, get [`crate::DRAIN_TIMEOUT`] each.
const STOP_WAIT: Duration = Duration::from_secs(2 * crate::DRAIN_TIMEOUT.as_secs() + 5);

define_windows_service!(ffi_service_main, service_main);

/// Hands the process to the service manager until the service stops.
pub fn run() -> anyhow::Result<()> {
    service_dispatcher::start(SERVICE_NAME, ffi_service_main)
        .context("statup service is started by the Windows service manager, not by hand")
}

fn service_main(_arguments: Vec<OsString>) {
    let stop = Arc::new(Notify::new());
    let asked = Arc::clone(&stop);
    let on_control = move |control| match control {
        ServiceControl::Stop | ServiceControl::Shutdown => {
            asked.notify_one();
            ServiceControlHandlerResult::NoError
        }
        ServiceControl::Interrogate => ServiceControlHandlerResult::NoError,
        _ => ServiceControlHandlerResult::NotImplemented,
    };
    // Without a handle there is no way to answer Windows, which gives up
    // on the start by itself.
    let Ok(status) = service_control_handler::register(SERVICE_NAME, on_control) else {
        return;
    };
    let _ = report(status, ServiceState::Running, ServiceExitCode::NO_ERROR);
    // An error code is logged by Windows, and lets its recovery actions
    // restart the service.
    let code = match serve(status, stop) {
        Ok(()) => ServiceExitCode::NO_ERROR,
        Err(_) => ServiceExitCode::ServiceSpecific(1),
    };
    let _ = report(status, ServiceState::Stopped, code);
}

/// Runs the server from the folder of `statup.exe`, where `static` is,
/// until Windows asks it to stop. A failure is written to the log file.
fn serve(status: ServiceStatusHandle, stop: Arc<Notify>) -> anyhow::Result<()> {
    let exe = std::env::current_exe().context("cannot find statup.exe")?;
    if let Some(folder) = exe.parent() {
        std::env::set_current_dir(folder)
            .with_context(|| format!("cannot work from {}", folder.display()))?;
    }
    load_dotenv()?;
    let database_url =
        std::env::var("DATABASE_URL").unwrap_or_else(|_| DEFAULT_DATABASE_URL.to_string());
    let log_path = log_file::path_for(&database_url);
    let file = log_file::open(&log_path)
        .with_context(|| format!("cannot open the log file {}", log_path.display()))?;

    let stopping = async move {
        stop.notified().await;
        let _ = report(status, ServiceState::StopPending, ServiceExitCode::NO_ERROR);
    };
    let runtime = tokio::runtime::Runtime::new().context("cannot start the async runtime")?;
    let served = runtime.block_on(crate::run_server(Some(file), stopping));
    if let Err(e) = &served {
        if tracing::dispatcher::has_been_set() {
            tracing::error!("{e:#}");
        } else {
            let _ = log_file::write_error(&log_path, &format!("{e:#}"));
        }
    }
    served
}

fn report(
    status: ServiceStatusHandle,
    state: ServiceState,
    exit_code: ServiceExitCode,
) -> ::windows_service::Result<()> {
    let controls_accepted = match state {
        ServiceState::Running => ServiceControlAccept::STOP | ServiceControlAccept::SHUTDOWN,
        _ => ServiceControlAccept::empty(),
    };
    let wait_hint = match state {
        ServiceState::StopPending => STOP_WAIT,
        _ => Duration::ZERO,
    };
    status.set_service_status(ServiceStatus {
        service_type: ServiceType::OWN_PROCESS,
        current_state: state,
        controls_accepted,
        exit_code,
        checkpoint: 0,
        wait_hint,
        process_id: None,
    })
}
