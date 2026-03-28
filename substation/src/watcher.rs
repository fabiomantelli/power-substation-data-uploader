use crate::{
    config::AgentConfig,
    manifest::EventManifest,
    queue::{LocalQueue, QueueItem, QueueItemStatus},
};
use anyhow::Result;
use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};
use tokio::sync::mpsc;
use tracing::{error, info, warn};

/// Extensões de arquivo COMTRADE
const COMTRADE_EXTENSIONS: &[&str] = &["cfg", "dat", "hdr", "inf"];
/// Extensão principal que indica um evento completo
const PRIMARY_EXTENSION: &str = "cfg";

pub struct InboxWatcher {
    config: Arc<AgentConfig>,
    queue: Arc<LocalQueue>,
}

impl InboxWatcher {
    pub fn new(config: Arc<AgentConfig>, queue: Arc<LocalQueue>) -> Self {
        Self { config, queue }
    }

    pub async fn run_loop(&self) -> Result<()> {
        let (tx, mut rx) = mpsc::channel::<PathBuf>(256);
        let inbox = self.config.inbox_dir.clone();
        let debounce = Duration::from_millis(self.config.watch_debounce_ms);

        // Watcher em thread separada
        let _watcher = spawn_watcher(inbox.clone(), tx)?;

        // Verificar inbox existente na inicialização
        self.scan_inbox().await;

        // Debounce state: base_name -> set de extensões detectadas
        let mut pending: HashMap<String, HashSet<String>> = HashMap::new();
        let mut last_seen: HashMap<String, std::time::Instant> = HashMap::new();

        loop {
            tokio::select! {
                Some(path) = rx.recv() => {
                    let ext = path
                        .extension()
                        .and_then(|e| e.to_str())
                        .unwrap_or("")
                        .to_lowercase();

                    if !COMTRADE_EXTENSIONS.contains(&ext.as_str()) {
                        continue;
                    }

                    let stem = path
                        .file_stem()
                        .and_then(|s| s.to_str())
                        .unwrap_or("")
                        .to_string();

                    pending.entry(stem.clone()).or_default().insert(ext);
                    last_seen.insert(stem, std::time::Instant::now());
                }
                _ = tokio::time::sleep(debounce) => {
                    let now = std::time::Instant::now();
                    let ready: Vec<String> = last_seen
                        .iter()
                        .filter(|(_, t)| now.duration_since(**t) >= debounce)
                        .map(|(k, _)| k.clone())
                        .collect();

                    for stem in ready {
                        last_seen.remove(&stem);
                        let exts = pending.remove(&stem).unwrap_or_default();

                        // Processar somente se tiver .cfg
                        if !exts.contains(PRIMARY_EXTENSION) {
                            continue;
                        }

                        let cfg_path = self.config.inbox_dir.join(format!("{}.cfg", stem));
                        if cfg_path.exists() {
                            if let Err(e) = self.process_event(&stem).await {
                                error!("Erro processando evento {}: {:#}", stem, e);
                            }
                        }
                    }
                }
            }
        }
    }

    async fn scan_inbox(&self) {
        let inbox = &self.config.inbox_dir;
        match std::fs::read_dir(inbox) {
            Ok(entries) => {
                let mut stems: HashSet<String> = HashSet::new();
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.extension().and_then(|e| e.to_str()) == Some(PRIMARY_EXTENSION) {
                        if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                            stems.insert(stem.to_string());
                        }
                    }
                }
                for stem in stems {
                    if let Err(e) = self.process_event(&stem).await {
                        error!("Scan inicial - erro em {}: {:#}", stem, e);
                    }
                }
            }
            Err(e) => {
                warn!("Não foi possível escanear inbox: {}", e);
            }
        }
    }

    async fn process_event(&self, stem: &str) -> Result<()> {
        let inbox = &self.config.inbox_dir;

        // Coletar arquivos do evento
        let files: Vec<PathBuf> = COMTRADE_EXTENSIONS
            .iter()
            .map(|ext| inbox.join(format!("{}.{}", stem, ext)))
            .filter(|p| p.exists())
            .collect();

        if files.is_empty() {
            return Ok(());
        }

        let file_refs: Vec<&Path> = files.iter().map(|p| p.as_path()).collect();
        let source_path = inbox.to_string_lossy().to_string();

        let manifest = EventManifest::build(
            &self.config.station_id,
            &self.config.device_id,
            &file_refs,
            &source_path,
        )?;

        let event_id = manifest.event_id.clone();

        // Mover para spool/
        let spool_dir = self.config.spool_dir.join(&event_id);
        std::fs::create_dir_all(&spool_dir)?;

        let mut spool_files = Vec::new();
        for src in &files {
            let dest = spool_dir.join(src.file_name().unwrap_or_default());
            std::fs::rename(src, &dest)?;
            spool_files.push(dest);
        }

        // Salvar manifesto no spool
        let manifest_path = spool_dir.join("manifest.json");
        std::fs::write(&manifest_path, serde_json::to_string_pretty(&manifest)?)?;

        let item = QueueItem {
            event_id: event_id.clone(),
            manifest,
            files: spool_files,
            status: QueueItemStatus::Pending,
            attempts: 0,
            last_error: None,
            queued_at: chrono::Utc::now(),
            sent_at: None,
        };

        self.queue.enqueue(&item)?;
        info!("Evento enfileirado: {}", event_id);
        Ok(())
    }
}

fn spawn_watcher(
    path: PathBuf,
    tx: mpsc::Sender<PathBuf>,
) -> Result<RecommendedWatcher> {
    let mut watcher = notify::recommended_watcher(move |res: notify::Result<Event>| {
        if let Ok(event) = res {
            if matches!(
                event.kind,
                EventKind::Create(_) | EventKind::Modify(_)
            ) {
                for path in event.paths {
                    let _ = tx.blocking_send(path);
                }
            }
        }
    })?;

    watcher.watch(&path, RecursiveMode::NonRecursive)?;
    Ok(watcher)
}
