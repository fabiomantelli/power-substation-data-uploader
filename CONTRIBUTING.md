# Contributing

## Prerequisites

- [Rust](https://rustup.rs/) stable toolchain
- Windows (the agent and service integrations are Windows-only)
- OpenSSL (required for the certificate management scripts in `scripts/`)
- PowerShell 5+ (for deployment scripts)

## Local Setup

```bash
git clone https://github.com/your-org/power-oscillography-uploader.git
cd power-oscillography-uploader
cargo build --release --all
```

Binaries will be at `target/release/osc-agent.exe`, `osc-server.exe`, and `osc-pki-server.exe`.

Copy and fill in the example configs before running:

```bash
cp config/agent.example.toml  config/agent.toml
cp config/server.example.toml config/server.toml
cp config/pki.example.toml    config/pki.toml
```

## Branch Naming

| Prefix | Use for |
|--------|---------|
| `feat/` | New features |
| `fix/` | Bug fixes |
| `refactor/` | Code cleanup with no behavior change |
| `docs/` | Documentation only |

Example: `feat/disk-aware-queue-pause`, `fix/renewal-race-condition`

## Pull Request Process

1. Branch off `main`.
2. Keep PRs focused — one concern per PR.
3. CI must pass (build, clippy, fmt) before merge.
4. At least one reviewer approval required before merging.

## Code Style

Style is enforced automatically — just run before pushing:

```bash
cargo fmt --all
cargo clippy --all-targets --all-features -- -D warnings
```

## Security

- **Never commit** `.pem`, `.key`, `.p12`, or any certificate/key material.
- **Never commit** `config/agent.toml`, `config/server.toml`, or `config/pki.toml` — use the `.example.toml` files as templates.
- If you accidentally commit a secret, treat it as compromised immediately and rotate it.
