# AURA - Asynchronous Ultra-low-overhead Resource Agent


---

## Overview

AURA is a nanosecond-level system telemetry probe designed for geeks and low-level developers. It uses a C/S architecture with shared memory IPC (SeqLock + bytemuck zero-copy serialization) for zero system call data retrieval.

### Key Features

- **Zero-copy IPC**: Shared memory communication via `/dev/shm` with SeqLock protocol
- **Zero-allocation collectors**: Fixed-size arrays, no heap allocation in hot paths
- **Sub-millisecond overhead**: Entire CLI lifecycle completes in microseconds
- **Cross-platform**: Linux (/proc) and macOS (Mach ports) support
- **Rich telemetry**: CPU, Memory, Network, GPU, Meta data

### Architecture

```
┌─────────────────┐      /dev/shm/       ┌─────────────────┐
│   aura-daemon   │ ──── SeqLock ────→ │    aura-cli      │
│   (producer)    │  TelemetryArchive   │   (consumer)     │
│  500ms heartbeat│                     │   µs response    │
└─────────────────┘                     └─────────────────┘
```

## Quick Start

### Prerequisites

- Rust 1.70+
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
# Module selection
aura-cli -m cpu      # CPU usage
aura-cli -m mem      # Memory usage
aura-cli -m net      # Network traffic
aura-cli -m all      # All modules

# Output format
aura-cli --format json   # JSON output

# Color mode
aura-cli --color ansi    # ANSI colors (default)
aura-cli --color tmux    # Tmux compatible
aura-cli --color none    # No colors

# Custom shared memory path
aura-cli --shm-path /tmp/aura_state.dat
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

# Install systemd service
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

# Install LaunchAgent
cp deployment/macos/com.aura.daemon.plist ~/Library/LaunchAgents/

# Load service
launchctl load ~/Library/LaunchAgents/com.aura.daemon.plist

# View logs
log show --predicate 'process == "aura-daemon"' --last 1m
```

### Homebrew (macOS)

```bash
# Add tap
brew tap zapsaang/tap

# Install (builds from source)
brew install zapsaang/tap/aura

# Start service
brew services start aura-daemon

# Query
aura-cli -m cpu
```

### NixOS / Home Manager

```nix
# In configuration.nix or home.nix
services.aura = {
  enable = true;
  heartbeatMs = 500;
};
```

## Configuration

### Shared Memory Path

Default: `/dev/shm/aura_state.dat`

Override via CLI:
```bash
aura-daemon --shm-path /tmp/aura_state.dat
aura-cli --shm-path /tmp/aura_state.dat
```

### Heartbeat Interval

Default: 500ms

```bash
aura-daemon --heartbeat-ms 1000
```

## Project Structure

```
aura/
├── aura-common/       # Shared types: Archive, SeqLock, Error
├── aura-daemon/      # Telemetry collector (producer)
├── aura-cli/         # CLI consumer
├── deployment/        # Service configurations
│   ├── systemd/      # Linux systemd unit
│   ├── macos/        # macOS LaunchAgent plist
│   ├── homebrew/     # Homebrew formulas
│   └── home-manager/ # NixOS Home Manager module
└── docs/             # Project documentation
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
```

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
