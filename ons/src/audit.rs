use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tracing::warn;

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuditEvent {
    UploadReceived {
        upload_id: String,
        event_id: String,
        station_id: String,
        client_cn: String,
        file_count: usize,
        total_bytes: u64,
    },
    UploadAccepted {
        upload_id: String,
        event_id: String,
        station_id: String,
        stored_path: String,
    },
    UploadRejected {
        upload_id: String,
        event_id: String,
        station_id: String,
        reason: String,
    },
    UploadQuarantined {
        upload_id: String,
        event_id: String,
        station_id: String,
        reason: String,
    },
    AuthRejected {
        client_ip: String,
        reason: String,
    },
    DuplicateDetected {
        upload_id: String,
        event_id: String,
        station_id: String,
    },
}

#[derive(Debug, Serialize)]
pub struct AuditRecord {
    pub timestamp_utc: DateTime<Utc>,
    pub event: AuditEvent,
}

pub struct AuditLogger {
    audit_dir: PathBuf,
}

impl AuditLogger {
    pub fn new(audit_dir: PathBuf) -> Self {
        Self { audit_dir }
    }

    pub fn log(&self, event: AuditEvent) {
        let record = AuditRecord {
            timestamp_utc: Utc::now(),
            event,
        };

        let date = record.timestamp_utc.format("%Y-%m-%d");
        let path = self.audit_dir.join(format!("audit-{}.jsonl", date));

        let line = match serde_json::to_string(&record) {
            Ok(s) => s,
            Err(e) => {
                warn!("Erro serializando audit record: {}", e);
                return;
            }
        };

        use std::io::Write;
        match std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
        {
            Ok(mut f) => {
                let _ = writeln!(f, "{}", line);
            }
            Err(e) => {
                warn!("Erro gravando audit log {}: {}", path.display(), e);
            }
        }
    }
}
