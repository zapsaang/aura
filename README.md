# AURA - Asynchronous Ultra-low-overhead Resource Agent

---

## Overview

AURA is a nanosecond-level system telemetry probe designed for geeks and low-level developers. It uses a C/S architecture with shared memory IPC (per-buffer SeqLock + bytemuck zero-copy serialization) for zero system call data retrieval.

### Key Features

- **Zero-copy IPC**: Shared memory in a private per-user runtime directory with a per-buffer SeqLock double buffer
- **No per-cycle allocation**: Collectors reuse preallocated buffers and fixed-size arrays; no heap allocation per collection cycle
- **Daemon-side derivation**: All percentages, aggregates, and threshold tones are computed by the daemon; the CLI only renders
- **Sub-millisecond overhead**: Entire CLI lifecycle completes in microseconds, bounded by wall-clock read deadlines
- **Cross-platform**: Linux (/proc) and macOS (public Mach/sysctl APIs only)
- **Seven telemetry dimensions**: CPU, Process, Memory, Storage, Network, Meta, GPU

### Architecture

```
┌─────────────────┐   per-user SHM    ┌─────────────────┐
│   aura-daemon   │ ──── SeqLock ───→ │    aura-cli     │
│   (producer)    │  TelemetryArchive │   (consumer)    │
│  500ms heartbeat│  (ABI v2, 2×64KiB)│   µs response   │
└─────────────────┘                   └─────────────────┘
```

## Quick Start

### Prerequisites

- Rust 1.70+ (release builds use 1.85.0)
- Linux (recommended) or macOS

### Build

```bash
cargo build --workspace
```

### Run

```bash
# Start daemon (background)
cargo run -p aura-daemon

# Query telemetry
cargo run -p aura-cli -- -m cpu
cargo run -p aura-cli -- -m mem
cargo run -p aura-cli -- -m all
```

## CLI Usage

```bash
# Module selection (-m/--module, default: all)
aura-cli -m cpu       # CPU (global, per-core, context switches)
aura-cli -m process   # Process (state counts, top CPU/memory; alias: proc)
aura-cli -m mem       # Memory (RAM; alias: memory)
aura-cli -m swap      # Swap
aura-cli -m disk      # Storage (disk I/O, mounts; alias: storage)
aura-cli -m net       # Network (bytes, rates; alias: network)
aura-cli -m os        # Meta (OS fingerprint, load, uptime; alias: meta)
aura-cli -m gpu       # GPU (Linux only)
aura-cli -m all       # All modules

# Output format (--format, default: human)
aura-cli --format human  # Human-readable lines
aura-cli --format json   # Structured JSON
aura-cli --format value  # key=value tokens

# Color mode (--color, default: ansi)
aura-cli --color ansi    # ANSI colors
aura-cli --color tmux    # Tmux compatible
aura-cli --color zellij  # Zellij compatible
aura-cli --color none    # No colors

# Custom shared memory path (-s/--shm-path)
# Must be an absolute path inside an existing euid-owned 0700 directory.
aura-cli --shm-path /run/user/1000/aura/state.dat
```

### Offline contract

When the daemon is absent, unreachable, or its data is stale (older than
2 seconds, or a read exceeds its wall-clock deadline), every module prints
exactly `[AURA: OFFLINE]` and the CLI exits with code 1. Corruption and
security failures are reported as `[AURA: ERROR - ...]`, never hidden as
offline. The CLI never panics.

### Daemon flags

```bash
aura-daemon                        # defaults
aura-daemon -i/--heartbeat-ms 500  # collection interval (default 500)
aura-daemon -s/--shm-path <path>   # override state path (validated)
aura-daemon -v/--verbose           # verbose logging
aura-daemon -f/--foreground        # stay in foreground
```

## Configuration

### Shared Memory Location

The default state lives in a private per-user runtime directory (mode
0700, file mode 0600, lock leaf `state.lock`):

| Platform | Default path |
|----------|--------------|
| Linux    | `/run/user/<uid>/aura/state.dat` (fallback: `/tmp/aura-<uid>/state.dat`) |
| macOS    | `/private/tmp/aura-<uid>/state.dat` |

Override on both binaries with `--shm-path`; the parent directory must
already exist and be euid-owned 0700.

### Heartbeat Interval

Default: 500ms

```bash
aura-daemon --heartbeat-ms 1000
```

## Installation

### Linux + systemd

AURA supports systemd deployment only as a per-user service; system-wide installation is unsupported.

```bash
# Build release
cargo build --release --workspace

# Install binaries
cp target/release/aura-daemon ~/.local/bin/
cp target/release/aura-cli ~/.local/bin/

# Install systemd service (Type=notify, WatchdogSec=3s, RuntimeDirectory=aura)
mkdir -p ~/.config/systemd/user/
cp deployment/systemd/aura-daemon.service ~/.config/systemd/user/

# Enable and start
systemctl --user daemon-reload
systemctl --user enable --now aura-daemon

# Check status
journalctl --user-unit aura-daemon -f
```

### macOS + LaunchAgent

```bash
# Build release
cargo build --release --workspace

# Install binaries
cp target/release/aura-daemon /usr/local/bin/
cp target/release/aura-cli /usr/local/bin/

# Install LaunchAgent (the daemon resolves its per-user state path itself)
cp deployment/macos/com.aura.daemon.plist ~/Library/LaunchAgents/

# Load service
launchctl load ~/Library/LaunchAgents/com.aura.daemon.plist

# View logs
log show --predicate 'process == "aura-daemon"' --last 1m
```

### Homebrew

Install the reviewed tap formula:

```bash
brew install zapsaang/tap/aura
```

Each release begins as a draft while its formula change is reviewed in the
tap. After the maintainer merges that PR, they publish it manually with
`gh release edit $TAG --draft=false`; only then can Homebrew download its
assets. Upgrade an installed copy with `brew update && brew upgrade aura`.

The formula is rendered from `deployment/homebrew/aura.rb.in` with the
verified release digests and audited on macOS; no source formula is tracked
in this repository.

### NixOS / Home Manager

```nix
# In configuration.nix or home.nix
services.aura = {
  enable = true;
  heartbeatMs = 500;      # optional, default 500
  shmPath = null;         # optional explicit state path override
};
```

The module builds from this repository, propagates the workspace version
from `Cargo.toml`, and enables the runtime-loaded NVML GPU feature on
Linux only.

## Platform Support

| Dimension | Linux | macOS |
|-----------|-------|-------|
| CPU, Process, Memory, Storage, Network, Meta | yes | capability-gated (public APIs only) |
| GPU | yes (runtime-loaded NVML `libnvidia-ml.so.1`, graceful when absent) | unsupported |

Unsupported capabilities are reported via capability bits and render as
`N/A`; nothing is faked.

## Project Structure

```
aura/
├── aura-common/       # Shared types: Archive (ABI v2), SeqLock, runtime paths
├── aura-daemon/       # Telemetry collector (producer)
│   └── src/collectors/  # cpu, process, memory, storage, network, meta, gpu
├── aura-cli/          # CLI consumer (presentation-only)
├── deployment/        # Service configurations
│   ├── systemd/       # Linux per-user systemd unit
│   ├── macos/         # macOS LaunchAgent plist
│   ├── homebrew/      # Homebrew formula template (CI-rendered)
│   └── home-manager/  # NixOS Home Manager module
├── scripts/           # Release/packaging/verification and PLOC checker
└── docs/              # Design docs, audit, ADR, PLOC rules
```

## Development

### Build

```bash
cargo build --workspace
```

### Test

```bash
cargo test --workspace
```

### Lint

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
python3 scripts/check-rust-loc.py --root .   # 250 PLOC ceiling per file
```

Every production Rust file is capped at 250 PLOC; see
[`docs/ploc.md`](docs/ploc.md) for the exact lexical/count/module rules.
Architectural decisions are locked in [`docs/adr.md`](docs/adr.md).

## License

MIT OR Apache-2.0

---

## Contributing

PRs welcome! Please run tests before submitting:

```bash
cargo test --workspace
cargo fmt --all
cargo clippy --all-targets --all-features -- -D warnings
```
