use crate::{
    config::AgentConfig,
    queue::{LocalQueue, QueueItem, QueueItemStatus},
};
use anyhow::{Context, Result};
use reqwest::{
    multipart::{Form, Part},
    Client,
};
use std::time::Duration;
use tokio::time::sleep;
use tracing::{error, info, warn};

pub struct Sender {
    client: Client,
    config: AgentConfig,
    queue: LocalQueue,
}

impl Sender {
    pub fn new(config: AgentConfig, queue: LocalQueue) -> Result<Self> {
        let client = build_client(&config)?;
        Ok(Self { client, config, queue })
    }

    pub async fn run_loop(&self) {
        loop {
            match self.process_queue().await {
                Ok(sent) => {
                    if sent > 0 {
                        info!("Ciclo: {} arquivo(s) enviado(s)", sent);
                    }
                }
                Err(e) => {
                    error!("Erro no ciclo de envio: {:#}", e);
                }
            }
            sleep(Duration::from_secs(30)).await;
        }
    }

    async fn process_queue(&self) -> Result<usize> {
        let items = self.queue.load_pending()?;
        let mut sent_count = 0;

        for mut item in items {
            if item.attempts >= self.config.max_retries + 1 {
                item.status = QueueItemStatus::Failed;
                item.last_error = Some(format!(
                    "Máximo de tentativas ({}) atingido",
                    self.config.max_retries
                ));
                self.queue.update(&item)?;
                error!("Evento falhou permanentemente: {}", item.event_id);
                continue;
            }

            // Backoff exponencial
            if item.attempts > 0 {
                let backoff = std::cmp::min(
                    self.config.retry_initial_backoff_seconds * (2u64.pow(item.attempts - 1)),
                    self.config.retry_max_backoff_seconds,
                );
                sleep(Duration::from_secs(backoff)).await;
            }

            item.status = QueueItemStatus::Sending;
            item.attempts += 1;
            self.queue.update(&item)?;

            match self.send_event(&item).await {
                Ok(upload_id) => {
                    item.status = QueueItemStatus::Sent;
                    item.sent_at = Some(chrono::Utc::now());
                    self.queue.update(&item)?;
                    info!(
                        event_id = %item.event_id,
                        upload_id = %upload_id,
                        "Enviado com sucesso"
                    );
                    sent_count += 1;
                    // Move arquivos para sent/
                    if let Err(e) = self.move_to_sent(&item) {
                        warn!("Erro ao mover para sent/: {:#}", e);
                    }
                    self.queue.remove(&item.event_id)?;
                }
                Err(e) => {
                    item.status = QueueItemStatus::Pending;
                    item.last_error = Some(e.to_string());
                    self.queue.update(&item)?;
                    warn!(
                        event_id = %item.event_id,
                        attempt = item.attempts,
                        error = %e,
                        "Falha no envio, será retentatado"
                    );
                }
            }
        }
        Ok(sent_count)
    }

    async fn send_event(&self, item: &QueueItem) -> Result<String> {
        let manifest_json =
            serde_json::to_string(&item.manifest).context("Serializando manifesto")?;

        let mut form = Form::new()
            .part("station_id", Part::text(item.manifest.station_id.clone()))
            .part("device_id", Part::text(item.manifest.device_id.clone()))
            .part("event_id", Part::text(item.event_id.clone()))
            .part(
                "timestamp_utc",
                Part::text(item.manifest.created_at_utc.to_rfc3339()),
            )
            .part("manifest", Part::text(manifest_json).mime_str("application/json")?);

        for file_path in &item.files {
            let data = tokio::fs::read(file_path)
                .await
                .with_context(|| format!("Lendo arquivo para upload: {}", file_path.display()))?;
            let name = file_path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("file")
                .to_string();
            let part = Part::bytes(data)
                .file_name(name.clone())
                .mime_str("application/octet-stream")?;
            form = form.part(name, part);
        }

        let url = format!("{}/api/v1/upload", self.config.server_url);
        let resp = self
            .client
            .post(&url)
            .multipart(form)
            .timeout(Duration::from_secs(self.config.upload_timeout_seconds))
            .send()
            .await
            .context("Enviando requisição")?;

        let status = resp.status();
        let body: serde_json::Value = resp.json().await.context("Lendo resposta")?;

        if !status.is_success() {
            return Err(anyhow::anyhow!(
                "Servidor retornou {}: {}",
                status,
                body
            ));
        }

        // Verifica ack
        let upload_id = body["upload_id"]
            .as_str()
            .unwrap_or("unknown")
            .to_string();

        if body["hash_verified"].as_bool() != Some(true) {
            return Err(anyhow::anyhow!("Servidor não confirmou integridade do hash"));
        }

        Ok(upload_id)
    }

    fn move_to_sent(&self, item: &QueueItem) -> Result<()> {
        let sent_dir = self.config.sent_dir.join(&item.event_id);
        std::fs::create_dir_all(&sent_dir)?;

        // Salvar manifesto em sent/
        let manifest_path = sent_dir.join("manifest.json");
        let json = serde_json::to_string_pretty(&item.manifest)?;
        std::fs::write(&manifest_path, json)?;

        // Mover arquivos originais
        for file_path in &item.files {
            if file_path.exists() {
                let dest = sent_dir.join(file_path.file_name().unwrap_or_default());
                std::fs::rename(file_path, &dest)
                    .or_else(|_| {
                        // Se não for possível mover (cross-device), copiar e deletar
                        std::fs::copy(file_path, &dest).map(|_| ())?;
                        std::fs::remove_file(file_path)
                    })
                    .with_context(|| format!("Movendo {} para sent/", file_path.display()))?;
            }
        }
        Ok(())
    }
}

fn build_client(config: &AgentConfig) -> Result<Client> {
    // Carregar certificado cliente
    let cert_pem = std::fs::read(&config.client_cert_pem)
        .with_context(|| format!("Lendo cert cliente: {}", config.client_cert_pem.display()))?;
    let key_pem = std::fs::read(&config.client_key_pem)
        .with_context(|| format!("Lendo chave cliente: {}", config.client_key_pem.display()))?;

    // Carregar CA bundle
    let ca_pem = std::fs::read(&config.ca_bundle_pem)
        .with_context(|| format!("Lendo CA bundle: {}", config.ca_bundle_pem.display()))?;

    let identity =
        reqwest::Identity::from_pem(&[cert_pem.as_slice(), key_pem.as_slice()].concat())
            .context("Criando identidade mTLS")?;

    let ca_cert = reqwest::Certificate::from_pem(&ca_pem).context("Carregando CA cert")?;

    let client = Client::builder()
        .identity(identity)
        .add_root_certificate(ca_cert)
        .use_rustls_tls()
        .min_tls_version(reqwest::tls::Version::TLS_1_2)
        .timeout(Duration::from_secs(config.upload_timeout_seconds + 10))
        .build()
        .context("Construindo cliente HTTP")?;

    Ok(client)
}
