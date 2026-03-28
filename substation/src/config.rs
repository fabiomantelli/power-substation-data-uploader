use anyhow::{Context, Result};
use serde::Deserialize;
use std::path::PathBuf;

#[derive(Debug, Deserialize, Clone)]
pub struct AgentConfig {
    pub station_id: String,
    pub device_id: String,
    pub inbox_dir: PathBuf,
    pub queue_dir: PathBuf,
    pub sent_dir: PathBuf,
    pub error_dir: PathBuf,
    pub spool_dir: PathBuf,
    pub log_dir: PathBuf,
    pub state_dir: PathBuf,
    pub server_url: String,
    pub client_cert_pem: PathBuf,
    pub client_key_pem: PathBuf,
    pub ca_bundle_pem: PathBuf,
    pub max_retries: u32,
    pub retry_initial_backoff_seconds: u64,
    pub retry_max_backoff_seconds: u64,
    pub upload_timeout_seconds: u64,
    pub watch_debounce_ms: u64,
    pub retention: RetentionConfig,
    pub disk: DiskConfig,
    pub renewal: RenewalConfig,
}

#[derive(Debug, Deserialize, Clone)]
pub struct RetentionConfig {
    /// Dias para manter arquivos em sent/
    pub sent_retention_days: u32,
    /// Dias máximos para manter em sent/ (se disco permitir)
    pub sent_retention_max_days: u32,
    /// Dias para manter em error/
    pub error_retention_days: u32,
    /// Dias para manter logs
    pub log_retention_days: u32,
    /// Intervalo de limpeza em minutos
    pub cleanup_interval_minutes: u64,
}

#[derive(Debug, Deserialize, Clone)]
pub struct DiskConfig {
    /// Disco a monitorar (ex: "D:")
    pub drive: String,
    /// Percentual de uso que dispara alerta (ex: 70)
    pub warn_threshold_pct: u64,
    /// Percentual que reduz retenção para mínimo (ex: 80)
    pub reduce_retention_threshold_pct: u64,
    /// Percentual que força limpeza agressiva (ex: 90)
    pub force_cleanup_threshold_pct: u64,
    /// Espaço livre mínimo a preservar em MB
    pub min_free_mb: u64,
}

#[derive(Debug, Deserialize, Clone)]
pub struct RenewalConfig {
    /// URL base do servidor PKI (ex: "https://pki.ons.intra:8444")
    pub pki_server_url: String,
    /// Dias antes da expiração para iniciar renovação (ex: 30)
    pub renewal_days_before_expiry: i64,
    /// Intervalo de verificação em horas (ex: 6)
    pub check_interval_hours: u64,
    /// Caminho para salvar a nova chave privada gerada
    pub new_key_path: PathBuf,
    /// Caminho para salvar o novo certificado recebido
    pub new_cert_path: PathBuf,
}

impl AgentConfig {
    pub fn load(path: &std::path::Path) -> Result<Self> {
        let content = std::fs::read_to_string(path)
            .with_context(|| format!("Lendo config: {}", path.display()))?;
        toml::from_str(&content)
            .with_context(|| format!("Parseando config: {}", path.display()))
    }

    pub fn ensure_dirs(&self) -> Result<()> {
        for dir in [
            &self.inbox_dir,
            &self.queue_dir,
            &self.sent_dir,
            &self.error_dir,
            &self.spool_dir,
            &self.log_dir,
            &self.state_dir,
        ] {
            std::fs::create_dir_all(dir)
                .with_context(|| format!("Criando dir: {}", dir.display()))?;
        }
        Ok(())
    }
}
