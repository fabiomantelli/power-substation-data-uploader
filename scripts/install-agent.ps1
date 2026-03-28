#Requires -RunAsAdministrator
<#
.SYNOPSIS
    Instala o osc-agent como Windows Service na subestacao.
.PARAMETER BinPath
    Caminho para o executavel osc-agent.exe
.PARAMETER ConfigPath
    Caminho para agent.toml
.PARAMETER WorkDir
    Diretorio de trabalho (default: D:\OscAgent)
#>
param(
    [Parameter(Mandatory=$true)]
    [string]$BinPath,

    [string]$ConfigPath = "D:\OscAgent\config\agent.toml",
    [string]$WorkDir = "D:\OscAgent"
)

$ServiceName = "OscAgent"
$DisplayName = "OSC Agent - Transferencia de Oscilografias"
$Description  = "Monitora a pasta de oscilografias e envia para o ONS via HTTPS/mTLS."

Write-Host "=== Instalando $ServiceName ===" -ForegroundColor Cyan

# Criar estrutura de diretorios
$dirs = @(
    "$WorkDir\inbox",
    "$WorkDir\queue",
    "$WorkDir\sent",
    "$WorkDir\error",
    "$WorkDir\spool",
    "$WorkDir\logs",
    "$WorkDir\state",
    "$WorkDir\certs",
    "$WorkDir\config"
)
foreach ($d in $dirs) {
    if (-not (Test-Path $d)) {
        New-Item -ItemType Directory -Path $d | Out-Null
        Write-Host "  Criado: $d"
    }
}

# Copiar executavel
$DestBin = "$WorkDir\osc-agent.exe"
$ResolvedBinObj = Resolve-Path $BinPath -ErrorAction SilentlyContinue
$ResolvedBin = if ($ResolvedBinObj) { $ResolvedBinObj.Path } else { $null }
if ($ResolvedBin -and ($ResolvedBin -ieq (Join-Path $WorkDir "osc-agent.exe"))) {
    Write-Host "  Executavel ja esta em $DestBin -- copia ignorada"
} else {
    Copy-Item $BinPath $DestBin -Force
    Write-Host "  Executavel copiado para $DestBin"
}

# Copiar config se fornecida
if (Test-Path $ConfigPath) {
    $DestConfig = "$WorkDir\config\agent.toml"
    $ResolvedCfgObj = Resolve-Path $ConfigPath -ErrorAction SilentlyContinue
    $ResolvedCfg = if ($ResolvedCfgObj) { $ResolvedCfgObj.Path } else { $null }
    $ResolvedDstObj = Resolve-Path $DestConfig -ErrorAction SilentlyContinue
    $ResolvedDst = if ($ResolvedDstObj) { $ResolvedDstObj.Path } else { $null }
    if ($ResolvedCfg -and $ResolvedDst -and ($ResolvedCfg -ieq $ResolvedDst)) {
        Write-Host "  Config ja esta em $DestConfig -- copia ignorada"
    } else {
        Copy-Item $ConfigPath $DestConfig -Force
        Write-Host "  Config copiada"
    }
}

# Remover servico existente se houver
if (Get-Service -Name $ServiceName -ErrorAction SilentlyContinue) {
    Stop-Service -Name $ServiceName -Force -ErrorAction SilentlyContinue
    sc.exe delete $ServiceName | Out-Null
    Start-Sleep -Seconds 2
    Write-Host "  Servico anterior removido"
}

# Criar servico
$binWithArgs = "`"$DestBin`" --config `"$WorkDir\config\agent.toml`" run-service"
New-Service `
    -Name $ServiceName `
    -DisplayName $DisplayName `
    -Description $Description `
    -BinaryPathName $binWithArgs `
    -StartupType Automatic `
    -ErrorAction Stop

# Configurar recovery
sc.exe failure $ServiceName reset= 86400 actions= restart/30000/restart/60000/restart/120000 | Out-Null

Write-Host "  Servico criado com sucesso" -ForegroundColor Green
Write-Host ""
Write-Host "Para iniciar: Start-Service $ServiceName"
Write-Host "Para status:  Get-Service $ServiceName"
Write-Host "Logs em:      $WorkDir\logs\"
