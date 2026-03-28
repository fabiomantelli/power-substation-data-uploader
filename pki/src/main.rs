mod api;
mod audit;
mod ca;
mod config;
mod init;
mod service;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use std::{net::SocketAddr, path::PathBuf, sync::Arc};
use tracing::info;
use tracing_appender::rolling;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

#[derive(Parser)]
#[command(
    name = "osc-pki-server",
    about = "Servidor PKI para renovação automática de certificados de subestações"
)]
struct Cli {
    #[arg(short, long, default_value = "config/pki.toml")]
    config: PathBuf,

    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Executa o servidor PKI em foreground
    Run,
    /// Executa sob controle do Windows Service Control Manager (chamado pelo SCM)
    #[cfg(windows)]
    RunService,
    /// Gera toda a hierarquia PKI (Root CA, Intermediate CA, certs de servidor)
    InitPki(init::InitPkiArgs),
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    // InitPki não precisa de config nem de logging rotativo
    if let Some(Commands::InitPki(args)) = cli.command {
        return init::run_init_pki(args);
    }

    let cfg = config::PkiConfig::load(&cli.config)
        .with_context(|| format!("Carregando config PKI: {}", cli.config.display()))?;

    cfg.ensure_dirs()?;

    // Logging rotativo diário
    let file_appender = rolling::daily(&cfg.log_dir, "osc-pki-server.log");
    let (non_blocking, _guard) = tracing_appender::non_blocking(file_appender);

    tracing_subscriber::registry()
        .with(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
        .with(
            tracing_subscriber::fmt::layer()
                .with_writer(non_blocking)
                .json(),
        )
        .with(tracing_subscriber::fmt::layer().with_writer(std::io::stderr))
        .init();

    match cli.command.unwrap_or(Commands::Run) {
        Commands::Run => run_server(cfg).await?,

        #[cfg(windows)]
        Commands::RunService => {
            service::windows_service::run_as_service(cfg)?;
        }

        // InitPki já tratado acima
        Commands::InitPki(_) => unreachable!(),
    }

    Ok(())
}

pub async fn run_server(cfg: config::PkiConfig) -> Result<()> {
    info!(listen_addr = %cfg.listen_addr, "osc-pki-server iniciando");

    let ca_cert_pem = std::fs::read_to_string(&cfg.ca_cert_pem)
        .with_context(|| format!("Lendo CA cert: {}", cfg.ca_cert_pem.display()))?;
    let ca_key_pem = std::fs::read_to_string(&cfg.ca_key_pem)
        .with_context(|| format!("Lendo CA key: {}", cfg.ca_key_pem.display()))?;

    let cert_data = std::fs::read(&cfg.server_cert_pem)
        .with_context(|| format!("Lendo cert servidor: {}", cfg.server_cert_pem.display()))?;
    let key_data = std::fs::read(&cfg.server_key_pem)
        .with_context(|| format!("Lendo chave servidor: {}", cfg.server_key_pem.display()))?;

    let audit = Arc::new(audit::PkiAuditLogger::new(cfg.audit_dir.clone()));

    let state = api::AppState {
        config: Arc::new(cfg.clone()),
        audit,
        ca_cert_pem,
        ca_key_pem,
    };

    let app = api::router(state);
    let addr: SocketAddr = cfg.listen_addr.parse().context("Parseando endereço")?;

    let tls_config = axum_server::tls_rustls::RustlsConfig::from_pem(cert_data, key_data)
        .await
        .context("Construindo config TLS do servidor PKI")?;

    info!(%addr, "Aguardando conexões HTTPS para renovação de certificados");

    axum_server::bind_rustls(addr, tls_config)
        .serve(app.into_make_service())
        .await
        .context("Erro no servidor PKI")?;

    Ok(())
}
