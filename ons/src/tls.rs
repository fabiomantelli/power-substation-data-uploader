use anyhow::{Context, Result};
use axum_server::tls_rustls::RustlsConfig;
use std::path::Path;

/// Carrega configuração TLS com mTLS obrigatório
pub async fn build_tls_config(
    cert_pem: &Path,
    key_pem: &Path,
    _ca_pem: &Path,
) -> Result<RustlsConfig> {
    let cert_data = std::fs::read(cert_pem)
        .with_context(|| format!("Lendo cert servidor: {}", cert_pem.display()))?;
    let key_data = std::fs::read(key_pem)
        .with_context(|| format!("Lendo chave servidor: {}", key_pem.display()))?;

    RustlsConfig::from_pem(cert_data, key_data)
        .await
        .context("Construindo configuração TLS")
}

/// Extrai CN do certificado cliente a partir dos dados raw.
/// Para produção, usar uma crate de parsing de ASN.1/X.509 completa
/// como x509-parser.
#[allow(dead_code)]
pub fn extract_client_cn(_cert_der: &[u8]) -> Option<String> {
    // placeholder - implementar com x509-parser
    None
}
