use anyhow::{Context, Result};
use serde::Deserialize;
use std::path::PathBuf;

#[derive(Debug, Deserialize, Clone)]
#[allow(dead_code)]
pub struct ServerConfig {
    pub listen_addr: String,
    pub server_cert_pem: PathBuf,
    pub server_key_pem: PathBuf,
    pub ca_bundle_pem: PathBuf,
    pub staging_dir: PathBuf,
    pub repository_dir: PathBuf,
    pub quarantine_dir: PathBuf,
    pub audit_dir: PathBuf,
    pub log_dir: PathBuf,
    pub max_upload_size_mb: u64,
    pub allowed_station_ids: Vec<String>,
    pub upload_timeout_seconds: u64,
    pub rate_limit: RateLimitConfig,
}

#[derive(Debug, Deserialize, Clone)]
#[allow(dead_code)]
pub struct RateLimitConfig {
    /// Uploads máximos por minuto por station_id
    pub max_uploads_per_minute: u32,
    /// Tamanho máximo de burst
    pub burst_size: u32,
}

impl ServerConfig {
    pub fn load(path: &std::path::Path) -> Result<Self> {
        let content = std::fs::read_to_string(path)
            .with_context(|| format!("Lendo config: {}", path.display()))?;
        toml::from_str(&content)
            .with_context(|| format!("Parseando config: {}", path.display()))
    }

    pub fn ensure_dirs(&self) -> Result<()> {
        for dir in [
            &self.staging_dir,
            &self.repository_dir,
            &self.quarantine_dir,
            &self.audit_dir,
            &self.log_dir,
        ] {
            std::fs::create_dir_all(dir)
                .with_context(|| format!("Criando dir: {}", dir.display()))?;
        }
        Ok(())
    }
}
