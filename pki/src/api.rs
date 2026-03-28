use crate::{
    audit::{PkiAuditEvent, PkiAuditLogger},
    ca::{self, RenewalError},
    config::PkiConfig,
};
use axum::{
    extract::{Multipart, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use serde::Serialize;
use std::sync::Arc;
use tracing::{error, info, warn};
use uuid::Uuid;

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<PkiConfig>,
    pub audit: Arc<PkiAuditLogger>,
    pub ca_cert_pem: String,
    pub ca_key_pem: String,
}

#[derive(Serialize)]
pub struct RenewResponse {
    pub request_id: String,
    pub status: String,
    pub message: Option<String>,
}

#[derive(Serialize)]
pub struct HealthResponse {
    pub status: &'static str,
    pub version: &'static str,
}

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/api/v1/pki/renew", post(handle_renew))
        .route("/health", get(handle_health))
        .with_state(state)
}

async fn handle_health() -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok",
        version: env!("CARGO_PKG_VERSION"),
    })
}

async fn handle_renew(
    State(state): State<AppState>,
    mut multipart: Multipart,
) -> Result<Response, (StatusCode, Json<RenewResponse>)> {
    let request_id = Uuid::new_v4().to_string();

    let mut station_id: Option<String> = None;
    let mut current_cert_pem: Option<String> = None;
    let mut csr_pem: Option<String> = None;

    while let Ok(Some(field)) = multipart.next_field().await {
        match field.name().unwrap_or("") {
            "station_id" => station_id = field.text().await.ok(),
            "current_cert_pem" => current_cert_pem = field.text().await.ok(),
            "csr_pem" => csr_pem = field.text().await.ok(),
            _ => {}
        }
    }

    let station_id = match station_id.filter(|s| !s.is_empty()) {
        Some(s) => s,
        None => return Err(reject(&request_id, StatusCode::BAD_REQUEST, "station_id ausente")),
    };

    let current_cert_pem = match current_cert_pem.filter(|s| !s.is_empty()) {
        Some(s) => s,
        None => {
            return Err(reject(
                &request_id,
                StatusCode::BAD_REQUEST,
                "current_cert_pem ausente",
            ))
        }
    };

    let csr_pem = match csr_pem.filter(|s| !s.is_empty()) {
        Some(s) => s,
        None => return Err(reject(&request_id, StatusCode::BAD_REQUEST, "csr_pem ausente")),
    };

    // Parsear PEM do cert atual para DER
    let cert_der = match pem::parse(&current_cert_pem)
        .ok()
        .map(|p| p.into_contents())
    {
        Some(d) => d,
        None => {
            return Err(reject(
                &request_id,
                StatusCode::BAD_REQUEST,
                "current_cert_pem inválido",
            ))
        }
    };

    // Validar cert cliente: CN na allowlist + janela de renovação
    let validated = match ca::validate_client_cert(
        &cert_der,
        &state.config.allowed_station_ids,
        state.config.renewal_window_days_max,
    ) {
        Ok(v) => v,
        Err(RenewalError::TooEarlyToRenew {
            days_remaining,
            max_window,
        }) => {
            warn!(
                station_id = %station_id,
                days_remaining,
                "Renovação rejeitada — fora da janela"
            );
            state.audit.log(PkiAuditEvent::RenewalRejected {
                request_id: request_id.clone(),
                station_id: station_id.clone(),
                client_cn: station_id.clone(),
                reason: format!(
                    "{} dias restantes — janela máxima: {} dias",
                    days_remaining, max_window
                ),
            });
            return Err(reject(
                &request_id,
                StatusCode::CONFLICT,
                &format!(
                    "{} dias restantes — renovação só permitida nos últimos {} dias",
                    days_remaining, max_window
                ),
            ));
        }
        Err(RenewalError::StationNotAllowed { station_id: cn }) => {
            warn!(cn = %cn, "Estação não autorizada");
            state.audit.log(PkiAuditEvent::RenewalRejected {
                request_id: request_id.clone(),
                station_id: cn.clone(),
                client_cn: cn.clone(),
                reason: "Estação não autorizada".to_string(),
            });
            return Err(reject(&request_id, StatusCode::FORBIDDEN, "Estação não autorizada"));
        }
        Err(e) => {
            warn!(error = %e, "Certificado cliente inválido");
            return Err(reject(
                &request_id,
                StatusCode::BAD_REQUEST,
                &format!("Certificado inválido: {}", e),
            ));
        }
    };

    // Verificar consistência: station_id do body == CN do cert
    if validated.station_id != station_id {
        warn!(
            body_station_id = %station_id,
            cert_cn = %validated.station_id,
            "station_id inconsistente com CN do certificado"
        );
        return Err(reject(
            &request_id,
            StatusCode::FORBIDDEN,
            "station_id não corresponde ao CN do certificado",
        ));
    }

    state.audit.log(PkiAuditEvent::RenewalRequested {
        request_id: request_id.clone(),
        station_id: validated.station_id.clone(),
        client_cn: validated.station_id.clone(),
        client_cert_expiry_utc: validated.cert_expiry_utc.to_rfc3339(),
        days_remaining: validated.days_remaining,
    });

    info!(
        request_id = %request_id,
        station_id = %validated.station_id,
        days_remaining = validated.days_remaining,
        "Processando renovação de certificado"
    );

    // Assinar CSR com CA intermediária
    let new_cert_expiry = ca::cert_expiry_utc(state.config.issued_cert_validity_days);
    let new_cert_pem = match ca::sign_csr(
        &csr_pem,
        &validated.station_id,
        state.config.issued_cert_validity_days,
        &state.ca_cert_pem,
        &state.ca_key_pem,
    ) {
        Ok(pem) => pem,
        Err(RenewalError::CnMismatch { csr_cn, client_cn }) => {
            warn!(csr_cn = %csr_cn, client_cn = %client_cn, "CN mismatch no CSR");
            state.audit.log(PkiAuditEvent::RenewalRejected {
                request_id: request_id.clone(),
                station_id: validated.station_id.clone(),
                client_cn: validated.station_id.clone(),
                reason: format!(
                    "CN mismatch: CSR tem '{}', cliente é '{}'",
                    csr_cn, client_cn
                ),
            });
            return Err(reject(
                &request_id,
                StatusCode::FORBIDDEN,
                "CN do CSR não corresponde ao certificado cliente",
            ));
        }
        Err(e) => {
            error!(request_id = %request_id, error = %e, "Erro ao assinar CSR");
            state.audit.log(PkiAuditEvent::RenewalRejected {
                request_id: request_id.clone(),
                station_id: validated.station_id.clone(),
                client_cn: validated.station_id.clone(),
                reason: format!("Erro interno: {}", e),
            });
            return Err(reject(
                &request_id,
                StatusCode::INTERNAL_SERVER_ERROR,
                "Erro interno ao assinar certificado",
            ));
        }
    };

    state.audit.log(PkiAuditEvent::RenewalIssued {
        request_id: request_id.clone(),
        station_id: validated.station_id.clone(),
        client_cn: validated.station_id.clone(),
        new_cert_expiry_utc: new_cert_expiry.to_rfc3339(),
    });

    info!(
        request_id = %request_id,
        station_id = %validated.station_id,
        new_expiry = %new_cert_expiry.format("%Y-%m-%d"),
        "Certificado renovado com sucesso"
    );

    // Retornar novo cert PEM como texto puro
    Ok((
        StatusCode::OK,
        [("content-type", "application/x-pem-file")],
        new_cert_pem,
    )
        .into_response())
}

fn reject(
    request_id: &str,
    status: StatusCode,
    message: &str,
) -> (StatusCode, Json<RenewResponse>) {
    (
        status,
        Json(RenewResponse {
            request_id: request_id.to_string(),
            status: "rejected".to_string(),
            message: Some(message.to_string()),
        }),
    )
}
