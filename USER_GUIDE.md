# AURA User Guide

**English** | [中文](#中文)

---

## Table of Contents

1. [Installation](#installation)
2. [Running](#running)
3. [CLI Reference](#cli-reference)
4. [Deployment](#deployment)
5. [Troubleshooting](#troubleshooting)
6. [Architecture](#architecture)

---

## Installation

### From Source

#### Prerequisites

- Rust 1.70+
- Linux (recommended) or macOS
- Build essentials (gcc, pkg-config, libssl-dev on Linux)

#### Build

```bash
# Clone repository
git clone https://github.com/zapsaang/aura.git
cd aura

# Build release
cargo build --release --workspace

# Verify build
./target/release/aura-cli --version
```

### Linux + systemd

#### Option 1: Manual Installation

```bash
# Create local bin directory
mkdir -p ~/.local/bin

# Copy binaries
cp target/release/aura-daemon ~/.local/bin/
cp target/release/aura-cli ~/.local/bin/

# Add to PATH (add to ~/.bashrc or ~/.zshrc)
export PATH="$HOME/.local/bin:$PATH"

# Install systemd service
mkdir -p ~/.config/systemd/user/
cp deployment/systemd/aura-daemon.service ~/.config/systemd/user/

# Reload systemd
systemctl --user daemon-reload

# Enable and start
systemctl --user enable --now aura-daemon

# Check status
systemctl --user status aura-daemon
journalctl --user-unit aura-daemon -f
```

#### Option 2: Homebrew (Linux)

```bash
# Requires Homebrew on Linux
brew tap zapsaang/tap
brew install zapsaang/tap/aura
brew services start aura-daemon
```

### macOS

#### Option 1: LaunchAgent

```bash
# Copy binaries to /usr/local/bin/
sudo cp target/release/aura-daemon /usr/local/bin/
sudo cp target/release/aura-cli /usr/local/bin/

# Create LaunchAgents directory
mkdir -p ~/Library/LaunchAgents

# Copy plist
cp deployment/macos/com.aura.daemon.plist ~/Library/LaunchAgents/

# Load service
launchctl load ~/Library/LaunchAgents/com.aura.daemon.plist

# View logs
log show --predicate 'process == "aura-daemon"' --last 1m
```

#### Option 2: Homebrew (Recommended)

```bash
brew tap zapsaang/tap
brew install zapsaang/tap/aura
brew services start aura-daemon
```

### NixOS / Home Manager

```nix
# In ~/.config/nixos/configuration.nix or ~/.home.nix
{ config, pkgs, ... }:

{
  services.aura = {
    enable = true;
    heartbeatMs = 500;
    shmPath = "/dev/shm/aura_state.dat";
  };
}
```

---

## Running

### Start Daemon

```bash
# Default settings
aura-daemon

# Custom settings
aura-daemon --heartbeat-ms 1000 --shm-path /tmp/aura.dat

# Foreground mode (for debugging)
aura-daemon --foreground

# Verbose logging
aura-daemon --verbose
```

### Query Data

```bash
# CPU usage
aura-cli -m cpu

# Memory usage
aura-cli -m mem

# Swap usage
aura-cli -m swap

# Disk I/O
aura-cli -m disk

# Network traffic
aura-cli -m net

# All modules
aura-cli -m all

# OS metadata only
aura-cli -m os
```

---

## CLI Reference

### Global Flags

| Flag | Description | Default |
|------|-------------|---------|
| `--shm-path <path>` | Shared memory file path | `/dev/shm/aura_state.dat` |
| `--version` | Show version | - |
| `--help` | Show help | - |

### Module Selection

| Module | Description |
|--------|-------------|
| `-m cpu` | CPU usage (global + per-core) |
| `-m mem` | Memory usage (RAM + swap) |
| `-m swap` | Swap only |
| `-m disk` | Disk I/O + mount statistics |
| `-m net` | Network interface statistics |
| `-m os` | OS metadata (uptime, load, timezone) |
| `-m all` | All modules |

### Output Options

#### Format

```bash
# Human-readable (default)
aura-cli -m cpu

# JSON (for scripting/dashboards)
aura-cli -m all --format json
```

#### Color Mode

```bash
# ANSI colors (default on terminals)
aura-cli -m cpu --color ansi

# Tmux mode (Tmux pane titles)
aura-cli -m cpu --color tmux

# Zellij mode
aura-cli -m cpu --color zellij

# No colors
aura-cli -m cpu --color none
```

### Threshold Colors

| Metric | Red | Yellow | Magenta | Green |
|--------|-----|--------|---------|-------|
| CPU | >80% | >70% | >60% | ≤60% |
| Memory | >80% | >70% | >60% | ≤60% |
| GPU Temp | ≥80°C | ≥70°C | ≥60°C | <60°C |

---

## Deployment

### Shared Memory Configuration

The daemon writes to shared memory, the CLI reads from it. Both must use the same path.

```bash
# Default (Linux)
aura-daemon --shm-path /dev/shm/aura_state.dat

# Custom location
aura-daemon --shm-path /tmp/aura_state.dat
aura-cli --shm-path /tmp/aura_state.dat
```

### systemd Service Management

```bash
# Start
systemctl --user start aura-daemon

# Stop
systemctl --user stop aura-daemon

# Restart
systemctl --user restart aura-daemon

# Status
systemctl --user status aura-daemon

# View logs
journalctl --user-unit aura-daemon -f

# Logs since last boot
journalctl --user-unit aura-daemon -b
```

### LaunchAgent Service Management (macOS)

```bash
# Start
launchctl start com.aura.daemon

# Stop
launchctl stop com.aura.daemon

# Unload
launchctl unload ~/Library/LaunchAgents/com.aura.daemon.plist

# Reload
launchctl unload ~/Library/LaunchAgents/com.aura.daemon.plist
launchctl load ~/Library/LaunchAgents/com.aura.daemon.plist
```

### Homebrew Service Management

```bash
# Start
brew services start aura-daemon

# Stop
brew services stop aura-daemon

# Restart
brew services restart aura-daemon

# Status
brew services list

# View logs
cat /tmp/aura-daemon.log
```

### Upgrading (Homebrew)

New releases land in the `zapsaang/tap` formula only after the release
maintainer merges the tap pull request and publishes the GitHub release.
To pick up a new version:

```bash
# Refresh the tap and upgrade
brew update
brew upgrade aura

# Restart the daemon so the running service matches the new binaries
brew services restart aura-daemon

# Verify
aura-cli --version
```

Notes:

- `brew update` only helps after the maintainer has merged the formula PR
  and flipped the release from draft to published. Until then the old
  formula (or no formula) is what you see; this is intentional.
- Once a release is published it is immutable. Rerunning the release
  workflow for the same tag will not change a published release.
- The daemon and CLI ship in the same formula, so upgrade both together.
  After upgrading, restart `aura-daemon`; a stale daemon with a new CLI is
  a version-skewed deployment.

---

## Troubleshooting

### Daemon Issues

#### Daemon won't start

```bash
# Check if another daemon is running
pgrep aura-daemon

# Check shared memory permissions
ls -la /dev/shm/aura_state.dat

# Check systemd logs
journalctl --user-unit aura-daemon -xe
```

#### High CPU usage from daemon

The 500ms heartbeat is CPU-intensive. Adjust interval:

```bash
# Less frequent (1000ms = 1s)
aura-daemon --heartbeat-ms 1000
```

### CLI Issues

#### "[AURA: OFFLINE]"

Daemon is not running or shared memory is stale.

```bash
# Solution 1: Start daemon
aura-daemon &

# Solution 2: Wait for data to refresh (2s timeout)
aura-cli -m cpu
```

#### Wrong data or zeros

Shared memory file may be corrupted.

```bash
# Stop daemon
systemctl --user stop aura-daemon  # Linux
# or
launchctl stop com.aura.daemon      # macOS

# Remove shared memory
rm /dev/shm/aura_state.dat

# Restart daemon
systemctl --user start aura-daemon
```

#### Permission denied

```bash
# Check user permissions for /dev/shm
ls -la /dev/shm | grep aura

# Fix permissions (Linux)
sudo chown $USER /dev/shm/aura_state.dat

# Or use alternative path
aura-daemon --shm-path /tmp/aura_state.dat
aura-cli --shm-path /tmp/aura_state.dat
```

### Performance Issues

#### CLI response is slow

- Ensure daemon is running
- Check network latency to shared memory (if using NFS/network mounts)
- Use local `/dev/shm` instead of network storage

#### Daemon using too much CPU

- Increase heartbeat interval (default 500ms)
- Reduce number of collectors (edit `collectors/mod.rs`)

---

## Architecture

### Data Flow

```
┌─────────────────────────────────────────────────────────┐
│                      aura-daemon                         │
│  ┌─────────────┐    ┌──────────────┐    ┌───────────┐  │
│  │  Collectors │───→│  Telemetry   │───→│  SeqLock  │  │
│  │  (500ms)    │    │   Archive    │    │  Writer   │  │
│  └─────────────┘    └──────────────┘    └─────┬─────┘  │
└──────────────────────────────────────────────┼─────────┘
                                               │
                                    /dev/shm/aura_state.dat
                                               │
┌──────────────────────────────────────────────┼─────────┐
│                       aura-cli               │          │
│                              ┌─────┴─────┐   │          │
│  ┌─────────────┐    ┌────────▼──────┐  │  │          │
│  │  SeqLock    │───→│   Telemetry   │──┘  │          │
│  │  Reader     │    │    Archive     │     │          │
│  └─────────────┘    └───────────────┘     │          │
│  ┌─────────────┐    ┌───────────────┐     │          │
│  │  Renderer   │───→│    Output     │     │          │
│  └─────────────┘    └───────────────┘     │          │
└─────────────────────────────────────────────────────────┘
```

### SeqLock Protocol

1. Writer increments version to odd (begin write)
2. Writer copies data to shared memory
3. Writer increments version to even (end write)
4. Reader reads version (if odd, spin wait)
5. Reader copies data
6. Reader re-reads version (if changed, retry)

### Telemetry Data

| Module | Data Source | Key Metrics |
|--------|------------|-------------|
| CPU | /proc/stat | usage%, context switches/s |
| Memory | /proc/meminfo | total, free, buffers, cached |
| Process | /proc/[pid]/stat | top 5 CPU, top 5 memory |
| Disk | /proc/diskstats | read/write bytes/s |
| Network | /proc/net/dev | rx/tx bytes/s |
| Meta | /proc/loadavg, /etc/os-release | uptime, OS info |
| GPU | NVML | name, memory, utilization, temp |

---

## 中文

## 目录

1. [安装](#安装)
2. [运行](#运行)
3. [CLI 参考](#cli-参考)
4. [部署](#部署)
5. [故障排除](#故障排除)
6. [架构](#架构)

---

## 安装

### 从源码构建

```bash
git clone https://github.com/zapsaang/aura.git
cd aura
cargo build --release --workspace
```

### Linux + systemd

```bash
# 安装二进制文件
mkdir -p ~/.local/bin
cp target/release/aura-daemon ~/.local/bin/
cp target/release/aura-cli ~/.local/bin/

# 安装 systemd 服务
mkdir -p ~/.config/systemd/user/
cp deployment/systemd/aura-daemon.service ~/.config/systemd/user/

# 启用并启动
systemctl --user daemon-reload
systemctl --user enable --now aura-daemon
```

### macOS + LaunchAgent

```bash
# 安装二进制文件
sudo cp target/release/aura-daemon /usr/local/bin/
sudo cp target/release/aura-cli /usr/local/bin/

# 安装 LaunchAgent
cp deployment/macos/com.aura.daemon.plist ~/Library/LaunchAgents/
launchctl load ~/Library/LaunchAgents/com.aura.daemon.plist
```

### Homebrew

```bash
brew tap zapsaang/tap
brew install zapsaang/tap/aura
brew services start aura-daemon
```

升级：新版本在维护者合并 tap PR 并发布 release 后可用，执行
`brew update && brew upgrade aura`，然后 `brew services restart aura-daemon`。
已发布的 release 不可变，同 tag 重跑发布流程不会改动已发布内容。

### NixOS

```nix
services.aura.enable = true;
```

---

## 运行

```bash
# 启动 daemon
aura-daemon

# 查询数据
aura-cli -m cpu      # CPU
aura-cli -m mem      # 内存
aura-cli -m all      # 全部
```

---

## CLI 参考

```bash
# 模块选择
aura-cli -m cpu      # CPU 使用率
aura-cli -m mem      # 内存使用率
aura-cli -m disk     # 磁盘 I/O
aura-cli -m net      # 网络流量
aura-cli -m all      # 全部模块

# 输出格式
aura-cli --format json   # JSON 格式

# 颜色模式
aura-cli --color ansi     # ANSI 颜色
aura-cli --color tmux     # Tmux 兼容
aura-cli --color none    # 无颜色

# 自定义共享内存路径
aura-cli --shm-path /tmp/aura_state.dat
```

---

## 故障排除

### "[AURA: OFFLINE]"

Daemon 未运行。

```bash
# 启动 daemon
aura-daemon &

# 然后再试
aura-cli -m cpu
```

### 权限错误

```bash
# Linux 检查 /dev/shm 权限
ls -la /dev/shm/aura_state.dat

# 或使用自定义路径
aura-daemon --shm-path /tmp/aura_state.dat
aura-cli --shm-path /tmp/aura_state.dat
```

---

## 架构

```
┌──────────────┐      SeqLock      ┌──────────────┐
│ aura-daemon  │ ────────────────→│   aura-cli    │
│  (生产者)     │   TelemetryArchive  │   (消费者)     │
│ 每500ms采集    │                  │   按需读取      │
└──────────────┘                  └──────────────┘
```
