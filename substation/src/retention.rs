use crate::config::AgentConfig;
use anyhow::Result;
use std::path::Path;
use std::time::{Duration, SystemTime};
use tracing::{info, warn};

pub struct RetentionManager {
    config: AgentConfig,
}

impl RetentionManager {
    pub fn new(config: AgentConfig) -> Self {
        Self { config }
    }

    pub async fn run_loop(&self) {
        let interval = Duration::from_secs(
            self.config.retention.cleanup_interval_minutes * 60,
        );
        loop {
            if let Err(e) = self.run_cleanup() {
                warn!("Erro no ciclo de retenção: {:#}", e);
            }
            tokio::time::sleep(interval).await;
        }
    }

    pub fn run_cleanup(&self) -> Result<()> {
        let disk_pct = get_disk_usage_pct(&self.config.disk.drive);
        let free_mb = get_disk_free_mb(&self.config.disk.drive);

        if disk_pct >= self.config.disk.warn_threshold_pct {
            warn!(
                disk_pct,
                free_mb,
                drive = %self.config.disk.drive,
                "ALERTA: uso de disco acima do limite de aviso"
            );
        }

        // Determinar retenção efetiva baseada em uso de disco
        let effective_retention_days = if disk_pct >= self.config.disk.force_cleanup_threshold_pct
            || free_mb < self.config.disk.min_free_mb
        {
            warn!(
                disk_pct,
                "Disco crítico — limpeza agressiva ativada"
            );
            // Reduzir para mínimo: 7 dias
            7u32
        } else if disk_pct >= self.config.disk.reduce_retention_threshold_pct {
            warn!(
                disk_pct,
                "Disco alto — reduzindo retenção para mínimo configurado"
            );
            self.config.retention.sent_retention_days
        } else {
            // Usar máximo configurado se houver espaço
            self.config.retention.sent_retention_max_days
        };

        // Limpar sent/
        let cleaned = clean_old_dirs(
            &self.config.sent_dir,
            effective_retention_days,
        )?;
        if cleaned > 0 {
            info!(cleaned, "Eventos removidos de sent/");
        }

        // Limpar error/
        let cleaned_errors = clean_old_dirs(
            &self.config.error_dir,
            self.config.retention.error_retention_days,
        )?;
        if cleaned_errors > 0 {
            info!(cleaned_errors, "Eventos removidos de error/");
        }

        // Limpar logs antigos
        let cleaned_logs = clean_old_files(
            &self.config.log_dir,
            self.config.retention.log_retention_days,
        )?;
        if cleaned_logs > 0 {
            info!(cleaned_logs, "Arquivos de log removidos");
        }

        Ok(())
    }
}

fn clean_old_dirs(dir: &Path, retention_days: u32) -> Result<usize> {
    if !dir.exists() {
        return Ok(0);
    }
    let cutoff = SystemTime::now()
        - Duration::from_secs(retention_days as u64 * 86400);

    let mut count = 0;
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let meta = entry.metadata()?;
        if !meta.is_dir() {
            continue;
        }
        let modified = meta.modified()?;
        if modified < cutoff {
            match std::fs::remove_dir_all(entry.path()) {
                Ok(_) => count += 1,
                Err(e) => warn!("Erro removendo {}: {}", entry.path().display(), e),
            }
        }
    }
    Ok(count)
}

fn clean_old_files(dir: &Path, retention_days: u32) -> Result<usize> {
    if !dir.exists() {
        return Ok(0);
    }
    let cutoff = SystemTime::now()
        - Duration::from_secs(retention_days as u64 * 86400);

    let mut count = 0;
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let meta = entry.metadata()?;
        if !meta.is_file() {
            continue;
        }
        let modified = meta.modified()?;
        if modified < cutoff {
            match std::fs::remove_file(entry.path()) {
                Ok(_) => count += 1,
                Err(e) => warn!("Erro removendo {}: {}", entry.path().display(), e),
            }
        }
    }
    Ok(count)
}

fn get_disk_usage_pct(drive: &str) -> u64 {
    use sysinfo::Disks;
    let disks = Disks::new_with_refreshed_list();
    for disk in &disks {
        let mount = disk.mount_point().to_string_lossy().to_lowercase();
        if mount.starts_with(&drive.to_lowercase()) {
            let total = disk.total_space();
            let avail = disk.available_space();
            if total > 0 {
                let used = total.saturating_sub(avail);
                return (used * 100) / total;
            }
        }
    }
    0
}

fn get_disk_free_mb(drive: &str) -> u64 {
    use sysinfo::Disks;
    let disks = Disks::new_with_refreshed_list();
    for disk in &disks {
        let mount = disk.mount_point().to_string_lossy().to_lowercase();
        if mount.starts_with(&drive.to_lowercase()) {
            return disk.available_space() / (1024 * 1024);
        }
    }
    u64::MAX
}
