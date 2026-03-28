#Requires -RunAsAdministrator
<#
.SYNOPSIS
    Instala o osc-pki-server como Windows Service.
    IMPORTANTE: Esta maquina contera a chave privada da CA intermediaria.
    Deve ser uma maquina SEPARADA do osc-server.
.PARAMETER BinPath
    Caminho para o executavel osc-pki-server.exe
.PARAMETER ConfigPath
    Caminho para pki.toml
.PARAMETER WorkDir
    Diretorio de trabalho (default: D:\OscPki)
#>
param(
    [Parameter(Mandatory=$true)]
    [string]$BinPath,

    [string]$ConfigPath = "D:\OscPki\config\pki.toml",
    [string]$WorkDir = "D:\OscPki"
)

$ServiceName = "OscPkiServer"
$DisplayName  = "OSC PKI Server - Renovacao de Certificados"
$Description  = "Servidor PKI para renovacao automatica de certificados de subestacoes via HTTPS."

Write-Host "=== Instalando $ServiceName ===" -ForegroundColor Cyan
Write-Host ""
Write-Host "AVISO DE SEGURANCA:" -ForegroundColor Yellow
Write-Host "  Esta maquina armazenara a chave privada da CA intermediaria." -ForegroundColor Yellow
Write-Host "  Certifique-se de que esta maquina e SEPARADA do osc-server." -ForegroundColor Yellow
Write-Host "  Restrinja o acesso a porta 8444 apenas as subestacoes." -ForegroundColor Yellow
Write-Host ""

# Criar estrutura de diretorios
$dirs = @(
    "$WorkDir\certs",
    "$WorkDir\audit",
    "$WorkDir\logs",
    "$WorkDir\config"
)
foreach ($d in $dirs) {
    if (-not (Test-Path $d)) {
        New-Item -ItemType Directory -Path $d | Out-Null
        Write-Host "  Criado: $d"
    }
}

# Restringir permissoes no diretorio de certs (apenas SYSTEM e Administradores)
try {
    $acl = Get-Acl "$WorkDir\certs"
    $acl.SetAccessRuleProtection($true, $false)
    $ruleSystem = New-Object System.Security.AccessControl.FileSystemAccessRule(
        "SYSTEM", "FullControl", "ContainerInherit,ObjectInherit", "None", "Allow"
    )
    $ruleAdmins = New-Object System.Security.AccessControl.FileSystemAccessRule(
        "Administrators", "FullControl", "ContainerInherit,ObjectInherit", "None", "Allow"
    )
    $acl.AddAccessRule($ruleSystem)
    $acl.AddAccessRule($ruleAdmins)
    Set-Acl "$WorkDir\certs" $acl
    Write-Host "  Permissoes restritivas aplicadas em $WorkDir\certs" -ForegroundColor Green
} catch {
    Write-Host "  Aviso: nao foi possivel restringir permissoes automaticamente: $_" -ForegroundColor Yellow
}

# Copiar executavel
$DestBin = "$WorkDir\osc-pki-server.exe"
$ResolvedBinObj = Resolve-Path $BinPath -ErrorAction SilentlyContinue
$ResolvedBin = if ($ResolvedBinObj) { $ResolvedBinObj.Path } else { $null }
if ($ResolvedBin -and ($ResolvedBin -ieq (Join-Path $WorkDir "osc-pki-server.exe"))) {
    Write-Host "  Executavel ja esta em $DestBin -- copia ignorada"
} else {
    Copy-Item $BinPath $DestBin -Force
    Write-Host "  Executavel copiado para: $DestBin"
}

# Copiar config se fornecida
if (Test-Path $ConfigPath) {
    $DestConfig = "$WorkDir\config\pki.toml"
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
$binWithArgs = "`"$DestBin`" --config `"$WorkDir\config\pki.toml`""
New-Service `
    -Name $ServiceName `
    -DisplayName $DisplayName `
    -Description $Description `
    -BinaryPathName $binWithArgs `
    -StartupType Automatic `
    -ErrorAction Stop

# Configurar recovery automatico
sc.exe failure $ServiceName reset= 86400 actions= restart/30000/restart/60000/restart/120000 | Out-Null

Write-Host ""
Write-Host "=== $ServiceName instalado com sucesso ===" -ForegroundColor Green
Write-Host ""
Write-Host "PROXIMOS PASSOS OBRIGATORIOS:"
Write-Host "  1. Copiar intermediate-cert.pem para $WorkDir\certs\"
Write-Host "  2. Copiar intermediate-key.pem para $WorkDir\certs\  *** PROTEGER COM ACL ***"
Write-Host "  3. Copiar server.pem e server-key.pem para $WorkDir\certs\"
Write-Host "  4. Copiar ca-chain.pem para $WorkDir\certs\"
Write-Host "  5. Revisar $WorkDir\config\pki.toml"
Write-Host "  6. Configurar firewall: porta 8444 acessivel APENAS pelas SEs autorizadas"
Write-Host "  7. Start-Service $ServiceName"
Write-Host ""
Write-Host "Logs em: $WorkDir\logs\"
Write-Host "Audit em: $WorkDir\audit\"
