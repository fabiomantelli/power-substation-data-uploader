# Substation Data Uploader (OSC)

A secure, resilient system for transferring oscillography files (COMTRADE format) from remote electrical substations to the Brazilian National Electric System Operator (ONS). Built in Rust, it implements mutual TLS authentication, SHA-256 hash verification, persistent queuing, and automated certificate renewal.

## Overview

The system consists of three independent binaries that work together:

```
[Substation IED/Relay]
        |
        v
  osc-agent.exe          ← runs on each substation (Windows Service)
        |
        | HTTPS / mTLS
        v
  osc-server.exe         ← runs at ONS receiving endpoint (Windows Service)

  osc-pki-server.exe     ← PKI service for automated certificate renewal
```

| Component | Binary | Responsibility |
|-----------|--------|----------------|
| **Substation Agent** | `osc-agent` | Watches inbox, builds manifests, queues and uploads files |
| **ONS Receiving Server** | `osc-server` | Receives uploads, verifies hashes, archives to repository |
| **PKI Server** | `osc-pki-server` | Issues and renews station client certificates |

## Key Features

- **Mutual TLS (mTLS)** — every station has a unique client certificate; no certificate means no upload
- **Persistent queue** — JSON-based local queue survives process restarts and network outages
- **SHA-256 verification** — hashes are computed on the agent and re-verified on the server
- **Intelligent retention** — hybrid time + disk capacity policy with configurable watermarks (70 / 80 / 90%)
- **Automated certificate renewal** — agent renews its own cert 30 days before expiry
- **Exponential backoff** — retry delay grows from 30 s up to 3600 s across up to 10 attempts
- **Debounced filesystem watching** — detects new COMTRADE files without false triggers
- **Immutable audit trail** — append-only JSONL audit logs on both server and PKI
- **Windows Service integration** — all components run as NT services with auto-restart

## Data Flow

```
Relay/IED drops file → inbox/
        │
        └─ watcher detects .cfg/.dat/.hdr/.inf
                │
                └─ manifest (SHA-256 hashes) + enqueue
                        │
                        └─ sender loop (exponential backoff retry)
                                │
                        HTTPS/mTLS multipart upload
                                │
                        ┌───────┴────────┐
                        │  osc-server    │
                        │  validate mTLS │
                        │  verify hashes │
                        └───────┬────────┘
                    ┌───────────┴────────────┐
               all valid               any invalid
                    │                        │
              repository/              quarantine/
              audit log                audit log
```

## Project Structure

```
substation-data-uploader/
├── substation/        # osc-agent source (Rust)
├── ons/               # osc-server source (Rust)
├── pki/               # osc-pki-server source (Rust)
├── config/            # Example TOML configuration files
│   ├── agent.example.toml
│   ├── server.example.toml
│   └── pki.example.toml
└── scripts/           # PowerShell deployment and certificate management
    ├── install-agent.ps1
    ├── install-server.ps1
    ├── install-pki-server.ps1
    ├── new-station-cert.ps1
    ├── renew-cert.ps1
    └── check-expiry.ps1
```

## Requirements

- Rust 2021 edition toolchain (`rustup` recommended)
- Windows (production deployment uses `windows-service`)
- OpenSSL (for certificate management scripts)
- PowerShell 5+ (for deployment scripts)

## Build

```bash
# Build all components
cargo build --release

# Binaries will be at:
#   target/release/osc-agent.exe
#   target/release/osc-server.exe
#   target/release/osc-pki-server.exe
```

## Configuration

Copy the example configs and fill in your values:

```bash
cp config/agent.example.toml  config/agent.toml
cp config/server.example.toml config/server.toml
cp config/pki.example.toml    config/pki.toml
```

### Agent (`agent.toml`)

| Key | Description |
|-----|-------------|
| `station_id` | Unique identifier for this substation (e.g. `SE_XANXERE`) |
| `device_id` | Device identifier (e.g. `F60_01`) |
| `inbox_dir` | Directory monitored for new COMTRADE files |
| `server_url` | ONS receiving server URL |
| `client_cert_pem` / `client_key_pem` | mTLS client certificate and key |
| `ca_bundle_pem` | CA chain used to validate the server certificate |
| `max_retries` | Maximum upload attempts before moving to error/ |
| `[retention]` | Time-based retention for sent/error/log directories |
| `[disk]` | Disk usage thresholds (warn / reduce / force cleanup) |
| `[renewal]` | PKI server URL and renewal schedule |

### Server (`server.toml`)

| Key | Description |
|-----|-------------|
| `listen_addr` | Bind address (default `0.0.0.0:8443`) |
| `allowed_station_ids` | Whitelist of valid station identifiers |
| `repository_dir` | Final archive for validated uploads |
| `quarantine_dir` | Holds uploads that failed hash verification |
| `[rate_limit]` | Max uploads per minute per station |

### PKI Server (`pki.toml`)

| Key | Description |
|-----|-------------|
| `listen_addr` | Bind address (default `0.0.0.0:8444`) |
| `renewal_window_days_max` | How early before expiry renewal is allowed (default 60 days) |
| `issued_cert_validity_days` | Validity period for newly issued certificates (default 365) |

## Usage

### Run in Foreground

```powershell
# Substation agent
osc-agent.exe --config config/agent.toml run

# ONS server
osc-server.exe --config config/server.toml run

# PKI server
osc-pki-server.exe --config config/pki.toml
```

### Deploy as Windows Service (recommended)

Use the PowerShell scripts to install and register the service. They copy the binary, create the directory structure, and register the service with the correct `run-service` argument for the Windows SCM:

```powershell
# On the substation host
.\scripts\install-agent.ps1 -BinPath .\osc-agent.exe
Start-Service OscAgent
Get-Service  OscAgent

# On the ONS server host
.\scripts\install-server.ps1 -BinPath .\osc-server.exe
Start-Service OscServer
Get-Service  OscServer
```

> **Note:** The `run-service` subcommand is used internally by the Windows Service Control Manager (SCM).
> Do not invoke it directly — use `run` for foreground testing.

### Agent Service — Additional Commands

```powershell
Stop-Service   OscAgent
Restart-Service OscAgent

# Check upload queue depth
osc-agent.exe --config D:\OscAgent\config\agent.toml status

# Uninstall the service
osc-agent.exe --config D:\OscAgent\config\agent.toml uninstall-service
```

### PowerShell Certificate Management Scripts

```powershell
# Issue a new client certificate for a station
.\scripts\new-station-cert.ps1 -StationId SE_XANXERE

# Check certificate expiry across all stations
.\scripts\check-expiry.ps1

# Renew a specific station certificate manually
.\scripts\renew-cert.ps1 -StationId SE_XANXERE
```

## Directory Layout (Agent Host)

```
D:\OscAgent\
├── config\agent.toml
├── certs\
│   ├── client.pem          # Client certificate (renewed automatically)
│   ├── client-key.pem      # Client private key
│   └── ca-chain.pem        # CA bundle for server validation
├── inbox\                  # Drop zone for new COMTRADE files
├── queue\                  # Persistent upload queue (JSON)
├── spool\                  # Files being processed
├── sent\                   # Successfully uploaded (retention-managed)
├── error\                  # Failed uploads (retention-managed)
├── state\                  # Runtime state (renewal tracking)
└── logs\                   # Daily rolling structured logs
```

## Security Notes

- The root CA key must be kept **offline** (USB key or HSM).
- The intermediate CA key (used by `osc-pki-server`) must be protected with strict file permissions.
- All TLS connections require TLS 1.2 or higher; TLS 1.0/1.1 are disabled.
- mTLS is mandatory on every endpoint — unauthenticated requests are rejected at the TLS handshake.
- Uploads with any hash mismatch are moved to `quarantine/` and never written to the repository.

## Logging

Set `RUST_LOG` to control log verbosity (default: `info`):

```bash
set RUST_LOG=debug
osc-agent.exe --config config/agent.toml run
```

Logs are written as structured JSON to daily-rotating files under the configured `log_dir`, and also to stderr.

## Tech Stack

| Layer | Libraries |
|-------|-----------|
| Async runtime | Tokio 1 (full) |
| HTTP server | Axum 0.7, axum-server 0.7, Tower-HTTP |
| HTTP client | Reqwest 0.12 |
| TLS | Rustls 0.23 |
| PKI / Certs | rcgen 0.13, x509-parser 0.16, rustls-pemfile |
| Hashing | SHA2 0.10 |
| Filesystem watch | Notify 6 |
| Serialization | Serde, serde_json, TOML 0.8 |
| Logging | tracing, tracing-subscriber, tracing-appender |
| CLI | Clap 4 |
| Windows Service | windows-service 0.7 |
| Disk info | sysinfo 0.30 |
