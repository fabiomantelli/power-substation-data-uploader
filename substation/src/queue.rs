use crate::manifest::EventManifest;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tracing::{debug, warn};

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
pub enum QueueItemStatus {
    Pending,
    Sending,
    Sent,
    Failed,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct QueueItem {
    pub event_id: String,
    pub manifest: EventManifest,
    pub files: Vec<PathBuf>,
    pub status: QueueItemStatus,
    pub attempts: u32,
    pub last_error: Option<String>,
    pub queued_at: chrono::DateTime<chrono::Utc>,
    pub sent_at: Option<chrono::DateTime<chrono::Utc>>,
}

pub struct LocalQueue {
    queue_dir: PathBuf,
}

impl LocalQueue {
    pub fn new(queue_dir: PathBuf) -> Self {
        Self { queue_dir }
    }

    fn item_path(&self, event_id: &str) -> PathBuf {
        self.queue_dir.join(format!("{}.json", event_id))
    }

    pub fn enqueue(&self, item: &QueueItem) -> Result<()> {
        let path = self.item_path(&item.event_id);
        let json = serde_json::to_string_pretty(item).context("Serializando item")?;
        std::fs::write(&path, json)
            .with_context(|| format!("Gravando item na fila: {}", path.display()))?;
        debug!("Enfileirado: {}", item.event_id);
        Ok(())
    }

    pub fn update(&self, item: &QueueItem) -> Result<()> {
        self.enqueue(item)
    }

    pub fn remove(&self, event_id: &str) -> Result<()> {
        let path = self.item_path(event_id);
        if path.exists() {
            std::fs::remove_file(&path)
                .with_context(|| format!("Removendo item da fila: {}", path.display()))?;
        }
        Ok(())
    }

    pub fn load_pending(&self) -> Result<Vec<QueueItem>> {
        let mut items = Vec::new();
        for entry in std::fs::read_dir(&self.queue_dir).context("Lendo dir da fila")? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            match self.load_item(&path) {
                Ok(item) => {
                    if matches!(item.status, QueueItemStatus::Pending | QueueItemStatus::Sending) {
                        items.push(item);
                    }
                }
                Err(e) => {
                    warn!("Erro carregando item da fila {}: {}", path.display(), e);
                }
            }
        }
        // Ordenar por data de enfileiramento
        items.sort_by_key(|i| i.queued_at);
        Ok(items)
    }

    fn load_item(&self, path: &Path) -> Result<QueueItem> {
        let json = std::fs::read_to_string(path)
            .with_context(|| format!("Lendo item: {}", path.display()))?;
        serde_json::from_str(&json)
            .with_context(|| format!("Parseando item: {}", path.display()))
    }

    pub fn count_pending(&self) -> usize {
        self.load_pending().map(|v| v.len()).unwrap_or(0)
    }
}
