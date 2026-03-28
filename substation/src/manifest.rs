use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::Path;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct FileEntry {
    pub name: String,
    pub sha256: String,
    pub size_bytes: u64,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct EventManifest {
    pub event_id: String,
    pub station_id: String,
    pub device_id: String,
    pub created_at_utc: DateTime<Utc>,
    pub files: Vec<FileEntry>,
    pub source_path: String,
    pub schema_version: u32,
}

impl EventManifest {
    pub fn build(
        station_id: &str,
        device_id: &str,
        files: &[&Path],
        source_path: &str,
    ) -> Result<Self> {
        let now = Utc::now();
        let ts = now.format("%Y-%m-%dT%H-%M-%SZ");
        let event_id = format!(
            "{}_{}_{}_{}",
            station_id,
            device_id,
            ts,
            &uuid::Uuid::new_v4().to_string()[..8]
        );

        let mut entries = Vec::new();
        for path in files {
            let data = std::fs::read(path)
                .with_context(|| format!("Lendo arquivo: {}", path.display()))?;
            let hash = hex::encode(Sha256::digest(&data));
            let name = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("unknown")
                .to_string();
            entries.push(FileEntry {
                name,
                sha256: hash,
                size_bytes: data.len() as u64,
            });
        }

        Ok(Self {
            event_id,
            station_id: station_id.to_string(),
            device_id: device_id.to_string(),
            created_at_utc: now,
            files: entries,
            source_path: source_path.to_string(),
            schema_version: 1,
        })
    }

    #[allow(dead_code)]
    pub fn total_size_bytes(&self) -> u64 {
        self.files.iter().map(|f| f.size_bytes).sum()
    }
}

#[allow(dead_code)]
pub fn hash_file(path: &Path) -> Result<String> {
    let data = std::fs::read(path)
        .with_context(|| format!("Lendo para hash: {}", path.display()))?;
    Ok(hex::encode(Sha256::digest(&data)))
}
