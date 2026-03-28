# Guia Operacional

## Instalação — Subestação

1. Copiar `osc-agent.exe` para servidor da SE
2. Criar `D:\OscAgent\config\agent.toml` baseado em `agent.example.toml`
3. Instalar certificados em `D:\OscAgent\certs\`
4. Executar como Administrador:
   ```powershell
   .\install-agent.ps1 -BinPath .\osc-agent.exe
   Start-Service OscAgent
   ```

## Instalação — ONS

1. Copiar `osc-server.exe` para servidor ONS
2. Criar `D:\OscServer\config\server.toml`
3. Instalar certificados em `D:\OscServer\certs\`
4. Executar como Administrador:
   ```powershell
   .\install-server.ps1 -BinPath .\osc-server.exe
   Start-Service OscServer
   ```

## Adicionar Nova Subestação

1. Emitir certificado: `.\new-station-cert.ps1 -StationId SE_NOVA ...`
2. Adicionar `SE_NOVA` em `allowed_station_ids` no server.toml
3. Reiniciar OscServer: `Restart-Service OscServer`
4. Enviar cert + key para SE
5. Atualizar agent.toml da SE
6. Reiniciar OscAgent na SE
7. Registrar no inventário de certificados

## Verificar Status

```powershell
# Na SE
Get-Service OscAgent
osc-agent.exe --config D:\OscAgent\config\agent.toml status

# No ONS
Get-Service OscServer
# Ver logs
Get-Content D:\OscServer\logs\osc-server.log -Tail 50
```

## Renovar Certificado

```powershell
.\renew-cert.ps1 -StationId SE_XANXERE `
    -CertDir D:\OscAdmin\certs `
    -CaKeyPath D:\OscAdmin\intermediate-key.pem `
    -CaCertPath D:\OscAdmin\intermediate-cert.pem
# Distribuir novo cert para a SE
# Reiniciar OscAgent na SE
```

## Verificar Expiração Diária

```powershell
# Agendar via Task Scheduler
.\check-expiry.ps1 -CertDir D:\OscAdmin\certs -WarnDays 45
```

## Resolução de Problemas

### Fila crescendo na SE
- Verificar conectividade com ONS (ping, telnet na porta 8443)
- Verificar validade do certificado cliente (`check-expiry.ps1`)
- Verificar logs: `D:\OscAgent\logs\`

### Upload rejeitado por hash
- O ONS moverá o pacote para quarantine/
- Verificar integridade do arquivo na SE (o original ainda está em queue/ ou spool/)
- Reenvio manual: mover o evento de volta para inbox/

### Disco quase cheio na SE
- Verificar se ONS está confirmando recebimentos
- Limpeza manual de sent/ se confirmado pelo ONS
- Revisar configurações de retenção

### Certificado expirado
1. Emitir novo: `new-station-cert.ps1`
2. Distribuir para SE
3. Reiniciar OscAgent
