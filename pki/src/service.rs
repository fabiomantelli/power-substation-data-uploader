/// Windows Service integration for osc-pki-server

#[cfg(windows)]
pub mod windows_service {
    use crate::config::PkiConfig;
    use crate::run_server;
    use std::ffi::OsString;
    use std::sync::{Arc, Mutex};
    use windows_service::{
        define_windows_service,
        service::{
            ServiceControl, ServiceControlAccept, ServiceExitCode, ServiceState, ServiceStatus,
            ServiceType,
        },
        service_control_handler::{self, ServiceControlHandlerResult},
        service_dispatcher,
    };

    const SERVICE_NAME: &str = "OscPkiServer";

    static GLOBAL_CONFIG: Mutex<Option<PkiConfig>> = Mutex::new(None);

    define_windows_service!(ffi_service_main, service_main);

    pub fn run_as_service(cfg: PkiConfig) -> anyhow::Result<()> {
        {
            let mut guard = GLOBAL_CONFIG.lock().unwrap();
            *guard = Some(cfg);
        }
        service_dispatcher::start(SERVICE_NAME, ffi_service_main)
            .map_err(|e| anyhow::anyhow!("service_dispatcher::start falhou: {}", e))
    }

    fn service_main(_arguments: Vec<OsString>) {
        if let Err(e) = run_service() {
            tracing::error!("Erro no serviço Windows: {:#}", e);
        }
    }

    fn run_service() -> anyhow::Result<()> {
        let cfg = {
            let mut guard = GLOBAL_CONFIG.lock().unwrap();
            guard.take().ok_or_else(|| anyhow::anyhow!("Config não inicializada"))?
        };

        let (shutdown_tx, shutdown_rx) = std::sync::mpsc::channel::<()>();
        let shutdown_tx = Arc::new(Mutex::new(Some(shutdown_tx)));

        let event_handler = move |control_event| match control_event {
            ServiceControl::Stop | ServiceControl::Shutdown => {
                let mut guard = shutdown_tx.lock().unwrap();
                if let Some(tx) = guard.take() {
                    let _ = tx.send(());
                }
                ServiceControlHandlerResult::NoError
            }
            _ => ServiceControlHandlerResult::NotImplemented,
        };

        let status_handle = service_control_handler::register(SERVICE_NAME, event_handler)
            .map_err(|e| anyhow::anyhow!("Registrando service control handler: {}", e))?;

        status_handle
            .set_service_status(ServiceStatus {
                service_type: ServiceType::OWN_PROCESS,
                current_state: ServiceState::Running,
                controls_accepted: ServiceControlAccept::STOP | ServiceControlAccept::SHUTDOWN,
                exit_code: ServiceExitCode::Win32(0),
                checkpoint: 0,
                wait_hint: std::time::Duration::default(),
                process_id: None,
            })
            .map_err(|e| anyhow::anyhow!("Setando status Running: {}", e))?;

        let runtime = tokio::runtime::Runtime::new()?;
        runtime.block_on(async move {
            tokio::select! {
                result = run_server(cfg) => {
                    if let Err(e) = result {
                        tracing::error!("run_server PKI encerrou com erro: {:#}", e);
                    }
                }
                _ = tokio::task::spawn_blocking(move || {
                    let _ = shutdown_rx.recv();
                }) => {
                    tracing::info!("Sinal de parada recebido do SCM");
                }
            }
        });

        let _ = status_handle.set_service_status(ServiceStatus {
            service_type: ServiceType::OWN_PROCESS,
            current_state: ServiceState::Stopped,
            controls_accepted: ServiceControlAccept::empty(),
            exit_code: ServiceExitCode::Win32(0),
            checkpoint: 0,
            wait_hint: std::time::Duration::default(),
            process_id: None,
        });

        Ok(())
    }
}
