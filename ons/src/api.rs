use crate::{
    audit::{AuditEvent, AuditLogger},
    config::ServerConfig,
    storage::StorageManager,
};
use axum::{
    extract::{Multipart, State},
    http::StatusCode,
    response::Json,
    routing::{get, post},
    Router,
};
use serde::Serialize;
use std::sync::Arc;
use tracing::{error, info, warn};
use uuid::Uuid;

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<ServerConfig>,
    pub storage: Arc<StorageManager>,
    pub audit: Arc<AuditLogger>,
}

#[derive(Serialize)]
pub struct UploadResponse {
    pub upload_id: String,
    pub status: String,
    pub hash_verified: bool,
    pub stored_path: Option<String>,
    pub message: Option<String>,
}

#[derive(Serialize)]
pub struct HealthResponse {
    pub status: String,
    pub version: &'static str,
}

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/api/v1/upload", post(handle_upload))
        .route("/health", get(handle_health))
        .with_state(state)
        .layer(
            tower_http::limit::RequestBodyLimitLayer::new(
                200 * 1024 * 1024, // 200 MB
            ),
        )
}

async fn handle_health() -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok".to_string(),
        version: env!("CARGO_PKG_VERSION"),
    })
}

async fn handle_upload(
    State(state): State<AppState>,
    mut multipart: Multipart,
) -> Result<Json<UploadResponse>, (StatusCode, Json<UploadResponse>)> {
    let upload_id = Uuid::new_v4().to_string();

    // Parse do multipart
    let mut station_id: Option<String> = None;
    let mut _device_id: Option<String> = None;
    let mut event_id: Option<String> = None;
    let mut _timestamp_utc: Option<String> = None;
    let mut manifest_json: Option<String> = None;
    // (name, data, declared_sha256_from_manifest)
    let mut uploaded_files: Vec<(String, Vec<u8>)> = Vec::new();

    while let Ok(Some(field)) = multipart.next_field().await {
        let field_name = field.name().unwrap_or("").to_string();
        match field_name.as_str() {
            "station_id" => {
                station_id = field.text().await.ok();
            }
            "device_id" => {
                _device_id = field.text().await.ok();
            }
            "event_id" => {
                event_id = field.text().await.ok();
            }
            "timestamp_utc" => {
                _timestamp_utc = field.text().await.ok();
            }
            "manifest" => {
                manifest_json = field.text().await.ok();
            }
            _ => {
                // Arquivo
                if let Ok(data) = field.bytes().await {
                    uploaded_files.push((field_name, data.to_vec()));
                }
            }
        }
    }

    // Validar campos obrigatórios
    let station_id = match station_id {
        Some(s) if !s.is_empty() => s,
        _ => {
            return Err(reject(
                &upload_id,
                StatusCode::BAD_REQUEST,
                "station_id ausente",
            ));
        }
    };

    let event_id = match event_id {
        Some(e) if !e.is_empty() => e,
        _ => {
            return Err(reject(
                &upload_id,
                StatusCode::BAD_REQUEST,
                "event_id ausente",
            ));
        }
    };

    let manifest_json = match manifest_json {
        Some(m) if !m.is_empty() => m,
        _ => {
            return Err(reject(
                &upload_id,
                StatusCode::BAD_REQUEST,
                "manifest ausente",
            ));
        }
    };

    // Validar station_id autorizada
    if !state.config.allowed_station_ids.contains(&station_id) {
        warn!(station_id = %station_id, "Estação não autorizada");
        state.audit.log(AuditEvent::UploadRejected {
            upload_id: upload_id.clone(),
            event_id: event_id.clone(),
            station_id: station_id.clone(),
            reason: "Estação não autorizada".to_string(),
        });
        return Err(reject(
            &upload_id,
            StatusCode::FORBIDDEN,
            "Estação não autorizada",
        ));
    }

    // Verificar duplicata
    if state.storage.is_duplicate(&event_id) {
        info!(event_id = %event_id, "Upload duplicado ignorado");
        state.audit.log(AuditEvent::DuplicateDetected {
            upload_id: upload_id.clone(),
            event_id: event_id.clone(),
            station_id: station_id.clone(),
        });
        return Ok(Json(UploadResponse {
            upload_id,
            status: "duplicate".to_string(),
            hash_verified: true,
            stored_path: None,
            message: Some("Evento já recebido anteriormente".to_string()),
        }));
    }

    // Parse manifesto para obter hashes declarados
    let manifest: serde_json::Value = match serde_json::from_str(&manifest_json) {
        Ok(m) => m,
        Err(e) => {
            return Err(reject(
                &upload_id,
                StatusCode::BAD_REQUEST,
                &format!("Manifesto inválido: {}", e),
            ));
        }
    };

    // Construir lista de arquivos com hash declarado
    let files_with_hashes: Vec<(String, Vec<u8>, String)> = uploaded_files
        .into_iter()
        .map(|(name, data)| {
            let declared_hash = manifest["files"]
                .as_array()
                .and_then(|arr| {
                    arr.iter()
                        .find(|f| f["name"].as_str() == Some(&name))
                        .and_then(|f| f["sha256"].as_str())
                        .map(|s| s.to_string())
                })
                .unwrap_or_default();
            (name, data, declared_hash)
        })
        .collect();

    state.audit.log(AuditEvent::UploadReceived {
        upload_id: upload_id.clone(),
        event_id: event_id.clone(),
        station_id: station_id.clone(),
        client_cn: "unknown".to_string(), // TODO: extrair do mTLS context
        file_count: files_with_hashes.len(),
        total_bytes: files_with_hashes.iter().map(|(_, d, _)| d.len() as u64).sum(),
    });

    // Armazenar
    match state
        .storage
        .store_upload(
            &upload_id,
            &event_id,
            &station_id,
            &manifest_json,
            files_with_hashes,
        )
    {
        Ok(record) => {
            Ok(Json(UploadResponse {
                upload_id,
                status: "accepted".to_string(),
                hash_verified: true,
                stored_path: Some(record.repository_path),
                message: None,
            }))
        }
        Err(e) => {
            error!(
                upload_id = %upload_id,
                event_id = %event_id,
                error = %e,
                "Erro armazenando upload"
            );
            Err(reject(
                &upload_id,
                StatusCode::INTERNAL_SERVER_ERROR,
                &format!("Erro interno: {}", e),
            ))
        }
    }
}

fn reject(
    upload_id: &str,
    status: StatusCode,
    reason: &str,
) -> (StatusCode, Json<UploadResponse>) {
    (
        status,
        Json(UploadResponse {
            upload_id: upload_id.to_string(),
            status: "rejected".to_string(),
            hash_verified: false,
            stored_path: None,
            message: Some(reason.to_string()),
        }),
    )
}
