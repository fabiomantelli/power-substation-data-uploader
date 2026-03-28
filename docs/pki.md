# Infraestrutura de Chave Pública (PKI)

## Hierarquia

```
Root CA (offline)
└── Intermediate CA (online, controlada)
    ├── server.pem       (osc-server ONS)
    └── SE_XANXERE.pem   (osc-agent subestação)
    └── SE_PALHOCA.pem
    └── ...
```

## Pré-requisitos

- `osc-pki-server.exe` compilado (`cargo build -p osc-pki-server --release`)
- PowerShell 5+
- OpenSSL no PATH apenas para verificação (opcional)

---

## Passo 1 — Bootstrap automático (Root CA + Intermediate CA + certs de servidor)

Um único script gera toda a hierarquia e distribui para os diretórios corretos.
**Não requer OpenSSL.**

```powershell
# Executar como Administrador na máquina do ONS
.\scripts\setup-pki.ps1 `
    -BinPath    .\osc-pki-server.exe `
    -ServerDns  upload.ons.intra `
    -PkiDns     pki.ons.intra

# Com múltiplos SANs (DNS + IP):
.\scripts\setup-pki.ps1 `
    -BinPath   .\osc-pki-server.exe `
    -ServerDns upload.ons.intra -ServerDns 10.0.1.50 `
    -PkiDns    pki.ons.intra    -PkiDns    10.0.1.51
```

O script:
1. Chama `osc-pki-server.exe init-pki` (usa `rcgen` internamente)
2. Gera Root CA, CA Intermediária, `ca-chain.pem`, cert do osc-server e cert do osc-pki-server
3. Distribui para `D:\OscServer\certs\` e `D:\OscPki\certs\`
4. Aplica ACL restritiva nas chaves privadas (SYSTEM + Administrators)
5. Avisa para mover `root-key.pem` para pendrive cifrado

> **Segurança:** após o script, mova `root-key.pem` para um pendrive cifrado (VeraCrypt) ou HSM
> e apague-o desta máquina. Ela só será necessária para renovar a CA intermediária.

Também é possível chamar o subcomando diretamente:

```powershell
osc-pki-server.exe init-pki `
    --output-dir D:\OscPKI\generated `
    --server-dns upload.ons.intra `
    --pki-dns    pki.ons.intra `
    --root-days  3650 `
    --intermediate-days 1095 `
    --cert-days  365
```

---

## Passo 2 — Criar certificado cliente para uma subestação

Use o script `new-station-cert.ps1`. Ele gera os nomes `<StationId>-cert.pem` e `<StationId>-key.pem`:

```powershell
.\scripts\new-station-cert.ps1 `
    -StationId  SE_XANXERE `
    -OutputDir  "D:\OscPki\stations" `
    -CaKeyPath  "D:\OscPki\certs\intermediate-key.pem" `
    -CaCertPath "D:\OscPki\certs\intermediate-cert.pem"
```

**Distribuir para a subestação:**

```powershell
# Os nomes na SE devem ser client.pem / client-key.pem / ca-chain.pem
Copy-Item "D:\OscPki\stations\SE_XANXERE-cert.pem" "D:\OscAgent\certs\client.pem"
Copy-Item "D:\OscPki\stations\SE_XANXERE-key.pem"  "D:\OscAgent\certs\client-key.pem"
Copy-Item "D:\OscPki\certs\ca-chain.pem"           "D:\OscAgent\certs\ca-chain.pem"
```

> Os nomes `client.pem`, `client-key.pem` e `ca-chain.pem` são os padrões referenciados
> no `agent.toml`. Se usar nomes diferentes, atualize o `agent.toml` correspondente.

---

## Verificar os certificados

```powershell
$GEN = "D:\OscPKI\generated"

# Ver validade
openssl x509 -in "$GEN\server-cert.pem"                  -noout -dates
openssl x509 -in "D:\OscPki\stations\SE_XANXERE-cert.pem" -noout -dates

# Verificar cadeia completa
openssl verify -CAfile "$GEN\ca-chain.pem" "$GEN\server-cert.pem"
openssl verify -CAfile "$GEN\ca-chain.pem" "D:\OscPki\stations\SE_XANXERE-cert.pem"
```

---

## Resumo — o que vai para cada lugar

| Arquivo gerado | Destino no osc-server | Destino no osc-pki-server | Destino na subestação |
|---------------|----------------------|--------------------------|----------------------|
| `server-cert.pem` | `D:\OscServer\certs\server.pem` | — | — |
| `server-key.pem` | `D:\OscServer\certs\server-key.pem` | — | — |
| `pki-server-cert.pem` | — | `D:\OscPki\certs\server.pem` | — |
| `pki-server-key.pem` | — | `D:\OscPki\certs\server-key.pem` | — |
| `intermediate-cert.pem` | — | `D:\OscPki\certs\` | — |
| `intermediate-key.pem` | — | `D:\OscPki\certs\` | — |
| `ca-chain.pem` | `D:\OscServer\certs\ca-bundle.pem` | `D:\OscPki\certs\ca-chain.pem` | `D:\OscAgent\certs\ca-chain.pem` |
| `SE_X-cert.pem` → `client.pem` | — | — | `D:\OscAgent\certs\client.pem` |
| `SE_X-key.pem` → `client-key.pem` | — | — | `D:\OscAgent\certs\client-key.pem` |

> `setup-pki.ps1` cuida automaticamente da distribuição para osc-server e osc-pki-server.
> O cert de subestação precisa ser copiado manualmente após emissão via `new-station-cert.ps1`.

---

## Ciclo de Vida

1. Emissão: `scripts/new-station-cert.ps1`
2. Distribuição: copiar cert + key para `D:\OscAgent\certs\`
3. Renovação: `scripts/renew-cert.ps1` (preserva chave quando possível)
4. Verificação de expiração: `scripts/check-expiry.ps1`
5. Revogação: revogar via openssl + publicar CRL

## Inventário

Manter planilha ou registro com:

| SE         | Thumbprint | Emissão    | Expiração  | Status |
|------------|------------|------------|------------|--------|
| SE_XANXERE | sha256:... | 2026-03-27 | 2027-03-27 | ativo  |

## Alertas

Executar `check-expiry.ps1` diariamente via Task Scheduler.
Alertar em: 45, 30, 15 e 7 dias antes da expiração.
