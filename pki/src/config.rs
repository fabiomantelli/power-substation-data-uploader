use anyhow::{Context, Result};
use serde::Deserialize;
use std::path::PathBuf;

#[derive(Debug, Deserialize, Clone)]
pub struct PkiConfig {
    pub listen_addr: String,
    /// Certificado da CA intermediária (público, enviado na chain)
    pub ca_cert_pem: PathBuf,
    /// Chave privada da CA intermediária (MUITO SENSÍVEL)
    pub ca_key_pem: PathBuf,
    /// Certificado TLS do próprio servidor PKI
    pub server_cert_pem: PathBuf,
    /// Chave TLS do próprio servidor PKI
    pub server_key_pem: PathBuf,
    /// CA chain para verificar certificados cliente (mTLS)
    #[allow(dead_code)]
    pub ca_bundle_pem: PathBuf,
    /// Subestações autorizadas a renovar (lista de CNs)
    pub allowed_station_ids: Vec<String>,
    /// Máximo de dias restantes para permitir renovação (ex: 60)
    pub renewal_window_days_max: i64,
    /// Validade dos certificados emitidos em dias
    pub issued_cert_validity_days: u32,
    pub audit_dir: PathBuf,
    pub log_dir: PathBuf,
}

impl PkiConfig {
    pub fn load(path: &std::path::Path) -> Result<Self> {
        let content = std::fs::read_to_string(path)
            .with_context(|| format!("Lendo config PKI: {}", path.display()))?;
        toml::from_str(&content)
            .with_context(|| format!("Parseando config PKI: {}", path.display()))
    }

    pub fn ensure_dirs(&self) -> Result<()> {
        for dir in [&self.audit_dir, &self.log_dir] {
            std::fs::create_dir_all(dir)
                .with_context(|| format!("Criando dir: {}", dir.display()))?;
        }
        Ok(())
    }
}
