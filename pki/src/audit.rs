use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tracing::warn;

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PkiAuditEvent {
    RenewalRequested {
        request_id: String,
        station_id: String,
        client_cn: String,
        client_cert_expiry_utc: String,
        days_remaining: i64,
    },
    RenewalIssued {
        request_id: String,
        station_id: String,
        client_cn: String,
        new_cert_expiry_utc: String,
    },
    RenewalRejected {
        request_id: String,
        station_id: String,
        client_cn: String,
        reason: String,
    },
    AuthRejected {
        client_ip: String,
        reason: String,
    },
}

#[derive(Debug, Serialize)]
pub struct PkiAuditRecord {
    pub timestamp_utc: DateTime<Utc>,
    pub event: PkiAuditEvent,
}

pub struct PkiAuditLogger {
    audit_dir: PathBuf,
}

impl PkiAuditLogger {
    pub fn new(audit_dir: PathBuf) -> Self {
        Self { audit_dir }
    }

    pub fn log(&self, event: PkiAuditEvent) {
        let record = PkiAuditRecord {
            timestamp_utc: Utc::now(),
            event,
        };

        let date = record.timestamp_utc.format("%Y-%m-%d");
        let path = self.audit_dir.join(format!("pki-audit-{}.jsonl", date));

        let line = match serde_json::to_string(&record) {
            Ok(s) => s,
            Err(e) => {
                warn!("Erro serializando PKI audit record: {}", e);
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
                warn!("Erro gravando PKI audit log {}: {}", path.display(), e);
            }
        }
    }
}
