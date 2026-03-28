<#
.SYNOPSIS
    Emite um certificado cliente para uma nova subestacao.
    Requer OpenSSL no PATH.
.PARAMETER StationId
    ID da subestacao (ex: SE_XANXERE)
.PARAMETER OutputDir
    Diretorio onde os arquivos serao salvos
.PARAMETER CaKeyPath
    Chave privada da CA intermediaria
.PARAMETER CaCertPath
    Certificado da CA intermediaria
.PARAMETER ValidityDays
    Validade em dias (default: 365)
#>
param(
    [Parameter(Mandatory=$true)] [string]$StationId,
    [Parameter(Mandatory=$true)] [string]$OutputDir,
    [Parameter(Mandatory=$true)] [string]$CaKeyPath,
    [Parameter(Mandatory=$true)] [string]$CaCertPath,
    [int]$ValidityDays = 365
)

$ErrorActionPreference = "Stop"

if (-not (Get-Command openssl -ErrorAction SilentlyContinue)) {
    Write-Error "OpenSSL nao encontrado no PATH"
    exit 1
}

New-Item -ItemType Directory -Path $OutputDir -Force | Out-Null

$KeyPath  = Join-Path $OutputDir "$StationId-key.pem"
$CsrPath  = Join-Path $OutputDir "$StationId.csr"
$CertPath = Join-Path $OutputDir "$StationId-cert.pem"
$ExtFile  = Join-Path $OutputDir "$StationId.ext"

Write-Host "Gerando chave privada..."
openssl genrsa -out $KeyPath 4096

Write-Host "Gerando CSR..."
openssl req -new -key $KeyPath -out $CsrPath `
    -subj "/CN=$StationId/O=MedFasee/OU=Subestacoes"

# Arquivo de extensoes para EKU de cliente
@"
[v3_client]
basicConstraints = CA:FALSE
keyUsage = critical, digitalSignature, keyEncipherment
extendedKeyUsage = clientAuth
subjectAltName = DNS:$StationId
"@ | Out-File -FilePath $ExtFile -Encoding ASCII

Write-Host "Assinando certificado..."
openssl x509 -req -in $CsrPath `
    -CA $CaCertPath -CAkey $CaKeyPath -CAcreateserial `
    -out $CertPath `
    -days $ValidityDays `
    -extensions v3_client `
    -extfile $ExtFile

# Remover arquivos intermediarios
Remove-Item $CsrPath, $ExtFile -Force

# Verificar
$expiry = openssl x509 -in $CertPath -noout -enddate
Write-Host ""
Write-Host "=== Certificado emitido com sucesso ===" -ForegroundColor Green
Write-Host "  Estacao:   $StationId"
Write-Host "  Cert:      $CertPath"
Write-Host "  Chave:     $KeyPath"
Write-Host "  Validade:  $expiry"
Write-Host ""
Write-Host "PROXIMO PASSO: distribuir $CertPath e $KeyPath para a SE."
Write-Host "REGISTRAR no inventario de certificados."
