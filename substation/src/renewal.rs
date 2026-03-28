use crate::config::AgentConfig;
use anyhow::{Context, Result};
use rcgen::{
    CertificateParams, DnType, ExtendedKeyUsagePurpose, IsCa, KeyPair, KeyUsagePurpose, SanType,
};
use reqwest::Client;
use std::{path::Path, sync::Arc, time::Duration};
use tracing::{info, warn};
use x509_parser::prelude::*;

const STAMP_FILENAME: &str = "renewal_pending.stamp";
const STAMP_THROTTLE_HOURS: i64 = 24;

pub struct RenewalTask {
    config: Arc<AgentConfig>,
}

impl RenewalTask {
    pub fn new(config: Arc<AgentConfig>) -> Self {
        Self { config }
    }

    pub async fn run_loop(&self) {
        // Verificar imediatamente na inicialização
        if let Err(e) = self.check_and_renew().await {
            warn!("Verificação inicial de renovação: {:#}", e);
        }

        loop {
            tokio::time::sleep(Duration::from_secs(
                self.config.renewal.check_interval_hours * 3600,
            ))
            .await;

            if let Err(e) = self.check_and_renew().await {
                warn!("Ciclo de renovação falhou: {:#}", e);
            }
        }
    }

    async fn check_and_renew(&self) -> Result<()> {
        let cert_pem = std::fs::read_to_string(&self.config.client_cert_pem).with_context(
            || format!("Lendo cert atual: {}", self.config.client_cert_pem.display()),
        )?;

        let days_remaining = cert_days_remaining(&cert_pem)?;

        if days_remaining > self.config.renewal.renewal_days_before_expiry {
            info!(
                days_remaining,
                threshold = self.config.renewal.renewal_days_before_expiry,
                "Certificado dentro do prazo"
            );
            return Ok(());
        }

        warn!(
            days_remaining,
            "Certificado próximo da expiração — iniciando renovação automática"
        );

        // Throttle: evitar múltiplas requisições no mesmo dia
        let stamp_path = self.config.state_dir.join(STAMP_FILENAME);
        if is_stamp_recent(&stamp_path) {
            info!(
                "Renovação já solicitada nas últimas {}h — aguardando",
                STAMP_THROTTLE_HOURS
            );
            return Ok(());
        }

        self.perform_renewal(&cert_pem).await
    }

    async fn perform_renewal(&self, current_cert_pem: &str) -> Result<()> {
        info!("Gerando novo par de chaves ECDSA e CSR...");

        // Gerar novo par de chaves (ECDSA P-384 — equivalente a RSA-4096, suporte nativo rustls)
        let key_pair = KeyPair::generate().context("Gerando novo par de chaves")?;

        // Parâmetros do CSR
        let mut params = CertificateParams::default();
        params
            .distinguished_name
            .push(DnType::CommonName, self.config.station_id.as_str());
        params
            .distinguished_name
            .push(DnType::OrganizationName, "MedFasee");
        params
            .distinguished_name
            .push(DnType::OrganizationalUnitName, "Subestacoes");

        // SAN: DNS name igual ao station_id
        #[allow(clippy::infallible_destructuring_match)]
        {
            let san_str = self.config.station_id.as_str();
            if let Ok(san) = rcgen::Ia5String::try_from(san_str) {
                params.subject_alt_names = vec![SanType::DnsName(san)];
            }
        }

        params.key_usages = vec![
            KeyUsagePurpose::DigitalSignature,
            KeyUsagePurpose::KeyEncipherment,
        ];
        params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ClientAuth];
        params.is_ca = IsCa::NoCa;

        // Gerar CSR
        let csr = params
            .serialize_request(&key_pair)
            .context("Gerando CSR")?;
        let csr_pem = csr.pem().context("Serializando CSR para PEM")?;
        let new_key_pem = key_pair.serialize_pem();

        info!("CSR gerado — enviando para servidor PKI...");

        // Cliente HTTP com cert ATUAL para mTLS (o novo cert ainda não existe)
        let client = build_mtls_client(&self.config)?;

        let form = reqwest::multipart::Form::new()
            .part(
                "station_id",
                reqwest::multipart::Part::text(self.config.station_id.clone()),
            )
            .part(
                "current_cert_pem",
                reqwest::multipart::Part::text(current_cert_pem.to_string()),
            )
            .part(
                "csr_pem",
                reqwest::multipart::Part::text(csr_pem)
                    .mime_str("application/x-pem-file")
                    .context("mime str")?,
            );

        let url = format!("{}/api/v1/pki/renew", self.config.renewal.pki_server_url);
        let resp = client
            .post(&url)
            .multipart(form)
            .timeout(Duration::from_secs(60))
            .send()
            .await
            .context("Enviando CSR para servidor PKI")?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!(
                "Servidor PKI retornou {}: {}",
                status,
                body
            ));
        }

        let new_cert_pem = resp.text().await.context("Lendo resposta do servidor PKI")?;

        // Escrita atômica: .tmp → rename (idempotente se o processo reiniciar no meio)
        let key_tmp = self
            .config
            .renewal
            .new_key_path
            .with_extension("pem.tmp");
        let cert_tmp = self
            .config
            .renewal
            .new_cert_path
            .with_extension("pem.tmp");

        std::fs::write(&key_tmp, &new_key_pem)
            .with_context(|| format!("Escrevendo nova chave: {}", key_tmp.display()))?;
        std::fs::rename(&key_tmp, &self.config.renewal.new_key_path).with_context(|| {
            format!(
                "Movendo nova chave: {}",
                self.config.renewal.new_key_path.display()
            )
        })?;

        std::fs::write(&cert_tmp, &new_cert_pem)
            .with_context(|| format!("Escrevendo novo cert: {}", cert_tmp.display()))?;
        std::fs::rename(&cert_tmp, &self.config.renewal.new_cert_path).with_context(|| {
            format!(
                "Movendo novo cert: {}",
                self.config.renewal.new_cert_path.display()
            )
        })?;

        // Gravar stamp para throttle de 24h
        let stamp_path = self.config.state_dir.join(STAMP_FILENAME);
        std::fs::write(&stamp_path, chrono::Utc::now().to_rfc3339())
            .with_context(|| format!("Gravando stamp: {}", stamp_path.display()))?;

        warn!(
            new_key = %self.config.renewal.new_key_path.display(),
            new_cert = %self.config.renewal.new_cert_path.display(),
            "RENOVAÇÃO CONCLUÍDA — reiniciar OscAgent para ativar o novo certificado"
        );

        Ok(())
    }
}

/// Retorna quantos dias restam até a expiração do cert PEM.
fn cert_days_remaining(cert_pem: &str) -> Result<i64> {
    let parsed = ::pem::parse(cert_pem).context("Parsing PEM do certificado")?;
    let (_, cert) =
        parse_x509_certificate(parsed.contents()).context("Parsing X.509 do certificado")?;

    let not_after_ts = cert.validity().not_after.timestamp();
    let not_after_dt = chrono::DateTime::from_timestamp(not_after_ts, 0)
        .unwrap_or_else(chrono::Utc::now);

    Ok((not_after_dt - chrono::Utc::now()).num_days())
}

/// Retorna true se o stamp file existe e tem menos de STAMP_THROTTLE_HOURS horas.
fn is_stamp_recent(stamp_path: &Path) -> bool {
    if let Ok(content) = std::fs::read_to_string(stamp_path) {
        if let Ok(stamp_time) = chrono::DateTime::parse_from_rfc3339(content.trim()) {
            let age = chrono::Utc::now() - stamp_time.with_timezone(&chrono::Utc);
            return age.num_hours() < STAMP_THROTTLE_HOURS;
        }
    }
    false
}

/// Constrói cliente reqwest com mTLS usando o certificado ATUAL da SE.
fn build_mtls_client(config: &AgentConfig) -> Result<Client> {
    let cert_pem = std::fs::read(&config.client_cert_pem)
        .with_context(|| format!("Lendo cert: {}", config.client_cert_pem.display()))?;
    let key_pem = std::fs::read(&config.client_key_pem)
        .with_context(|| format!("Lendo key: {}", config.client_key_pem.display()))?;
    let ca_pem = std::fs::read(&config.ca_bundle_pem)
        .with_context(|| format!("Lendo CA: {}", config.ca_bundle_pem.display()))?;

    let identity =
        reqwest::Identity::from_pem(&[cert_pem.as_slice(), key_pem.as_slice()].concat())
            .context("Criando identidade mTLS para renovação")?;

    let ca_cert = reqwest::Certificate::from_pem(&ca_pem).context("Carregando CA cert")?;

    Client::builder()
        .identity(identity)
        .add_root_certificate(ca_cert)
        .use_rustls_tls()
        .min_tls_version(reqwest::tls::Version::TLS_1_2)
        .build()
        .context("Construindo cliente HTTP para renovação")
}
