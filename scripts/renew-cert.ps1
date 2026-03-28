<#
.SYNOPSIS
    Renova o certificado de uma subestacao preservando a chave privada existente.
#>
param(
    [Parameter(Mandatory=$true)] [string]$StationId,
    [Parameter(Mandatory=$true)] [string]$CertDir,
    [Parameter(Mandatory=$true)] [string]$CaKeyPath,
    [Parameter(Mandatory=$true)] [string]$CaCertPath,
    [int]$ValidityDays = 365
)

$KeyPath    = Join-Path $CertDir "$StationId-key.pem"
$CertPath   = Join-Path $CertDir "$StationId-cert.pem"
$BackupPath = Join-Path $CertDir "$StationId-cert.pem.bak-$(Get-Date -Format 'yyyyMMdd')"

if (-not (Test-Path $KeyPath)) {
    Write-Error "Chave nao encontrada: $KeyPath -- use new-station-cert.ps1 para emitir novo certificado."
    exit 1
}

# Fazer backup do cert atual
if (Test-Path $CertPath) {
    Copy-Item $CertPath $BackupPath -Force
    Write-Host "Backup do certificado atual: $BackupPath"
}

$CsrPath = Join-Path $CertDir "$StationId-renew.csr"
$ExtFile  = Join-Path $CertDir "$StationId-renew.ext"

# Gerar novo CSR com a chave existente
openssl req -new -key $KeyPath -out $CsrPath `
    -subj "/CN=$StationId/O=MedFasee/OU=Subestacoes"

@"
[v3_client]
basicConstraints = CA:FALSE
keyUsage = critical, digitalSignature, keyEncipherment
extendedKeyUsage = clientAuth
subjectAltName = DNS:$StationId
"@ | Out-File -FilePath $ExtFile -Encoding ASCII

openssl x509 -req -in $CsrPath `
    -CA $CaCertPath -CAkey $CaKeyPath -CAcreateserial `
    -out $CertPath `
    -days $ValidityDays `
    -extensions v3_client `
    -extfile $ExtFile

Remove-Item $CsrPath, $ExtFile -Force

$expiry = openssl x509 -in $CertPath -noout -enddate
Write-Host ""
Write-Host "=== Certificado renovado ===" -ForegroundColor Green
Write-Host "  Estacao:       $StationId"
Write-Host "  Nova validade: $expiry"
Write-Host "REGISTRAR renovacao no inventario."
