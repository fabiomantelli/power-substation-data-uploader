# Runbook — Incidentes

## INC-01: SE não envia há mais de 1 hora

**Sintomas:** Fila de queue/ crescendo, sem novos eventos no ONS.

**Diagnóstico:**
1. `Get-Service OscAgent` — serviço rodando?
2. Logs em `D:\OscAgent\logs\`
3. Testar conectividade: `Test-NetConnection upload.ons.intra -Port 8443`
4. `check-expiry.ps1` — certificado válido?

**Resolução:**
- Serviço parado: `Start-Service OscAgent`
- Certificado expirado: emitir novo e reiniciar
- Rede indisponível: acionar NOC

---

## INC-02: Upload rejeitado por certificado inválido

**Sintomas:** Logs com "certificado não autorizado" ou "TLS handshake failed".

**Diagnóstico:**
1. Verificar se station_id está em allowed_station_ids no server.toml
2. Verificar se CA bundle está correta em ambos os lados
3. `openssl verify -CAfile ca-chain.pem client-cert.pem`

**Resolução:**
- Adicionar station_id ao server.toml e reiniciar OscServer
- Se CA errada: redistribuir ca-chain.pem correto

---

## INC-03: Disco cheio na SE

**Sintomas:** OscAgent para de processar, erros de disco.

**Diagnóstico:**
1. `Get-PSDrive D` — uso atual
2. Verificar se sent/ está sendo limpo

**Resolução:**
1. Confirmar com ONS quais eventos foram recebidos
2. Limpar manualmente sent/ para eventos confirmados
3. Revisar configuração de retenção

---

## INC-04: Suspeita de comprometimento de SE

**Ação imediata:**
1. Revogar certificado: `openssl ca -revoke SE_X-cert.pem`
2. Publicar nova CRL
3. Remover SE de `allowed_station_ids`
4. Reiniciar OscServer
5. Investigar logs de audit/

---

## Contatos

| Papel        | Responsabilidade                        |
|--------------|-----------------------------------------|
| Admin PKI    | Emissão e revogação de certificados     |
| NOC          | Conectividade e firewall                |
| Operação SE  | Reinicialização do agente               |
