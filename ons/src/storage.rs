use crate::audit::{AuditEvent, AuditLogger};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use tracing::{info, warn};

#[derive(Debug, Serialize, Deserialize)]
pub struct UploadRecord {
    pub upload_id: String,
    pub event_id: String,
    pub station_id: String,
    pub received_at_utc: chrono::DateTime<chrono::Utc>,
    pub files: Vec<StoredFile>,
    pub total_bytes: u64,
    pub repository_path: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct StoredFile {
    pub name: String,
    pub sha256_declared: String,
    pub sha256_verified: String,
    pub size_bytes: u64,
    pub hash_ok: bool,
}

pub struct StorageManager {
    staging_dir: PathBuf,
    repository_dir: PathBuf,
    quarantine_dir: PathBuf,
    audit: AuditLogger,
}

impl StorageManager {
    pub fn new(
        staging_dir: PathBuf,
        repository_dir: PathBuf,
        quarantine_dir: PathBuf,
        audit: AuditLogger,
    ) -> Self {
        Self {
            staging_dir,
            repository_dir,
            quarantine_dir,
            audit,
        }
    }

    /// Verifica se upload_id já existe no repositório (idempotência)
    pub fn is_duplicate(&self, event_id: &str) -> bool {
        // Busca por subdiretório com event_id no repositório
        self.repository_dir
            .read_dir()
            .ok()
            .map(|entries| {
                entries.flatten().any(|e| {
                    e.file_name()
                        .to_string_lossy()
                        .contains(event_id)
                })
            })
            .unwrap_or(false)
    }

    /// Processa e armazena um upload recebido
    pub fn store_upload(
        &self,
        upload_id: &str,
        event_id: &str,
        station_id: &str,
        manifest_json: &str,
        files: Vec<(String, Vec<u8>, String)>, // (name, data, declared_sha256)
    ) -> Result<UploadRecord> {
        // Criar área de staging
        let staging_path = self.staging_dir.join(upload_id);
        std::fs::create_dir_all(&staging_path)?;

        let mut stored_files = Vec::new();
        let mut all_hashes_ok = true;

        for (name, data, declared_sha256) in &files {
            let verified_sha256 = hex::encode(Sha256::digest(data));
            let hash_ok = *declared_sha256 == verified_sha256;

            if !hash_ok {
                warn!(
                    upload_id,
                    event_id,
                    file = %name,
                    declared = %declared_sha256,
                    verified = %verified_sha256,
                    "Hash mismatch detectado"
                );
                all_hashes_ok = false;
            }

            let file_path = staging_path.join(name);
            std::fs::write(&file_path, data)
                .with_context(|| format!("Gravando {} em staging", name))?;

            stored_files.push(StoredFile {
                name: name.clone(),
                sha256_declared: declared_sha256.clone(),
                sha256_verified: verified_sha256,
                size_bytes: data.len() as u64,
                hash_ok,
            });
        }

        // Salvar manifesto em staging
        std::fs::write(staging_path.join("manifest.json"), manifest_json)?;

        if !all_hashes_ok {
            // Mover para quarentena
            let quarantine_path = self.quarantine_dir.join(upload_id);
            std::fs::rename(&staging_path, &quarantine_path)
                .or_else(|_| copy_dir(&staging_path, &quarantine_path))?;

            self.audit.log(AuditEvent::UploadQuarantined {
                upload_id: upload_id.to_string(),
                event_id: event_id.to_string(),
                station_id: station_id.to_string(),
                reason: "Hash mismatch em um ou mais arquivos".to_string(),
            });

            return Err(anyhow::anyhow!("Hash mismatch: upload movido para quarentena"));
        }

        // Promover para repositório: organizado por station/date/event_id
        let date = chrono::Utc::now().format("%Y-%m-%d").to_string();
        let repo_path = self.repository_dir.join(station_id).join(&date).join(event_id);
        std::fs::create_dir_all(&repo_path)?;

        // Mover de staging para repositório
        for entry in std::fs::read_dir(&staging_path)? {
            let entry = entry?;
            let dest = repo_path.join(entry.file_name());
            std::fs::rename(entry.path(), &dest)
                .or_else(|_| {
                    std::fs::copy(entry.path(), &dest).map(|_| ())?;
                    std::fs::remove_file(entry.path())
                })?;
        }
        // Remover staging vazio
        let _ = std::fs::remove_dir(&staging_path);

        let total_bytes: u64 = stored_files.iter().map(|f| f.size_bytes).sum();
        let record = UploadRecord {
            upload_id: upload_id.to_string(),
            event_id: event_id.to_string(),
            station_id: station_id.to_string(),
            received_at_utc: chrono::Utc::now(),
            files: stored_files,
            total_bytes,
            repository_path: repo_path.to_string_lossy().to_string(),
        };

        // Salvar registro de upload
        let record_path = repo_path.join("upload_record.json");
        std::fs::write(&record_path, serde_json::to_string_pretty(&record)?)?;

        info!(
            upload_id,
            event_id,
            station_id,
            repository_path = %repo_path.display(),
            "Upload armazenado com sucesso"
        );

        self.audit.log(AuditEvent::UploadAccepted {
            upload_id: upload_id.to_string(),
            event_id: event_id.to_string(),
            station_id: station_id.to_string(),
            stored_path: repo_path.to_string_lossy().to_string(),
        });

        Ok(record)
    }
}

fn copy_dir(src: &Path, dst: &Path) -> Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let dest = dst.join(entry.file_name());
        std::fs::copy(entry.path(), dest)?;
    }
    Ok(())
}
