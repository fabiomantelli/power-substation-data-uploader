#Requires -RunAsAdministrator
<#
.SYNOPSIS
    Gera toda a hierarquia PKI e distribui os certificados para os servidores.
    Não requer OpenSSL — usa o subcomando init-pki do osc-pki-server.

.PARAMETER BinPath
    Caminho para o executavel osc-pki-server.exe (compilado com cargo build --release)

.PARAMETER OutputDir
    Diretorio temporario onde os PEM serao gerados (default: D:\OscPKI\generated)

.PARAMETER ServerDns
    DNS ou IP do servidor ONS (osc-server). Pode ser passado multiplas vezes.
    Exemplos: upload.ons.intra  ou  10.0.1.50

.PARAMETER PkiDns
    DNS ou IP do servidor PKI (osc-pki-server). Pode ser passado multiplas vezes.

.PARAMETER ServerWorkDir
    Diretorio de trabalho do osc-server (default: D:\OscServer)

.PARAMETER PkiWorkDir
    Diretorio de trabalho do osc-pki-server (default: D:\OscPki)

.PARAMETER RootDays
    Validade da Root CA em dias (default: 3650)

.PARAMETER IntermediateDays
    Validade da CA Intermediaria em dias (default: 1095)

.PARAMETER CertDays
    Validade dos certificados de servidor em dias (default: 365)

.EXAMPLE
    .\setup-pki.ps1 -BinPath .\osc-pki-server.exe `
        -ServerDns upload.ons.intra `
        -PkiDns pki.ons.intra

.EXAMPLE
    # Com multiplos SANs (DNS + IP)
    .\setup-pki.ps1 -BinPath .\osc-pki-server.exe `
        -ServerDns upload.ons.intra -ServerDns 10.0.1.50 `
        -PkiDns pki.ons.intra -PkiDns 10.0.1.51
#>
param(
    [Parameter(Mandatory=$true)]
    [string]$BinPath,

    [string]$OutputDir = "D:\OscPKI\generated",

    [Parameter(Mandatory=$true)]
    [string[]]$ServerDns,

    [Parameter(Mandatory=$true)]
    [string[]]$PkiDns,

    [string]$ServerWorkDir = "D:\OscServer",
    [string]$PkiWorkDir    = "D:\OscPki",

    [int]$RootDays         = 3650,
    [int]$IntermediateDays = 1095,
    [int]$CertDays         = 365
)

$ErrorActionPreference = "Stop"

Write-Host "=== Setup PKI ===" -ForegroundColor Cyan
Write-Host ""

# Verificar que o binario existe
if (-not (Test-Path $BinPath)) {
    Write-Error "Binario nao encontrado: $BinPath"
    Write-Host "Compile primeiro com: cargo build -p osc-pki-server --release"
    exit 1
}

# Criar diretorio de saida
New-Item -ItemType Directory -Path $OutputDir -Force | Out-Null

# --- Passo 1: Gerar hierarquia PKI ---
Write-Host "[ 1/4 ] Gerando hierarquia PKI..." -ForegroundColor Yellow

$initArgs = @(
    "init-pki",
    "--output-dir", $OutputDir,
    "--root-days", $RootDays,
    "--intermediate-days", $IntermediateDays,
    "--cert-days", $CertDays
)
foreach ($dns in $ServerDns) { $initArgs += "--server-dns"; $initArgs += $dns }
foreach ($dns in $PkiDns)    { $initArgs += "--pki-dns";    $initArgs += $dns }

& $BinPath @initArgs
if ($LASTEXITCODE -ne 0) {
    Write-Error "init-pki falhou (exit $LASTEXITCODE)"
    exit 1
}

# --- Passo 2: Distribuir para D:\OscServer\certs\ ---
Write-Host ""
Write-Host "[ 2/4 ] Distribuindo certificados para $ServerWorkDir\certs\" -ForegroundColor Yellow

$srvCerts = "$ServerWorkDir\certs"
New-Item -ItemType Directory -Path $srvCerts -Force | Out-Null

Copy-Item "$OutputDir\server-cert.pem" "$srvCerts\server.pem"     -Force
Copy-Item "$OutputDir\server-key.pem"  "$srvCerts\server-key.pem" -Force
Copy-Item "$OutputDir\ca-chain.pem"    "$srvCerts\ca-bundle.pem"  -Force

Write-Host "  server.pem, server-key.pem, ca-bundle.pem -> $srvCerts"

# --- Passo 3: Distribuir para D:\OscPki\certs\ ---
Write-Host "[ 3/4 ] Distribuindo certificados para $PkiWorkDir\certs\" -ForegroundColor Yellow

$pkiCerts = "$PkiWorkDir\certs"
New-Item -ItemType Directory -Path $pkiCerts -Force | Out-Null

Copy-Item "$OutputDir\pki-server-cert.pem"  "$pkiCerts\server.pem"              -Force
Copy-Item "$OutputDir\pki-server-key.pem"   "$pkiCerts\server-key.pem"          -Force
Copy-Item "$OutputDir\intermediate-cert.pem" "$pkiCerts\intermediate-cert.pem"  -Force
Copy-Item "$OutputDir\intermediate-key.pem"  "$pkiCerts\intermediate-key.pem"   -Force
Copy-Item "$OutputDir\ca-chain.pem"          "$pkiCerts\ca-chain.pem"           -Force

Write-Host "  server.pem, server-key.pem, intermediate-*.pem, ca-chain.pem -> $pkiCerts"

# --- Passo 4: Restringir ACL nas chaves privadas ---
Write-Host "[ 4/4 ] Aplicando ACL restritiva nas chaves privadas..." -ForegroundColor Yellow

$keyFiles = @(
    "$srvCerts\server-key.pem",
    "$pkiCerts\server-key.pem",
    "$pkiCerts\intermediate-key.pem"
)

foreach ($keyFile in $keyFiles) {
    try {
        $acl = Get-Acl $keyFile
        $acl.SetAccessRuleProtection($true, $false)
        $acl.AddAccessRule((New-Object System.Security.AccessControl.FileSystemAccessRule(
            "SYSTEM", "FullControl", "None", "None", "Allow")))
        $acl.AddAccessRule((New-Object System.Security.AccessControl.FileSystemAccessRule(
            "Administrators", "FullControl", "None", "None", "Allow")))
        Set-Acl $keyFile $acl
        Write-Host "  ACL restritiva: $keyFile"
    } catch {
        Write-Host "  Aviso: nao foi possivel restringir ACL de ${keyFile}: $_" -ForegroundColor Yellow
    }
}

# --- Resumo e avisos ---
Write-Host ""
Write-Host "=== Setup concluido ===" -ForegroundColor Green
Write-Host ""
Write-Host "AVISO DE SEGURANCA:" -ForegroundColor Red
Write-Host "  $OutputDir\root-key.pem contem a chave privada da Root CA."
Write-Host "  >> MOVA-A AGORA para um pendrive cifrado (VeraCrypt) ou HSM <<"
Write-Host "  >> e APAGUE este arquivo desta maquina. <<"
Write-Host ""
Write-Host "PROXIMOS PASSOS:"
Write-Host "  1. Instalar osc-server:"
Write-Host "       .\install-server.ps1 -BinPath .\osc-server.exe"
Write-Host "       Start-Service OscServer"
Write-Host ""
Write-Host "  2. Instalar osc-pki-server:"
Write-Host "       .\install-pki-server.ps1 -BinPath .\osc-pki-server.exe"
Write-Host "       Start-Service OscPkiServer"
Write-Host ""
Write-Host "  3. Emitir certificados para cada subestacao:"
Write-Host "       .\new-station-cert.ps1 -StationId SE_XANXERE \"
Write-Host "           -OutputDir $PkiWorkDir\stations \"
Write-Host "           -CaKeyPath  $pkiCerts\intermediate-key.pem \"
Write-Host "           -CaCertPath $pkiCerts\intermediate-cert.pem"
Write-Host ""
Write-Host "  4. Copiar o ca-chain.pem para cada subestacao:"
Write-Host "       D:\OscAgent\certs\ca-chain.pem"
Write-Host ""
