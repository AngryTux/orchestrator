# Orchestrator

Secure LLM workspace manager and orchestrator. Run AI providers in isolated sandboxes with encrypted credentials, kernel-level security, and full cost visibility.

## What it does

- **Isolated execution** — Every provider runs in a sandboxed process with Landlock, seccomp, rlimits, and clean environment
- **Credential management** — API keys encrypted with AES-256-GCM, scoped per namespace
- **Multi-formation** — Solo (single provider) and Duet (parallel + consolidation). Quartet, Chamber, Symphonic coming in Act II
- **Metrics** — SQLite-backed history of every performance: tokens, cost, duration
- **Namespaces** — Isolated contexts (default, secure, lab) with independent credentials and configs

## Requirements

- Linux kernel >= 6.12 (Landlock ABI v6)
- Rust (edition 2024)
- systemd (for socket activation)

## Quick start

```bash
# Build
make build

# Run tests
make test

# Full CI check (fmt + lint + audit + tests)
make check

# Start daemon
make run

# Smoke test (end-to-end with mock provider)
make smoke
```

## Usage

```bash
# Start the daemon
make run

# Store a provider credential
curl -s --unix-socket $XDG_RUNTIME_DIR/orchestrator/orchestrator.sock \
  http://localhost/v1/namespaces/default/providers \
  -X POST -H "Content-Type: application/json" \
  -d '{"name": "claude", "key": "YOUR_API_KEY"}'

# Run a performance
curl -s --unix-socket $XDG_RUNTIME_DIR/orchestrator/orchestrator.sock \
  http://localhost/v1/namespaces/default/performances \
  -X POST -H "Content-Type: application/json" \
  -d '{"prompt": "what is CQRS?", "provider": "claude"}'

# System info
curl -s --unix-socket $XDG_RUNTIME_DIR/orchestrator/orchestrator.sock \
  http://localhost/v1/system/info | python3 -m json.tool
```

## API

```
GET  /v1/system/health                          Health check
GET  /v1/system/version                         Version info
GET  /v1/system/info                            Host detection (kernel, Landlock, CPU, RAM)
POST /v1/namespaces/{ns}/providers              Add provider credential
GET  /v1/namespaces/{ns}/providers              List configured providers
POST /v1/namespaces/{ns}/performances           Execute a performance
GET  /v1/namespaces/{ns}/performances           List past performances
GET  /v1/namespaces/{ns}/performances/{id}      Get performance details
GET  /v1/metrics                                Aggregate metrics
```

## Project structure

```
crates/
  orch-core/       Core library (contracts, engine, isolation, credentials, metrics)
  orchestratord/   Daemon binary (systemd socket activation, HTTP API)
  orch/            CLI binary (Act I)

contrib/systemd/   systemd user units
repertoire/        Provider specs (YAML)
scripts/           Smoke tests
```

## Security

8-layer process isolation:

1. `PR_SET_NO_NEW_PRIVS` — prevents setuid escalation
2. rlimits — NPROC + RLIMIT_AS (fork-bomb and OOM prevention)
3. Landlock — filesystem restriction (no Execute on write paths)
4. Seccomp BPF — syscall allowlist (~100 safe syscalls)
5. close_range — inherited fd leak prevention
6. Clean environment — only specified variables visible
7. stdin /dev/null — no terminal access
8. Timeout — SIGKILL on process timeout

## License

MIT
