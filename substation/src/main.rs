mod config;
mod manifest;
mod queue;
mod renewal;
mod retention;
mod sender;
mod service;
mod watcher;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use std::{path::PathBuf, sync::Arc};
use tracing::{error, info};
use tracing_appender::rolling;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

#[derive(Parser)]
#[command(name = "osc-agent", about = "Agente de transferência de oscilografias")]
struct Cli {
    #[arg(short, long, default_value = "config/agent.toml")]
    config: PathBuf,

    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Executa como processo normal (foreground)
    Run,
    /// Executa sob controle do Windows Service Control Manager (chamado pelo SCM)
    #[cfg(windows)]
    RunService,
    /// Instala como Windows Service
    #[cfg(windows)]
    InstallService,
    /// Remove o Windows Service
    #[cfg(windows)]
    UninstallService,
    /// Verifica status da fila
    Status,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    let cfg = config::AgentConfig::load(&cli.config)
        .with_context(|| format!("Carregando configuração: {}", cli.config.display()))?;

    cfg.ensure_dirs()?;

    // Configurar logging
    let file_appender = rolling::daily(&cfg.log_dir, "osc-agent.log");
    let (non_blocking, _guard) = tracing_appender::non_blocking(file_appender);

    tracing_subscriber::registry()
        .with(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
        .with(tracing_subscriber::fmt::layer().with_writer(non_blocking).json())
        .with(tracing_subscriber::fmt::layer().with_writer(std::io::stderr))
        .init();

    info!(
        station_id = %cfg.station_id,
        device_id = %cfg.device_id,
        "osc-agent iniciando"
    );

    match cli.command.unwrap_or(Commands::Run) {
        Commands::Run => run_agent(cfg).await?,

        #[cfg(windows)]
        Commands::RunService => {
            service::windows_service::run_as_service(cfg)?;
        }

        #[cfg(windows)]
        Commands::InstallService => {
            install_service()?;
        }

        #[cfg(windows)]
        Commands::UninstallService => {
            uninstall_service()?;
        }

        Commands::Status => {
            let queue = queue::LocalQueue::new(cfg.queue_dir.clone());
            let pending = queue.count_pending();
            println!("Itens pendentes na fila: {}", pending);
        }
    }

    Ok(())
}

pub async fn run_agent(cfg: config::AgentConfig) -> Result<()> {
    let cfg = Arc::new(cfg);
    let queue = Arc::new(queue::LocalQueue::new(cfg.queue_dir.clone()));

    let watcher = Arc::new(watcher::InboxWatcher::new(cfg.clone(), queue.clone()));
    let sender = sender::Sender::new((*cfg).clone(), queue::LocalQueue::new(cfg.queue_dir.clone()))?;
    let retention = retention::RetentionManager::new((*cfg).clone());
    let renewal = renewal::RenewalTask::new(cfg.clone());

    tokio::select! {
        result = async { watcher.run_loop().await } => {
            if let Err(e) = result {
                error!("Watcher encerrou com erro: {:#}", e);
            }
        }
        _ = sender.run_loop() => {}
        _ = retention.run_loop() => {}
        _ = renewal.run_loop() => {}
        _ = tokio::signal::ctrl_c() => {
            info!("Sinal de encerramento recebido");
        }
    }

    info!("osc-agent encerrado");
    Ok(())
}

#[cfg(windows)]
fn install_service() -> Result<()> {
    use std::ffi::OsString;
    use windows_service::service::{
        ServiceAccess, ServiceErrorControl, ServiceInfo, ServiceStartType, ServiceType,
    };
    use windows_service::service_manager::{ServiceManager, ServiceManagerAccess};

    let manager = ServiceManager::local_computer(None::<&str>, ServiceManagerAccess::CREATE_SERVICE)?;
    let exe_path = std::env::current_exe()?;

    let service_info = ServiceInfo {
        name: OsString::from("OscAgent"),
        display_name: OsString::from("OSC Agent - Transferência de Oscilografias"),
        service_type: ServiceType::OWN_PROCESS,
        start_type: ServiceStartType::AutoStart,
        error_control: ServiceErrorControl::Normal,
        executable_path: exe_path,
        launch_arguments: vec![OsString::from("run-service")],
        dependencies: vec![],
        account_name: None,
        account_password: None,
    };

    manager.create_service(&service_info, ServiceAccess::empty())?;
    println!("Serviço OscAgent instalado com sucesso.");
    Ok(())
}

#[cfg(windows)]
fn uninstall_service() -> Result<()> {
    use windows_service::service::ServiceAccess;
    use windows_service::service_manager::{ServiceManager, ServiceManagerAccess};

    let manager = ServiceManager::local_computer(None::<&str>, ServiceManagerAccess::CONNECT)?;
    let service = manager.open_service("OscAgent", ServiceAccess::DELETE)?;
    service.delete()?;
    println!("Serviço OscAgent removido.");
    Ok(())
}
