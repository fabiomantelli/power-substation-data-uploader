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

## Root CA

- Mantida offline (pendrive cifrado ou HSM)
- Usada apenas para assinar a Intermediate CA
- Validade: 10 anos
- Algoritmo: RSA 4096 ou ECDSA P-384

## Intermediate CA

- Online, no servidor de gestão do ONS
- Emite certificados de servidor e cliente
- Validade: 3 anos
- CRL publicada internamente

## Certificados de Servidor (ONS)

- EKU: serverAuth
- SAN: DNS do endpoint (ex: upload.ons.intra)
- Validade: 1 ano
- Renovação: 45 dias antes da expiração

## Certificados Cliente (Subestação)

- EKU: clientAuth
- CN: identificador da SE (ex: SE_XANXERE)
- SAN: DNS:SE_XANXERE
- Validade: 1 ano
- Renovação: 45 dias antes
- Uma chave por SE; renovação preserva a chave quando possível

## Ciclo de Vida

1. Emissão: `scripts/new-station-cert.ps1`
2. Distribuição: copiar cert + key para D:\OscAgent\certs\
3. Renovação: `scripts/renew-cert.ps1` (preserva chave)
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
