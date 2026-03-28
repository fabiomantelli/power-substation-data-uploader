<#
.SYNOPSIS
    Verifica expiracao de todos os certificados cliente no inventario.
.PARAMETER CertDir
    Diretorio com os certificados .pem das subestacoes
.PARAMETER WarnDays
    Alertar se expirar em menos de N dias (default: 45)
#>
param(
    [Parameter(Mandatory=$true)] [string]$CertDir,
    [int]$WarnDays = 45
)

if (-not (Get-Command openssl -ErrorAction SilentlyContinue)) {
    Write-Error "OpenSSL nao encontrado no PATH"
    exit 1
}

$today = Get-Date
$results = @()

foreach ($certFile in Get-ChildItem -Path $CertDir -Filter "*-cert.pem") {
    $expiryStr = openssl x509 -in $certFile.FullName -noout -enddate 2>$null
    if ($expiryStr -match "notAfter=(.+)") {
        $expiryDate = [DateTime]::ParseExact(
            $Matches[1].Trim(),
            "MMM  d HH:mm:ss yyyy GMT",
            [System.Globalization.CultureInfo]::InvariantCulture
        )
        $daysLeft = ($expiryDate - $today).Days
        $subject  = openssl x509 -in $certFile.FullName -noout -subject 2>$null

        $status = if ($daysLeft -lt 0) { "EXPIRADO" }
                  elseif ($daysLeft -lt 15) { "CRITICO" }
                  elseif ($daysLeft -lt $WarnDays) { "AVISO" }
                  else { "OK" }

        $results += [PSCustomObject]@{
            Arquivo       = $certFile.Name
            Subject       = $subject -replace "subject=", ""
            Expiracao     = $expiryDate.ToString("yyyy-MM-dd")
            DiasRestantes = $daysLeft
            Status        = $status
        }
    }
}

$results | Sort-Object DiasRestantes | Format-Table -AutoSize

$critical = $results | Where-Object { $_.Status -in "EXPIRADO","CRITICO","AVISO" }
if ($critical) {
    Write-Host ""
    Write-Host "ATENCAO: $($critical.Count) certificado(s) precisam de atencao!" -ForegroundColor Yellow
    exit 1
}
