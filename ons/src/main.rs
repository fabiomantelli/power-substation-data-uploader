mod api;
mod audit;
mod config;
mod service;
mod storage;
mod tls;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use std::{net::SocketAddr, path::PathBuf, sync::Arc};
use tracing::info;
use tracing_appender::rolling;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

#[derive(Parser)]
#[command(name = "osc-server", about = "Servidor ONS de recepção de oscilografias")]
struct Cli {
    #[arg(short, long, default_value = "config/server.toml")]
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
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    let cfg = config::ServerConfig::load(&cli.config)
        .with_context(|| format!("Carregando config: {}", cli.config.display()))?;

    cfg.ensure_dirs()?;

    // Logging
    let file_appender = rolling::daily(&cfg.log_dir, "osc-server.log");
    let (non_blocking, _guard) = tracing_appender::non_blocking(file_appender);

    tracing_subscriber::registry()
        .with(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
        .with(tracing_subscriber::fmt::layer().with_writer(non_blocking).json())
        .with(tracing_subscriber::fmt::layer().with_writer(std::io::stderr))
        .init();

    match cli.command.unwrap_or(Commands::Run) {
        Commands::Run => run_server(cfg).await?,

        #[cfg(windows)]
        Commands::RunService => {
            service::windows_service::run_as_service(cfg)?;
        }
    }

    Ok(())
}

pub async fn run_server(cfg: config::ServerConfig) -> Result<()> {
    info!(listen_addr = %cfg.listen_addr, "osc-server iniciando");

    let audit = Arc::new(audit::AuditLogger::new(cfg.audit_dir.clone()));
    let storage = Arc::new(storage::StorageManager::new(
        cfg.staging_dir.clone(),
        cfg.repository_dir.clone(),
        cfg.quarantine_dir.clone(),
        audit::AuditLogger::new(cfg.audit_dir.clone()),
    ));

    let state = api::AppState {
        config: Arc::new(cfg.clone()),
        storage,
        audit,
    };

    let app = api::router(state);
    let addr: SocketAddr = cfg.listen_addr.parse().context("Parseando endereço")?;

    let tls_config = tls::build_tls_config(
        &cfg.server_cert_pem,
        &cfg.server_key_pem,
        &cfg.ca_bundle_pem,
    )
    .await?;

    info!(%addr, "Aguardando conexões HTTPS/mTLS");

    axum_server::bind_rustls(addr, tls_config)
        .serve(app.into_make_service())
        .await
        .context("Erro no servidor HTTP")?;

    Ok(())
}
