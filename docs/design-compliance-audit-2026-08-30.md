# AURA 设计合规审计报告

**审计日期**: 2026-08-30
**审计对象**: `zapsaang/aura` 当前工作树，HEAD `2ac76c0`（main 分支），dirty worktree
**审计人员**: Sisyphus
**基准文档**: `docs/project.md`、`docs/tech_blueprint.md`、`README.md`
**输出说明**: 本报告仅记录当前工作树状态，不做任何代码修改或提交。

---

## 1. 审计范围与方法

### 1.1 范围

对照 `docs/project.md`、`docs/tech_blueprint.md` 与 `README.md` 中的设计承诺，逐项核对当前实现：

* CPU、Process、Memory、Storage、Network、Meta、GPU 七大数据维度的字段完整性与语义正确性
* Watchdog 保活机制
* IPC / SeqLock / double buffer 协议
* CLI 路由、颜色模式、JSON、离线降级
* systemd / LaunchAgent / Home Manager / Homebrew 部署配置
* 测试覆盖与 CI 构建矩阵
* 安全与运维风险

### 1.2 方法

1. 静态走查：读取源代码与部署配置，记录精确到 `file:line` 的证据。
2. 自动检查：
   * `cargo fmt --all -- --check`
   * `cargo clippy --all-targets --all-features -- -D warnings`
   * `cargo test --workspace`
3. 运行时验证：启动 `aura-daemon`，用 `aura-cli` 采集 JSON / 人工输出，验证 `help`、离线降级、缺失 SHM 报错、进程 / 存储字段行为。
4. 历史核对：检查 `git log`、`git status` 与关键提交 `4c419fc`、`59d33e7`、`87e5e74`、`2ac76c0`。

### 1.3 严重性定义

| 级别 | 定义 |
|------|------|
| **CRITICAL** | 数据损坏、系统危险、核心功能不可用，或在生产环境中可能导致宕机 / 重启 / 安全事件。 |
| **HIGH** | 主要维度缺失或误导、严重正确性缺陷，影响用户决策或产品核心卖点。 |
| **MEDIUM** | 部分平台 / 部署 / 安全行为不完整，存在明显风险但短期可规避。 |
| **LOW** | 字段 / 文档 / 测试不匹配，不影响主流程但需补齐。 |

---

## 2. 执行摘要

**总体裁决：FAIL**

当前实现完成了部分 Linux 核心采集与 IPC 框架，但与设计白皮书承诺存在显著差距。Process、Storage 维度在代码中仍占有 `TelemetryArchive` 位置，却在运行时始终输出零值或完全缺失；CLI 仍在进行设计文档明确禁止的浮点计算；systemd 软心跳（`WATCHDOG=1`）被替换为直接操作 `/dev/watchdog` 的硬件喂狗，存在触发主机重启的风险；macOS 平台多处维度为空或部分为零；SeqLock 读者重试次数、超时分类与弱内存模型边界尚未达到蓝图要求。测试全部通过，但覆盖缺口严重。

---

## 3. 五通道审查裁决

| # | 审查通道 | 代理类型 | 裁决 | 置信度 / 说明 |
|---|----------|----------|------|---------------|
| 1 | Goal & Constraint Verification | Oracle | **FAIL** | 多个设计约束未满足，包括 daemon-only 计算、硬件 watchdog、完整 JSON、Process / Storage 维度。 |
| 2 | Runtime QA Execution | unspecified-high | **FAIL（6 passed / 3 failed）** | 3 个阻塞性运行时失败：CLI `help` 缺少 `disk` module、JSON `process` 维度全为零、JSON 无 `storage` 键。此外观察到 missing SHM 被分类为 `[AURA: ERROR - ...]` 而非 `[AURA: OFFLINE]`，属于合同/降级语义不一致，但不计入本次 3 个阻塞失败。 |
| 3 | Code Quality Review | Oracle | **FAIL** | 存在越界访问隐患、部分平台 stub 未标记、错误分类与重试策略不一致、CLI 渲染层职责倒置。 |
| 4 | Security Review | Oracle | **FAIL** | SHM 0o666 权限、checksum 无认证、lockfile 缺少 `O_NOFOLLOW` / owner / mode 校验，存在本地 DoS / 欺骗风险。 |
| 5 | Context / History Mining | unspecified-high | **PASS** | 已确认 4c419fc  intentional 移除 disk/process，59d33e7 与 87e5e74 有意替代旧 SeqLock 设计；`.gitignore` 导致 docs 未跟踪；无 RFC/ADR。 |

---

## 4. 需求合规矩阵

| 维度 | 设计基线要求（来源） | 当前状态 | 结论 |
|------|---------------------|----------|------|
| CPU | 全局与每核利用率、上下文切换频率；daemon 计算差值（`tech_blueprint.md` 3） | Linux 全局 CPU 的周期差值逻辑正确，但每核 `usage_percent` 使用累计 tick，表达的是开机以来的平均值。由于全局 CPU、上下文切换和其他速率指标的上一周期基线初始为零，首次发布还会出现基线尖峰。 | **PARTIAL** |
| Process | Top 5 CPU / 内存进程，PID、comm、百分比、内存字节；直接解析 `/proc/[pid]/stat`（`project.md` 2） | `ProcessStats` 仍存在于 `TelemetryArchive`，但 `collect_all` 不再调用 process collector；运行时 `process.total` 为 0，`top_cpu/top_mem` 全零。 | **FAIL** |
| Memory | total/free/used、buffers/cache、swap、缺页中断率（`project.md` 2 / `tech_blueprint.md` 3） | Linux 完整实现并测试。macOS `buffers=0`、`swap_total/free/used=0`、`page_faults_per_sec=0`，无 availability 标记。 | **PARTIAL** |
| Storage | 每块设备 rx/wx 速率、IOPS、队列深度、延迟；每个挂载点 size/available/percent/type/mount（`project.md` 2） | `StorageStats` 保留在 ABI 中，但 collector 已删除；JSON 不输出 storage；CLI 无 `disk` module；`DiskStat` 无 IOPS / 队列 / 延迟字段。 | **FAIL** |
| Network | 每接口 rx/tx 字节与速率；skip lo/docker0/veth（`project.md` 2 / `tech_blueprint.md` 3） | Linux 实现正确并测试。macOS `collectors/network/macos.rs` 直接返回 `if_count=0`，无任何接口。 | **PARTIAL** |
| Meta | 系统指纹 type/id/version/versionId/prettyName/versionCodename；绝对时间戳、uptime、时区（`project.md` 2） | Linux 解析 `/etc/os-release` 但缺少 `version` 与 `versionCodename`；`timestamp_ns` 使用 `CLOCK_BOOTTIME`（monotonic），不是绝对时间。macOS 仅填充部分字段。 | **PARTIAL** |
| GPU | NVML 原生调用，name、显存、利用率、功耗、温度（`project.md` 2） | 实现位于 `gpu-nvml` feature 后；release CI 与 Homebrew 均未启用该 feature；macOS 无 GPU 路径。 | **PARTIAL** |
| Watchdog | systemd `WATCHDOG=1`，`WatchdogSec`，daemon 通过 init 系统保活（`project.md` 2 / `tech_blueprint.md` 1） | 改为直接打开 `/dev/watchdog` 并写入 `0u8`；无 `WatchdogSec`；即使 collector 失败也会喂狗。 | **FAIL** |
| IPC / SeqLock | 10ms 自旋上限、checksum、离线判定；正确 SeqLock 协议（`tech_blueprint.md` 1 / 4） | double buffer + per-buffer seq 已实现；但 reader 仅重试 3 次且没有时间预算；`SeqLockInvalid` / `ChecksumMismatch` / `SharedMemory` 分类与蓝图不一致；存在 active_index 排序与 odd-seq 重启恢复的弱内存疑虑（中等置信度）。 | **PARTIAL** |
| Deployment | systemd 用户服务、LaunchAgent、Home Manager、Homebrew 一致默认 SHM 路径 | Linux systemd 使用 `%t/aura_state.dat`，Home Manager 使用 `$XDG_RUNTIME_DIR/aura_state.dat`，均与常量 `/dev/shm/aura_state.dat` 不一致；Homebrew / LaunchAgent 路径未在本次审计中逐行验证。 | **FAIL** |
| CLI 路由 / 颜色 / JSON | `--module cpu|mem|swap|disk|net|os`；颜色引擎；JSON 全量；离线降级（`project.md` 5） | CLI 无 `disk` module；JSON 缺 storage；process 全零；三种颜色模式 `ansi/tmux/zellij` 行为完全相同；CLI 侧仍在计算百分比与求和。 | **FAIL** |
| Tests | 单元 / 集成 / 并发覆盖，含新维度（`tech_blueprint.md` 及 README） | 全部通过，但 Process/Storage 测试随 4c419fc 删除；缺少 macOS pipeline、GPU release、watchdog、部署路径、离线错误分类、`>MAX_NETIFS`、JSON 完整性测试。 | **PARTIAL** |

---

## 5. 详细问题目录

### AUD-001 CRITICAL：硬件 watchdog 直接操作 `/dev/watchdog`，缺少 `WatchdogSec`，失败仍喂狗

* **严重性**: CRITICAL
* **分类**: Operational / Safety
* **影响**: 任何导致 collector 失败但主循环仍运行的故障，都会持续喂狗。若系统实际已不健康但进程未死，硬件 watchdog 不会触发预期的主机重启，反而掩盖故障；反之，若 daemon 崩溃前未正确 magic-close，某些驱动可能在超时后重启机器。
* **证据**:
  * `aura-daemon/src/heartbeat.rs:21-41`：`watchdog::init` 直接 `OpenOptions::new().write(true).open("/dev/watchdog")`，未配置超时。
  * `aura-daemon/src/heartbeat.rs:36-41`：`pet()` 无条件写入 `&[0u8]`。
  * `aura-daemon/src/heartbeat.rs:75-86`：即使 `collectors::collect_all` 返回 `Err`，记录日志后仍执行 `shm.write` 与 `watchdog::pet()`。
  * `deployment/systemd/aura-daemon.service:1-18`：单元文件未设置 `WatchdogSec`，未使用 `NotifyAccess` 或 `WATCHDOG=1` 通知机制。
* **建议**:
  1. 恢复 systemd `Type=notify` + `WATCHDOG=1` 软心跳，设置 `WatchdogSec=3`。
  2. 仅在完整采集与写入成功后喂狗；collector 连续失败达到阈值时停止喂狗并退出。
  3. 若保留 `/dev/watchdog` 作为 fallback，需通过 `ioctl(WDIOC_SETTIMEOUT)` 配置超时，并在崩溃路径保证 magic-close 或 disable。

### AUD-002 CRITICAL：网络接口数超过 `MAX_NETIFS` 后，第二次循环触发越界索引 panic/abort

* **严重性**: CRITICAL
* **分类**: Bounds / Availability
* **影响**: 当 `/proc/net/dev` 中实际接口数超过 `16` 个时，首次循环打印警告并 `break`；由于 `NETIF_LIMIT_WARNED` 在首次触发后已设置，后续循环中 `count >= MAX_NETIFS` 的防御分支不再进入，循环继续递增 `count`。当 `count` 达到 `MAX_NETIFS` 后再次尝试索引 `interfaces_out[count]`，Rust 会进行 bounds check，越界时触发 panic；release profile 设置 `panic = "abort"`，因此守护进程会直接 abort。这导致接口数超过上限的系统上 daemon 不可用，而非只损失部分接口可见性。
* **证据**:
  * `aura-daemon/src/collectors/network/linux.rs:33-42`：仅在 `NETIF_LIMIT_WARNED.get().is_none()` 时执行 `break`，警告状态一旦设置便不再拦截。
  * `aura-daemon/src/collectors/network/linux.rs:65-72`：`interfaces_out[count]` 与 `count += 1` 在 `count` 超过 `MAX_NETIFS` 时产生越界索引；Rust 数组索引自带 bounds check，越界即 panic。
  * `Cargo.toml:48`：`[profile.release] panic = "abort"`。
* **建议**: 将上限检查改为无条件的 `if count >= MAX_NETIFS { break; }`，警告逻辑独立；或持续跳过多余接口而不写入。

### AUD-003 HIGH：Process 与 Storage 已 intentionally 移除，但 ABI / JSON / 文档残留，功能缺失

* **严重性**: HIGH
* **分类**: Compliance / Functional
* **影响**: 产品白皮书明确承诺 Process Top 5、Storage IOPS/队列/延迟/挂载点。commit `4c419fc` 已删除对应 collector 与 CLI 输出，但 `TelemetryArchive` 仍占用空间，JSON 与 CLI 不输出 storage，process 全零，形成“僵尸字段”。用户拿到的数据与文档严重不符。
* **证据**:
  * `git show 4c419fc --stat`：删除 `aura-daemon/src/collectors/disk/*`、`aura-daemon/src/collectors/process.rs`、`aura-cli/src/output/disk.rs` 等。
  * `aura-common/src/archive.rs:128-135`、`177-184`：`ProcessStats`、`StorageStats` 仍在 ABI 中。
  * `aura-daemon/src/collectors/mod.rs:113-135`：`collect_all` 不再调用 process 或 storage collector。
  * `aura-cli/src/format/json.rs:7-21`：`TelemetryJson` 无 `storage` 字段；`process_to_json` 在 `json.rs:180-189` 仍序列化全零数组。
  * `aura-cli/src/output/mod.rs:14-21`：`RENDERERS` 只有 6 项，无 disk；`Module` 枚举无 `Disk`（`aura-cli/src/main.rs:29-36`）。
  * `aura-cli/src/main.rs --help` 输出：可选 module 为 `cpu, mem, swap, net, all, os`，无 `disk`。
* **建议**:
  1. 若功能确定移除，应同步清理 ABI（移除 `ProcessStats` / `StorageStats` 或标记 deprecated）并更新 `docs/project.md`、`README.md`。
  2. 若计划恢复，需按白皮书实现无堆分配 process collector 与 storage collector，并补全 CLI / JSON 路由。

### AUD-004 HIGH：部分采集失败时仍更新时间戳并发布为“新鲜”数据

* **严重性**: HIGH
* **分类**: Correctness / Reliability
* **影响**: `collect_all` 中任一 collector 失败仅记录日志，随后 `timestamp_ns` 被更新为当前时间并写入 SHM。CLI 的 `is_fresh` 会因此认为数据新鲜，展示过时或零值。
* **证据**:
  * `aura-daemon/src/heartbeat.rs:75-79`：`collect_all` 出错后继续执行，`collector_state.telemetry.meta.timestamp_ns = aura_common::monotonic_ns();`
  * `aura-daemon/src/collectors/mod.rs:96-139`：`collect_all` 各 collector 返回 `AuraResult`，但 `collect_meta_and_gpu` 与 `prev_timestamp_ns` 更新在错误未拦截时仍发生。
* **建议**: 将 collector 失败视为整体周期失败，不更新 `timestamp_ns`，不执行 `shm.write`；连续失败若干周期后退出或进入 offline 状态。

### AUD-005 HIGH：JSON 输出缺 storage 且 process 字段永远为零

* **严重性**: HIGH
* **分类**: API Completeness
* **影响**: `aura-cli --format json` 是“上帝视角”全量输出，但缺少 storage；process 维度全零，无法用于 dashboard。
* **证据**:
  * `aura-cli/src/format/json.rs:7-21`：`TelemetryJson` 字段仅 `version/cpu/process/memory/network/meta/gpu`。
  * 运行时 `aura-cli --format json -m all` 输出：`process.total=0`、`top_cpu/top_mem` 全为 `pid:0, cpu_usage:0.0, memory_bytes:0, comm:""`；无 `storage` 键。
* **建议**: 恢复 storage collector 后添加 `storage_to_json`；process 全零时应标记 `process` 为 `null` 或移除，避免误导。

### AUD-006 HIGH：release artifact 默认关闭 `gpu-nvml`，macOS 无 GPU 路径

* **严重性**: HIGH
* **分类**: Platform / Build
* **影响**: GPU 功能被设计为可选 feature，但 release CI 与 Homebrew formula 均未启用 `--features gpu-nvml`，因此所有 release 二进制都不支持 NVIDIA GPU。macOS 平台完全没有 GPU collector 实现。
* **证据**:
  * `.github/workflows/release.yml:48-54`：`cargo build --release --target ... --workspace`，无 `--features gpu-nvml`。
  * `deployment/homebrew/aura.rb:11`：`cargo build --release --workspace` 同样未启用 feature。
  * `deployment/homebrew/aura-daemon.rb:13`：`cargo build --release --package aura-daemon` 未启用 feature。
  * `aura-daemon/src/collectors/gpu.rs:92-105`：未启用 feature 时 `init_nvml` / `collect_nvml` 为空操作。
  * macOS 平台 `collect_meta_and_gpu` 仅处理 meta 与 uptime（`aura-daemon/src/collectors/mod.rs:148-154`），无 GPU 分支。
* **建议**: release 构建启用 `gpu-nvml`；或提供独立 GPU-enabled artifact；macOS 至少暴露 `nvml_available=false` 并记录 unsupported。

### AUD-007 HIGH：每核 CPU `usage_percent` 使用累计 tick 而非区间差值，首样本异常

* **严重性**: HIGH
* **分类**: Correctness
* **影响**: 每核 `usage_percent` 使用累计 tick 计算，语义为“自开机以来的生命周期平均利用率”，不是 `tech_blueprint.md` 要求的区间差值。这与全局 CPU 不一致。另外，`CollectorState` 的 baselines（`prev_cpu_ticks`、`prev_net_bytes`、`prev_page_faults`）初始为零，且 `collectors::init` 仅设置 `prev_timestamp_ns`（`aura-daemon/src/collectors/mod.rs:92`）。因此在第一个采集周期，全局 CPU、上下文切换率、缺页中断率、网络速率都使用“当前累计值 - 0”作为差值，导致首次输出出现基于 lifetime counters 的基线尖峰；后续周期才回归正常区间差值。
* **证据**:
  * `aura-daemon/src/collectors/cpu/linux.rs:101-114`：每核 `usage_percent = ((total - idle) / total) * 100`，使用累计 tick，未做区间差值。
  * `aura-daemon/src/collectors/cpu/linux.rs:164-169`：全局 `usage_percent` 使用 `delta_total` / `delta_idle`，与每核语义不同。
  * `aura-daemon/src/collectors/mod.rs:49-69`：`CollectorState::new` 将 `prev_cpu_ticks`、`prev_net_bytes`、`prev_page_faults` 初始化为零；`init` 仅设置 `prev_timestamp_ns`。
  * `aura-daemon/src/collectors/mod.rs:96-118`：首次 `collect_all` 时 `prev_timestamp_ns != 0` 且 `delta_secs` 由实际间隔决定，但 `prev_cpu_ticks.total` 等仍为零，故全局速率首次为 lifetime 累计值。
* **建议**: 每核 collector 保存上一周期 tick，按区间差值计算 `usage_percent`；或在 `init` 时执行一次无发布的预采集以建立真实基线，消除首次输出的 lifetime spike。

### AUD-008 MEDIUM：macOS network 为空；memory / meta / context-switch 部分字段为零且无 availability 标记

* **严重性**: MEDIUM
* **分类**: Platform Completeness
* **影响**: macOS 平台 network 直接返回空；memory 中 `buffers`、`swap_*`、`page_faults_per_sec` 为 0；meta 无 loadavg、timezone；context switches 维度未采集。用户无法区分“真的为零”与“未实现”。
* **证据**:
  * `aura-daemon/src/collectors/network/macos.rs:5-14`：`out.if_count = 0; prev.count = 0;`。
  * `aura-daemon/src/platform/macos.rs:494-506`：`MemoryStats` 中 `buffers: 0`、`swap_total/free/used: 0`、`page_faults_per_sec: 0.0`。
  * `aura-daemon/src/collectors/mod.rs:148-154`：macOS 仅更新 `timestamp_ns` 与 `uptime_secs`。
  * `aura-daemon/src/platform/macos.rs:717-752`：`cache_os_fingerprint` 不填充 `versionCodename`，也不填充 `load_avg` / timezone。
* **建议**: 为各维度添加 `available` 或 `supported` 标志；在文档中明确列出 macOS 未实现项，或在 CLI 输出中显示 “N/A”。

### AUD-009 MEDIUM：SeqLock reader 仅重试 3 次且无时间预算；错误分类与弱内存边界未满足蓝图

* **严重性**: MEDIUM
* **分类**: Concurrency / Protocol
* **影响**: `tech_blueprint.md` 要求“10ms 内自旋未获锁”触发 offline 降级，但实现为固定 3 次重试（`double_buffer.rs:138`），没有 wall-clock 预算，也未区分 timeout、checksum mismatch 与 missing SHM。write protocol 的 entry ordering 与 odd-seq 重启恢复也存在弱内存疑虑（中等置信度）。
* **证据**:
  * `aura-common/src/double_buffer.rs:138-175`：`for _ in 0..3` 重试，未使用 `MAX_SPIN_WAIT_MS=100`（`aura-common/src/consts.rs:26`）。
  * `aura-cli/src/reader.rs:51-56`：`read_double_buffer` 失败统一映射为 `AuraError::SeqLockInvalid`。
  * `aura-cli/src/reader.rs:17-27`：SHM 不存在时非权限拒绝映射为 `AuraError::SharedMemory(std::io::Error)`，最终在 `main.rs:87-90` 输出 `[AURA: ERROR - ...]`，而非 `[AURA: OFFLINE]`。
  * `aura-common/src/double_buffer.rs:86-111`：writer 先 `fetch_add(1)` 使 seq 变奇，copy，再 `fetch_add(1)` 使 seq 变偶，最后 `active_index.store`。reader 先读 `active_index`，再读 `seq[active]`。现有测试（`read_returns_err_when_seq_is_odd`、`ipc_concurrent_reader_writer_stress` 等）仅覆盖普通 odd-seq 与并发行为，未覆盖 daemon 异常重启后的 header 恢复或弱内存边界。在 daemon 异常重启后，active 指针可能指向一个 odd seq 的 buffer，reader 重试 3 次后失败。该弱内存 / 崩溃恢复场景为理论边界，保持中等置信度标注。
* **建议**:
  1. 按蓝图实现基于 `MAX_SPIN_WAIT_MS` 的 wall-clock 重试，并在超时后返回专用错误类型。
  2. 将 `ChecksumMismatch`、`SeqLockInvalid`、missing SHM 分类输出为不同用户提示。
  3. 在 daemon 启动时清理 / 验证 SHM header，确保 active_index 指向 even seq buffer；或记录 per-buffer seq 恢复策略。

### AUD-010 MEDIUM：daemon 与 CLI 默认 SHM 路径在 systemd 与 Home Manager 中不一致

* **严重性**: MEDIUM
* **分类**: Deployment
* **影响**: 常量 `SHM_PATH` 在 Linux 为 `/dev/shm/aura_state.dat`，但 systemd 用户单元使用 `%t/aura_state.dat`（即 `/run/user/<uid>/aura_state.dat`），Home Manager 默认使用 `$XDG_RUNTIME_DIR/aura_state.dat`。用户若按 README 直接运行 `aura-cli`（无 `--shm-path`）会读取 `/dev/shm/aura_state.dat`，而 systemd 启动的 daemon 写入 run 目录，导致 CLI 报 `[AURA: ERROR - No such file]` 而非离线降级。
* **证据**:
  * `aura-common/src/consts.rs:1-5`：`#[cfg(target_os="linux")] pub const SHM_PATH = "/dev/shm/aura_state.dat";`。
  * `deployment/systemd/aura-daemon.service:9`：`Environment=AURA_SHM_PATH=%t/aura_state.dat`。
  * `deployment/home-manager/default.nix:22`：`shmPath` 默认 `${config.xdg.runtimeDir}/aura_state.dat`。
* **建议**: 统一默认路径；至少让 systemd 与 Home Manager 默认使用 `/dev/shm/aura_state.dat`，或在 CLI 默认中同步说明。

### AUD-011 MEDIUM：CLI 侧仍计算百分比 / 求和 / 颜色策略，违反 daemon-only 计算设计

* **严重性**: MEDIUM
* **分类**: Architecture
* **影响**: `tech_blueprint.md` 明确“任何耗费 CPU 周期的浮点数运算、类型转换、状态判断，必须全部收敛在后台守护进程中完成”。但 CLI 在 `output/mem.rs`、`output/value.rs` 中重复计算 RAM/Swap/Net 百分比与求和；`output/ansi.rs` 中 `ColorMode::Ansi`、`Tmux`、`Zellij` 三种模式行为完全相同，颜色策略未在 daemon 中完成。
* **证据**:
  * `aura-cli/src/output/mem.rs:9-13`、`aura-cli/src/output/mem.rs:38-42`：分别计算 RAM、Swap 使用百分比。
  * `aura-cli/src/output/value.rs:10-17`、`19-26`、`28-35`：计算 CPU / 内存 / swap 百分比与网络字节和。
  * `aura-cli/src/output/ansi.rs:55-58`、`65-68`：`ColorMode::Ansi | Tmux | Zellij` 使用同一 ANSI 转义序列逻辑。
* **建议**: 将百分比、求和、颜色 tone 判断移到 daemon，SHM 中携带 `usage_percent` 与预计算 tone；CLI 仅做字符串拼接。

### AUD-012 MEDIUM：系统指纹缺 `version` / `versionCodename`；时间戳为 monotonic 而非绝对时间

* **严重性**: MEDIUM
* **分类**: Data Completeness
* **影响**: `project.md` 要求系统画像包含 `version/versionId` 与 `versionCodename`。当前 `OsFingerprint` 只有 `os_type`、`os_id`、`os_version_id`、`os_pretty_name`，缺少独立 `version` 与 `versionCodename` 字段。`timestamp_ns` 使用 `CLOCK_BOOTTIME`（monotonic），不是可读的绝对时间，下游 dashboard 无法直接显示时间。
* **证据**:
  * `aura-common/src/archive.rs:204-211`：`OsFingerprint` 无 `version`、`version_codename`。
  * `aura-daemon/src/collectors/meta.rs:48-73`：解析 `/etc/os-release` 时只读取 `ID`、`VERSION_ID`、`PRETTY_NAME`。
  * `aura-common/src/time.rs:1-15`：`monotonic_ns` 使用 `libc::CLOCK_BOOTTIME`。
* **建议**: 在 `OsFingerprint` 中新增字段并解析 `VERSION`、`VERSION_CODENAME`；或 JSON 输出中补充 `version` 与 `version_codename`。timestamp 增加独立 `wallclock_ns` 字段，保留 `monotonic_ns` 用于 freshness。

### AUD-013 MEDIUM：SHM 0o666 权限允许本地欺骗 / 截断 DoS；checksum 非认证；lockfile 缺少 `O_NOFOLLOW`/owner/mode 校验

* **严重性**: MEDIUM
* **分类**: Security
* **影响**:
  1. `SHM_FILE_MODE=0o666` 使任何本地用户可打开、截断、覆盖 SHM 文件，造成数据欺骗或 DoS。
  2. `TelemetryArchive::calculate_checksum` 使用 CRC32，仅能检测偶然损坏，无法防止恶意篡改（无认证）。
  3. lockfile 路径使用 `path.with_extension("lock")`，`OpenOptions` 创建时未使用 `O_NOFOLLOW`，也未校验 owner / mode，可被 symlink 指向敏感文件。
* **证据**:
  * `aura-common/src/consts.rs:13-14`：`SHM_FILE_MODE: u32 = 0o666`。
  * `aura-common/src/archive.rs:264-269`：`calculate_checksum` 使用 `crc32fast::Hasher`。
  * `aura-daemon/src/state.rs:27-33`：`lock_path` 通过 `OpenOptions::new().create(true).truncate(true).open(&lock_path)` 创建，无 `O_NOFOLLOW`。
  * `aura-daemon/src/state.rs:49-114`：对 SHM 主文件校验 owner / mode / symlink / size，但 lockfile 未校验。
* **建议**:
  1. SHM 权限收紧为 `0o600` 或 `0o660` 并依赖用户组；若必须跨用户，使用 `SOCK_SEQPACKET` 或带 ACL 的 domain socket。
  2. 在 lockfile 创建时加入 `O_NOFOLLOW`、owner/mode 校验。
  3. 明确文档说明 checksum 为完整性校验而非认证，威胁模型为本地非恶意损坏。

### AUD-014 LOW：文档被 `.gitignore` 忽略且未跟踪；无 RFC/ADR；关键 commit 已替代旧蓝图

* **严重性**: LOW
* **分类**: Documentation / Governance
* **影响**: `.gitignore` 第 22 行 `*.md` 导致 `docs/*.md`、`README.md`、`CHANGELOG.md` 不被跟踪。当前 `docs/project.md` 与 `docs/tech_blueprint.md` 是工作树文件但可能未纳入版本控制；没有可追溯的 RFC/ADR 记录 Process/Storage 删除与 SeqLock 重设计的决策。
* **证据**:
  * `.gitignore:22-27`：`*.md`、`.*/`、排除 `.github/`。
  * `git status` 未列出 `docs/*.md`（被忽略），README 也未列出；存在大量 untracked 文件如 `package.json`、`bun.lock`。
  * `git log --oneline` 显示 `4c419fc refactor: remove disk/process collectors, improve macOS support`、`59d33e7 fix: resolve 6 critical audit findings`、`87e5e74 refactor(double_buffer): replace global write_seq with per-buffer seq[2]`。
* **建议**:
  1. 调整 `.gitignore`，将设计文档、README、CHANGELOG 纳入版本控制。
  2. 在 `docs/` 下补 ADR-00x 记录 Process/Storage 移除、SeqLock 从单 global seq 到 per-buffer seq 的变更原因。

### AUD-015 LOW：测试通过但覆盖缺口明显

* **严重性**: LOW
* **分类**: Test Coverage
* **影响**: 当前 `cargo test --workspace` 全部通过，但缺失以下场景：
  * Process / Storage  collector 与 CLI 输出（随 4c419fc 删除）。
  * macOS pipeline 中的 network / memory / meta 非零断言（现有 macOS 测试仅验证结构体非空）。
  * release 构建启用 `gpu-nvml` 与 GPU JSON 输出。
  * watchdog 行为（超时、失败不喂狗）。
  * systemd / Home Manager / Homebrew 部署路径一致性。
  * 所有 offline 错误分类（stale vs missing SHM vs checksum vs seqlock）。
  * JSON 完整性与 storage 键存在性。
  * `>MAX_NETIFS` 边界（现有测试 fixture 仅 1 个接口）。
* **证据**:
  * `git show 4c419fc --stat` 删除 `aura-daemon/tests/heap_tests.rs`、`aura-daemon/tests/process_collector_tests.rs` 与 disk fixtures。
  * `.github/workflows/ci.yml` 无 `gpu-nvml` feature 测试；`test-macos` 仅运行默认测试。
  * `aura-daemon/src/collectors/network/linux.rs` 单元测试 fixture 只有 1 个接口。
* **建议**: 针对 AUD-001 至 AUD-013 的每个修复补充失败用例先行，再实现修复。

---

## 6. 通过的能力

以下能力符合设计基线或已正确实现：

* **Linux 全局 CPU 与上下文切换**: `aura-daemon/src/collectors/cpu/linux.rs:131-172` 正确解析 `/proc/stat`，按区间差值计算全局 `usage_percent` 与 `context_switches_per_sec`，并通过 `parse_global_cpu_and_ctxt` 测试。
* **Linux 内存 / 网络 / 元数据**: `memory/linux.rs`、`network/linux.rs`、`meta.rs` 能正确采集 `/proc/meminfo`、`/proc/vmstat`、`/proc/net/dev`、`/proc/loadavg`、`/etc/os-release`，并有单元测试覆盖。
* **GPU NVML 实现**: `gpu.rs` 在 `gpu-nvml` feature 启用时通过 `nvml-wrapper` 原生调用获取显存、利用率、功耗、温度，结构定义完整。
* **Double buffer 原子复制与 checksum**: `double_buffer.rs` 实现 per-buffer SeqLock + 8 字节原子读写；`reader.rs` 在读取后校验 CRC32；`tests/` 覆盖并发读写、checksum 损坏、reader 不阻塞 writer 等场景。
* **SHM 主文件安全校验**: `state.rs:49-114` 对已有 SHM 文件校验 symlink、owner、mode、size、regular file，拒绝非预期文件。
* **Stale data 降级**: `reader.rs:70-94` 在数据超过 `OFFLINE_THRESHOLD_SECS=2.0` 或 SHM mtime 过期时返回 offline，运行时验证输出 `[AURA: OFFLINE]`。
* **部署产物完整**: systemd、LaunchAgent、Home Manager、Homebrew formula 文件均存在，CI release 矩阵覆盖 aarch64/x86_64 Linux 与 macOS。

---

## 7. 构建 / 测试 / 运行时证据

### 7.1 静态检查

```bash
cargo fmt --all -- --check
# PASS（无输出）

cargo clippy --all-targets --all-features -- -D warnings
# PASS：Finished dev profile unoptimized + debuginfo target(s) in 0.15s
```

### 7.2 单元 / 集成测试

```bash
cargo test --workspace
# PASS：全部测试通过。
```

### 7.3 运行时验证

启动 daemon 后采集：

```bash
cargo run -q -p aura-daemon -- --shm-path /tmp/aura_test.dat --heartbeat-ms 500 &
sleep 2
cargo run -q -p aura-cli -- --shm-path /tmp/aura_test.dat --format json -m all
```

验证结果：

* `help` 输出 module 列表为 `cpu, mem, swap, net, all, os`，**缺少 `disk`**。
* JSON 输出包含 `cpu/memory/network/meta/gpu/process`，**无 `storage` 键**。
* `process.total=0`，`top_cpu` / `top_mem` 数组全为零条目，符合 AUD-005。
* `network.interfaces` 正确列出 `ens34`、`tailscale0` 并带速率。
* `meta.os.os_id="debian"`、`os_version_id="13"`，但无 `version` / `version_codename`，符合 AUD-012。

停止 daemon 3 秒后：

```bash
cargo run -q -p aura-cli -- --shm-path /tmp/aura_test.dat -m cpu
# [AURA: OFFLINE]
```

删除 SHM 后：

```bash
cargo run -q -p aura-cli -- --shm-path /tmp/aura_missing.dat -m cpu
# [AURA: ERROR - Shared memory error: No such file or directory (os error 2)]
```

说明 missing SHM 被分类为 ERROR 而非 OFFLINE，与蓝图预期不一致（AUD-009）。

---

## 8. Git 历史与文档权威性说明

* **HEAD**: `2ac76c0 fix(macos): use null_mut() for sysctl oldp parameter`
* **工作树状态**: dirty，存在大量已跟踪文件修改与若干未跟踪文件（如 `package.json`、`bun.lock`）。
* **关键提交**:
  * `4c419fc refactor: remove disk/process collectors, improve macOS support`；确认 disk/process collector 与测试是 **intentionally** 移除，非意外遗漏。
  * `59d33e7 fix: resolve 6 critical audit findings`；引入原子完整拷贝 + SHM 文件 owner/mode/symlink 校验。
  * `87e5e74 refactor(double_buffer): replace global write_seq with per-buffer seq[2]`；有意将旧蓝图中的单 global SeqLock counter 替换为 per-buffer double buffer seq。
* **文档权威性 caveat**: `.gitignore:22` 忽略所有 `*.md`，导致 `docs/project.md`、`docs/tech_blueprint.md`、`README.md` 可能不在 Git 版本控制内。本次审计把它们作为“被请求的基准”使用，但不保证它们与已发布版本一致。项目未跟踪任何 RFC/ADR，重大架构变更仅凭 commit message 描述，建议补全设计决策记录。
* **不推荐的回退**: 旧版单 counter SeqLock 已被 `87e5e74` 证明存在 false contention 风险，不建议回退；应在当前 double buffer 基础上补齐重试、超时与恢复逻辑。

---

## 9. 优先修复计划

按优先级排序：

1. **AUD-002**（CRITICAL）：修复 `MAX_NETIFS` 越界索引 panic/abort，独立上限检查与警告。
2. **AUD-001**（CRITICAL）：恢复 systemd `WATCHDOG=1` + `WatchdogSec` 软心跳；失败时不喂狗；`/dev/watchdog` fallback 必须配置超时。
3. **AUD-003 / AUD-005**（HIGH）：决定 Process / Storage 的去留。若保留则补齐 collector、CLI / JSON 路由、字段（IOPS/queue/latency）；若移除则清理 ABI 并更新文档。
4. **AUD-004**（HIGH）：collector 失败时停止本周期发布，不更新时间戳。
5. **AUD-006**（HIGH）：release CI 与 Homebrew 启用 `--features gpu-nvml`；macOS 增加 GPU unsupported 标记。
6. **AUD-007**（HIGH）：每核 CPU 按区间差值计算 `usage_percent`。
7. **AUD-010**（MEDIUM）：统一 systemd / Home Manager 默认 SHM 路径为 `/dev/shm/aura_state.dat`。
8. **AUD-009**（MEDIUM）：实现基于 `MAX_SPIN_WAIT_MS` 的 wall-clock 重试；区分 timeout / checksum / missing SHM；启动时验证 header。
9. **AUD-011**（MEDIUM）：将百分比、求和、颜色 tone 移到 daemon；CLI 只做展示。
10. **AUD-008 / AUD-012**（MEDIUM）：macOS 维度补 availability 标记；补齐 `version` / `versionCodename` 与 wall-clock timestamp。
11. **AUD-013**（MEDIUM）：收紧 SHM 权限，lockfile 加 `O_NOFOLLOW` / owner / mode 校验，文档说明 checksum 仅为完整性。
12. **AUD-014 / AUD-015**（LOW）：解除 `.gitignore` 对文档的忽略，补 ADR；针对上述每项补测试。

---

*报告结束。本文件为审计产物，未对仓库做任何源文件或提交修改。*
