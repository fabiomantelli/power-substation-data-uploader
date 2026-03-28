#Requires -RunAsAdministrator
<#
.SYNOPSIS
    Instala o osc-server como Windows Service no ONS.
#>
param(
    [Parameter(Mandatory=$true)]
    [string]$BinPath,

    [string]$ConfigPath = "D:\OscServer\config\server.toml",
    [string]$WorkDir = "D:\OscServer"
)

$ServiceName = "OscServer"
$DisplayName = "OSC Server - Recepcao de Oscilografias ONS"
$Description  = "Recebe oscilografias das subestacoes via HTTPS/mTLS."

Write-Host "=== Instalando $ServiceName ===" -ForegroundColor Cyan

$dirs = @(
    "$WorkDir\staging",
    "$WorkDir\repository",
    "$WorkDir\quarantine",
    "$WorkDir\audit",
    "$WorkDir\logs",
    "$WorkDir\certs",
    "$WorkDir\config"
)
foreach ($d in $dirs) {
    if (-not (Test-Path $d)) {
        New-Item -ItemType Directory -Path $d | Out-Null
        Write-Host "  Criado: $d"
    }
}

$DestBin = "$WorkDir\osc-server.exe"
$ResolvedBinObj = Resolve-Path $BinPath -ErrorAction SilentlyContinue
$ResolvedBin = if ($ResolvedBinObj) { $ResolvedBinObj.Path } else { $null }
if ($ResolvedBin -and ($ResolvedBin -ieq (Join-Path $WorkDir "osc-server.exe"))) {
    Write-Host "  Executavel ja esta em $DestBin -- copia ignorada"
} else {
    Copy-Item $BinPath $DestBin -Force
    Write-Host "  Executavel copiado para: $DestBin"
}

if (Get-Service -Name $ServiceName -ErrorAction SilentlyContinue) {
    Stop-Service -Name $ServiceName -Force -ErrorAction SilentlyContinue
    sc.exe delete $ServiceName | Out-Null
    Start-Sleep -Seconds 2
}

$binWithArgs = "`"$DestBin`" --config `"$WorkDir\config\server.toml`" run-service"
New-Service `
    -Name $ServiceName `
    -DisplayName $DisplayName `
    -Description $Description `
    -BinaryPathName $binWithArgs `
    -StartupType Automatic `
    -ErrorAction Stop

sc.exe failure $ServiceName reset= 86400 actions= restart/10000/restart/30000/restart/60000 | Out-Null

Write-Host "  Servico criado com sucesso" -ForegroundColor Green
Write-Host "Para iniciar: Start-Service $ServiceName"
