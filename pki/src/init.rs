//! Bootstrap PKI: gera a hierarquia completa de certificados usando rcgen.
//! Não requer OpenSSL instalado.

use anyhow::{Context, Result};
use rcgen::{
    BasicConstraints, CertificateParams, DnType, IsCa, KeyPair, KeyUsagePurpose,
    SanType,
};
use std::path::{Path, PathBuf};

/// Argumentos do subcomando `init-pki`
#[derive(Debug, clap::Args)]
pub struct InitPkiArgs {
    /// Diretório onde os arquivos PEM serão gravados
    #[arg(long)]
    pub output_dir: PathBuf,

    /// DNS do servidor ONS (osc-server) para o Subject Alternative Name
    /// Pode ser repetido para múltiplos nomes/IPs (ex: --server-dns upload.ons.intra --server-dns 10.0.0.1)
    #[arg(long, required = true)]
    pub server_dns: Vec<String>,

    /// DNS do servidor PKI (osc-pki-server) para o Subject Alternative Name
    #[arg(long, required = true)]
    pub pki_dns: Vec<String>,

    /// Validade da Root CA em dias (default: 3650 = ~10 anos)
    #[arg(long, default_value = "3650")]
    pub root_days: u32,

    /// Validade da CA intermediária em dias (default: 1095 = ~3 anos)
    #[arg(long, default_value = "1095")]
    pub intermediate_days: u32,

    /// Validade dos certificados de servidor em dias (default: 365)
    #[arg(long, default_value = "365")]
    pub cert_days: u32,
}

pub fn run_init_pki(args: InitPkiArgs) -> Result<()> {
    std::fs::create_dir_all(&args.output_dir)
        .with_context(|| format!("Criando diretório: {}", args.output_dir.display()))?;

    let out = &args.output_dir;

    println!("=== Inicializando PKI ===");
    println!("  Diretório de saída: {}", out.display());
    println!();

    // 1. Root CA
    println!("[ 1/5 ] Gerando Root CA ({} dias)...", args.root_days);
    let (root_cert_pem, root_key_pem) =
        generate_root_ca(args.root_days).context("Gerando Root CA")?;
    write_pem(out, "root-cert.pem", &root_cert_pem)?;
    write_pem(out, "root-key.pem", &root_key_pem)?;

    // 2. Intermediate CA
    println!(
        "[ 2/5 ] Gerando CA Intermediária ({} dias)...",
        args.intermediate_days
    );
    let (int_cert_pem, int_key_pem) =
        generate_intermediate_ca(&root_cert_pem, &root_key_pem, args.intermediate_days)
            .context("Gerando CA Intermediária")?;
    write_pem(out, "intermediate-cert.pem", &int_cert_pem)?;
    write_pem(out, "intermediate-key.pem", &int_key_pem)?;

    // 3. CA chain (intermediate + root)
    println!("[ 3/5 ] Construindo ca-chain.pem...");
    let ca_chain = format!("{}{}", int_cert_pem, root_cert_pem);
    write_pem(out, "ca-chain.pem", &ca_chain)?;

    // 4. Certificado do osc-server
    println!(
        "[ 4/5 ] Gerando certificado do servidor ONS ({} dias, SAN: {})...",
        args.cert_days,
        args.server_dns.join(", ")
    );
    let (srv_cert_pem, srv_key_pem) = generate_server_cert(
        &int_cert_pem,
        &int_key_pem,
        "OSC Server ONS",
        &args.server_dns,
        args.cert_days,
    )
    .context("Gerando certificado osc-server")?;
    write_pem(out, "server-cert.pem", &srv_cert_pem)?;
    write_pem(out, "server-key.pem", &srv_key_pem)?;

    // 5. Certificado do osc-pki-server
    println!(
        "[ 5/5 ] Gerando certificado do servidor PKI ({} dias, SAN: {})...",
        args.cert_days,
        args.pki_dns.join(", ")
    );
    let (pki_cert_pem, pki_key_pem) = generate_server_cert(
        &int_cert_pem,
        &int_key_pem,
        "OSC PKI Server",
        &args.pki_dns,
        args.cert_days,
    )
    .context("Gerando certificado osc-pki-server")?;
    write_pem(out, "pki-server-cert.pem", &pki_cert_pem)?;
    write_pem(out, "pki-server-key.pem", &pki_key_pem)?;

    println!();
    println!("=== PKI gerado com sucesso ===");
    println!();
    println!("Arquivos em {}:", out.display());
    for name in [
        "root-cert.pem",
        "root-key.pem",
        "intermediate-cert.pem",
        "intermediate-key.pem",
        "ca-chain.pem",
        "server-cert.pem",
        "server-key.pem",
        "pki-server-cert.pem",
        "pki-server-key.pem",
    ] {
        println!("  {}", name);
    }
    println!();
    println!("AVISO DE SEGURANÇA:");
    println!("  root-key.pem contém a chave privada da Root CA.");
    println!("  Mova-a AGORA para um pendrive cifrado (VeraCrypt) ou HSM");
    println!("  e apague-a desta máquina.");
    println!();
    println!("PRÓXIMOS PASSOS:");
    println!("  1. Distribuir certificados (use scripts\\setup-pki.ps1)");
    println!("  2. Emitir certificados de subestações: new-station-cert.ps1");
    println!("  3. Instalar serviços: install-server.ps1, install-pki-server.ps1");

    Ok(())
}

/// Gera a Root CA auto-assinada. Retorna (cert_pem, key_pem).
fn generate_root_ca(validity_days: u32) -> Result<(String, String)> {
    let key = KeyPair::generate().context("Gerando chave Root CA")?;

    let mut params = CertificateParams::default();
    params
        .distinguished_name
        .push(DnType::CommonName, "OSC Root CA");
    params
        .distinguished_name
        .push(DnType::OrganizationName, "MedFasee");
    params
        .distinguished_name
        .push(DnType::OrganizationalUnitName, "PKI");
    params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    params.key_usages = vec![
        KeyUsagePurpose::KeyCertSign,
        KeyUsagePurpose::CrlSign,
        KeyUsagePurpose::DigitalSignature,
    ];
    params.not_before = now();
    params.not_after = now() + days(validity_days);

    let cert = params.self_signed(&key).context("Auto-assinando Root CA")?;
    Ok((cert.pem(), key.serialize_pem()))
}

/// Gera a CA intermediária assinada pela Root CA. Retorna (cert_pem, key_pem).
fn generate_intermediate_ca(
    root_cert_pem: &str,
    root_key_pem: &str,
    validity_days: u32,
) -> Result<(String, String)> {
    let int_key = KeyPair::generate().context("Gerando chave CA Intermediária")?;

    let mut params = CertificateParams::default();
    params
        .distinguished_name
        .push(DnType::CommonName, "OSC Intermediate CA");
    params
        .distinguished_name
        .push(DnType::OrganizationName, "MedFasee");
    params
        .distinguished_name
        .push(DnType::OrganizationalUnitName, "PKI");
    params.is_ca = IsCa::Ca(BasicConstraints::Constrained(0));
    params.key_usages = vec![
        KeyUsagePurpose::KeyCertSign,
        KeyUsagePurpose::CrlSign,
        KeyUsagePurpose::DigitalSignature,
    ];
    params.not_before = now();
    params.not_after = now() + days(validity_days);

    let root_key = KeyPair::from_pem(root_key_pem).context("Carregando chave Root CA")?;
    let root_params =
        CertificateParams::from_ca_cert_pem(root_cert_pem).context("Carregando cert Root CA")?;
    let root_cert = root_params
        .self_signed(&root_key)
        .context("Recriando cert Root CA para assinatura")?;

    let int_cert = params
        .signed_by(&int_key, &root_cert, &root_key)
        .context("Assinando CA Intermediária")?;

    Ok((int_cert.pem(), int_key.serialize_pem()))
}

/// Gera um certificado TLS de servidor (serverAuth) assinado pela CA intermediária.
/// `dns_names` pode conter hostnames ou IPs (IPs são detectados automaticamente).
/// Retorna (cert_pem, key_pem).
fn generate_server_cert(
    int_cert_pem: &str,
    int_key_pem: &str,
    cn: &str,
    dns_names: &[String],
    validity_days: u32,
) -> Result<(String, String)> {
    let srv_key = KeyPair::generate().context("Gerando chave servidor")?;

    let mut params = CertificateParams::default();
    params.distinguished_name.push(DnType::CommonName, cn);
    params
        .distinguished_name
        .push(DnType::OrganizationName, "MedFasee");
    params.is_ca = IsCa::NoCa;
    params.key_usages = vec![
        KeyUsagePurpose::DigitalSignature,
        KeyUsagePurpose::KeyEncipherment,
    ];
    params.extended_key_usages = vec![rcgen::ExtendedKeyUsagePurpose::ServerAuth];
    params.not_before = now();
    params.not_after = now() + days(validity_days);

    for name in dns_names {
        // Tentar parsear como IP; senão, tratar como DNS
        if let Ok(ip) = name.parse::<std::net::IpAddr>() {
            params.subject_alt_names.push(SanType::IpAddress(ip));
        } else {
            params
                .subject_alt_names
                .push(SanType::DnsName(name.clone().try_into().map_err(|e| {
                    anyhow::anyhow!("DNS name inválido '{}': {:?}", name, e)
                })?));
        }
    }

    let int_key = KeyPair::from_pem(int_key_pem).context("Carregando chave CA Intermediária")?;
    let int_ca_params = CertificateParams::from_ca_cert_pem(int_cert_pem)
        .context("Carregando cert CA Intermediária")?;
    let int_cert = int_ca_params
        .self_signed(&int_key)
        .context("Recriando cert CA Intermediária para assinatura")?;

    let srv_cert = params
        .signed_by(&srv_key, &int_cert, &int_key)
        .context("Assinando certificado servidor")?;

    Ok((srv_cert.pem(), srv_key.serialize_pem()))
}

fn write_pem(dir: &Path, filename: &str, content: &str) -> Result<()> {
    let path = dir.join(filename);
    std::fs::write(&path, content)
        .with_context(|| format!("Gravando {}", path.display()))?;
    Ok(())
}

fn now() -> ::time::OffsetDateTime {
    ::time::OffsetDateTime::now_utc()
}

fn days(n: u32) -> ::time::Duration {
    ::time::Duration::days(n as i64)
}
