use chrono::Utc;
use rcgen::{
    CertificateParams, CertificateSigningRequestParams, DnType, DnValue, KeyPair,
};
use thiserror::Error;
use x509_parser::prelude::*;

// Usar ::time para evitar ambiguidade com o módulo `time` re-exportado pelo x509_parser::prelude
use ::time::OffsetDateTime;

#[derive(Debug, Error)]
pub enum RenewalError {
    #[error(
        "Renovação prematura: {days_remaining} dias restantes, janela máxima: {max_window} dias"
    )]
    TooEarlyToRenew { days_remaining: i64, max_window: i64 },

    #[error("CN do CSR '{csr_cn}' não corresponde ao CN do cliente '{client_cn}'")]
    CnMismatch { csr_cn: String, client_cn: String },

    #[error("Estação não autorizada: {station_id}")]
    StationNotAllowed { station_id: String },

    #[error("Erro ao parsear CSR: {0}")]
    CsrParseError(String),

    #[error("Erro ao assinar certificado: {0}")]
    SigningError(String),

    #[error("Certificado cliente inválido: {0}")]
    InvalidClientCert(String),
}

#[derive(Debug)]
pub struct ValidatedClient {
    pub station_id: String,
    pub cert_expiry_utc: chrono::DateTime<Utc>,
    pub days_remaining: i64,
}

/// Valida o certificado cliente (DER): CN na allowlist e dentro da janela de renovação.
pub fn validate_client_cert(
    cert_der: &[u8],
    allowed_station_ids: &[String],
    renewal_window_days_max: i64,
) -> Result<ValidatedClient, RenewalError> {
    let (_, cert) = parse_x509_certificate(cert_der)
        .map_err(|e| RenewalError::InvalidClientCert(e.to_string()))?;

    // Extrair CN — bind a let para resolver lifetime com temporários do iterator
    let cn = {
        let result = cert
            .subject()
            .iter_common_name()
            .next()
            .and_then(|attr| attr.as_str().ok())
            .map(|s| s.to_string())
            .unwrap_or_default();
        result
    };

    if cn.is_empty() {
        return Err(RenewalError::InvalidClientCert(
            "CN vazio no certificado cliente".to_string(),
        ));
    }

    // Verificar allowlist
    if !allowed_station_ids.iter().any(|id| id == &cn) {
        return Err(RenewalError::StationNotAllowed { station_id: cn });
    }

    // Calcular dias restantes
    let not_after_ts = cert.validity().not_after.timestamp();
    let not_after_dt = chrono::DateTime::from_timestamp(not_after_ts, 0)
        .unwrap_or_else(Utc::now);
    let days_remaining = (not_after_dt - Utc::now()).num_days();

    // Rejeitar se ainda falta muito tempo (fora da janela)
    if days_remaining > renewal_window_days_max {
        return Err(RenewalError::TooEarlyToRenew {
            days_remaining,
            max_window: renewal_window_days_max,
        });
    }

    Ok(ValidatedClient {
        station_id: cn,
        cert_expiry_utc: not_after_dt,
        days_remaining,
    })
}

/// Assina um CSR com a CA intermediária.
/// Verifica que o CN do CSR corresponde a expected_station_id.
pub fn sign_csr(
    csr_pem: &str,
    expected_station_id: &str,
    validity_days: u32,
    ca_cert_pem: &str,
    ca_key_pem: &str,
) -> Result<String, RenewalError> {
    // Parsear CSR
    let mut csr = CertificateSigningRequestParams::from_pem(csr_pem)
        .map_err(|e| RenewalError::CsrParseError(e.to_string()))?;

    // Verificar CN do CSR (double-deref: iter retorna (&DnType, &DnValue))
    let csr_cn = csr
        .params
        .distinguished_name
        .iter()
        .find(|(t, _)| **t == DnType::CommonName)
        .map(|(_, v)| dn_value_to_string(v))
        .unwrap_or_default();

    if csr_cn != expected_station_id {
        return Err(RenewalError::CnMismatch {
            csr_cn,
            client_cn: expected_station_id.to_string(),
        });
    }

    // A CA define a validade — o CSR não carrega essa informação
    let now = OffsetDateTime::now_utc();
    csr.params.not_before = now;
    csr.params.not_after = now + ::time::Duration::days(validity_days as i64);

    // Carregar CA key e cert
    let ca_key = KeyPair::from_pem(ca_key_pem)
        .map_err(|e| RenewalError::SigningError(format!("Carregando chave CA: {}", e)))?;

    let ca_params = CertificateParams::from_ca_cert_pem(ca_cert_pem)
        .map_err(|e| RenewalError::SigningError(format!("Carregando cert CA: {}", e)))?;

    // Recriar o cert CA internamente para uso como issuer no rcgen
    let ca_cert = ca_params
        .self_signed(&ca_key)
        .map_err(|e| RenewalError::SigningError(format!("Re-assinando CA: {}", e)))?;

    // Assinar o CSR — retorna Certificate
    let new_cert = csr
        .signed_by(&ca_cert, &ca_key)
        .map_err(|e| RenewalError::SigningError(e.to_string()))?;

    // Certificate::pem() retorna String diretamente (rcgen 0.13)
    Ok(new_cert.pem())
}

/// Extrai o CN de um certificado DER (para uso em logs).
#[allow(dead_code)]
pub fn extract_cn_from_der(cert_der: &[u8]) -> Option<String> {
    let (_, cert) = parse_x509_certificate(cert_der).ok()?;
    // Bind a variável antes de retornar para evitar lifetime issue com temporários do iterator
    let result = cert
        .subject()
        .iter_common_name()
        .next()
        .and_then(|attr| attr.as_str().ok())
        .map(|s| s.to_string());
    result
}

/// Calcula a data de expiração de um certificado emitido agora com N dias de validade.
pub fn cert_expiry_utc(validity_days: u32) -> chrono::DateTime<Utc> {
    Utc::now() + chrono::Duration::days(validity_days as i64)
}

/// Extrai o conteúdo string de um DnValue de forma segura para qualquer variante.
fn dn_value_to_string(v: &DnValue) -> String {
    match v {
        DnValue::Utf8String(s) => s.clone(),
        DnValue::PrintableString(s) => s.as_str().to_string(),
        DnValue::Ia5String(s) => s.as_str().to_string(),
        // UniversalString, TeletexString, BmpString — raros em CNs modernos
        _ => String::new(),
    }
}
