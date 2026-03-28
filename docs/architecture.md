# Arquitetura do Sistema

## Visão Geral

Sistema de transferência segura de oscilografias entre subestações e o ONS.

```
[SE Remota]                          [ONS]
  osc-agent                         osc-server
  │                                  │
  ├─ inbox/    ← arquivos COMTRADE   ├─ staging/
  ├─ queue/    ← fila persistente    ├─ repository/  ← armazenamento final
  ├─ spool/    ← em processamento    ├─ quarantine/  ← hashes inválidos
  ├─ sent/     ← enviados            ├─ audit/       ← log imutável
  └─ error/    ← falhas              └─ logs/
       │                                  ▲
       └──── HTTPS / mTLS ───────────────┘
              (outbound only)
              TLS 1.2+
              Certificado cliente por SE
```

## Fluxo de Dados

1. Relay/IED deposita .cfg, .dat, .hdr em inbox/
2. osc-agent detecta via filesystem watcher (notify)
3. Agrupa arquivos do mesmo evento por stem
4. Calcula SHA-256 de cada arquivo
5. Gera manifest.json com hashes e metadados
6. Move para spool/<event_id>/
7. Persiste na fila (queue/<event_id>.json)
8. Sender envia via HTTPS multipart com certificado cliente
9. osc-server valida mTLS, station_id, hashes
10. Grava em staging/, verifica, promove para repository/
11. Retorna ack com hash_verified=true
12. Agent move para sent/, remove da fila

## Componentes

| Componente | Localização | Tecnologia            |
|------------|-------------|-----------------------|
| osc-agent  | Subestação  | Rust, Windows Service |
| osc-server | ONS         | Rust, Windows Service |
| PKI        | ONS offline | OpenSSL / step-ca     |
| scripts    | Admin       | PowerShell            |

## Segurança

- mTLS obrigatório: sem certificado cliente, sem upload
- Cada SE tem certificado único (EKU: clientAuth)
- CA interna com root offline + intermediate
- Firewall: apenas porta 8443 outbound das SEs
- Hash SHA-256 verificado no servidor
- staging → validate → repository (nunca direto)
- quarantine para pacotes com hash inválido
- Audit log imutável (JSONL append-only)
