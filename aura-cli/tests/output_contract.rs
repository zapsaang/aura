//! Hand-authored literal golden contract tests for the Todo-13 renderers.
//! No snapshot regeneration mechanism exists; every expectation is inline.

use std::fs::OpenOptions;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use aura_cli::args::{Args, ColorMode, Module, OutputFormat};
use aura_common::{
    monotonic_ns, write_double_buffer, CpuCoreStat, DiskStat, FixedString16, GpuStat, MountStat,
    NetIfStat, ProcessStat, TelemetryArchive, ARCHIVE_VERSION, CAP_CPU_CONTEXT_SWITCHES,
    CAP_CPU_GLOBAL, CAP_CPU_PER_CORE, CAP_GPU_ENUMERATION, CAP_MEMORY_BUFFERS, CAP_MEMORY_CACHED,
    CAP_MEMORY_PAGE_FAULTS, CAP_MEMORY_RAM_FREE, CAP_MEMORY_RAM_TOTAL, CAP_MEMORY_RAM_USED,
    CAP_MEMORY_SWAP, CAP_META_LOAD_AVERAGE, CAP_META_OS_CODENAME, CAP_META_OS_IDENTITY,
    CAP_META_OS_VERSION, CAP_META_OS_VERSION_ID, CAP_META_TIMEZONE, CAP_META_UPTIME,
    CAP_META_WALLCLOCK, CAP_NETWORK_BYTES, CAP_NETWORK_RATES, CAP_PROCESS_BLOCKED,
    CAP_PROCESS_RUNNING, CAP_PROCESS_SLEEPING, CAP_PROCESS_TOP_CPU, CAP_PROCESS_TOP_MEMORY,
    CAP_PROCESS_TOTAL, CAP_STORAGE_DISK_BYTES, CAP_STORAGE_DISK_IOPS, CAP_STORAGE_DISK_LATENCY,
    CAP_STORAGE_DISK_QUEUE_DEPTH, CAP_STORAGE_DISK_RATES, CAP_STORAGE_MOUNTS, GPU_CAP_MEMORY_TOTAL,
    GPU_CAP_MEMORY_USED, GPU_CAP_NAME, GPU_CAP_POWER, GPU_CAP_TEMPERATURE, GPU_CAP_UTILIZATION,
    MAX_CORES, MAX_DISKS, MAX_GPUS, MAX_MOUNTS, MAX_NETIFS, MAX_TOP_N, PROCESS_TRUNCATED, SHM_SIZE,
    TONE_GREEN, TONE_MAGENTA, TONE_RED, TONE_YELLOW,
};
use clap::Parser;
use memmap2::MmapOptions;

const ALL_CPU: u64 = CAP_CPU_GLOBAL | CAP_CPU_PER_CORE | CAP_CPU_CONTEXT_SWITCHES;
const ALL_PROCESS: u64 = CAP_PROCESS_TOTAL
    | CAP_PROCESS_RUNNING
    | CAP_PROCESS_BLOCKED
    | CAP_PROCESS_SLEEPING
    | CAP_PROCESS_TOP_CPU
    | CAP_PROCESS_TOP_MEMORY;
const ALL_MEMORY: u64 = CAP_MEMORY_RAM_TOTAL
    | CAP_MEMORY_RAM_FREE
    | CAP_MEMORY_RAM_USED
    | CAP_MEMORY_BUFFERS
    | CAP_MEMORY_CACHED
    | CAP_MEMORY_SWAP
    | CAP_MEMORY_PAGE_FAULTS;
const ALL_STORAGE: u64 = CAP_STORAGE_DISK_BYTES
    | CAP_STORAGE_DISK_RATES
    | CAP_STORAGE_DISK_IOPS
    | CAP_STORAGE_DISK_QUEUE_DEPTH
    | CAP_STORAGE_DISK_LATENCY
    | CAP_STORAGE_MOUNTS;
const ALL_NETWORK: u64 = CAP_NETWORK_BYTES | CAP_NETWORK_RATES;
const ALL_META: u64 = CAP_META_UPTIME
    | CAP_META_LOAD_AVERAGE
    | CAP_META_TIMEZONE
    | CAP_META_OS_IDENTITY
    | CAP_META_OS_VERSION
    | CAP_META_OS_VERSION_ID
    | CAP_META_OS_CODENAME
    | CAP_META_WALLCLOCK;
const ALL_GPU_RECORD: u64 = GPU_CAP_NAME
    | GPU_CAP_MEMORY_TOTAL
    | GPU_CAP_MEMORY_USED
    | GPU_CAP_UTILIZATION
    | GPU_CAP_POWER
    | GPU_CAP_TEMPERATURE;

fn archive(caps: u64) -> TelemetryArchive {
    let mut t = TelemetryArchive::zeroed();
    t.version = ARCHIVE_VERSION;
    t.capabilities = caps;
    t.meta.timestamp_ns = 1;
    t
}

fn fs16(text: &str) -> FixedString16 {
    FixedString16::from_bytes(text.as_bytes())
}

fn fixed<const N: usize>(text: &str) -> [u8; N] {
    let mut out = [0u8; N];
    out[..text.len()].copy_from_slice(text.as_bytes());
    out
}

fn human(module: Module, t: &TelemetryArchive) -> String {
    aura_cli::output::render(module, ColorMode::None, t)
}

fn human_color(module: Module, color: ColorMode, t: &TelemetryArchive) -> String {
    aura_cli::output::render(module, color, t)
}

fn value(module: Module, t: &TelemetryArchive) -> String {
    aura_cli::output::value::render(module, t)
}

fn json(module: Module, t: &TelemetryArchive) -> String {
    aura_cli::format::json::render(module, t).unwrap()
}

fn cpu_archive() -> TelemetryArchive {
    let mut t = archive(ALL_CPU);
    t.cpu.user_ticks = 100;
    t.cpu.system_ticks = 50;
    t.cpu.idle_ticks = 850;
    t.cpu.total_ticks = 1000;
    t.cpu.context_switches = 500;
    t.cpu.context_switches_per_sec = 1234.5;
    t.cpu.usage_percent = 42.0;
    t.cpu.cores = [CpuCoreStat {
        core_index: 0,
        _pad0: [0; 7],
        user_ticks: 50,
        system_ticks: 25,
        idle_ticks: 425,
        total_ticks: 500,
        usage_percent: 10.0,
        _pad1: [0; 4],
    }; MAX_CORES];
    t.cpu.cores[1].core_index = 1;
    t.cpu.cores[1].usage_percent = 20.5;
    for core in t.cpu.cores[2..].iter_mut() {
        *core = CpuCoreStat {
            core_index: 0,
            _pad0: [0; 7],
            user_ticks: 0,
            system_ticks: 0,
            idle_ticks: 0,
            total_ticks: 0,
            usage_percent: 0.0,
            _pad1: [0; 4],
        };
    }
    t.cpu.core_count = 2;
    t.derived.cpu_tone = TONE_GREEN;
    t
}

fn process_archive() -> TelemetryArchive {
    let mut t = archive(ALL_PROCESS);
    t.process.total = 10;
    t.process.running = 2;
    t.process.blocked = 1;
    t.process.sleeping = 3;
    t.process.top_cpu = [ProcessStat {
        pid: 42,
        cpu_usage: 50.0,
        memory_bytes: 1000,
        comm: fs16("top"),
    }; MAX_TOP_N];
    t.process.top_mem = [ProcessStat {
        pid: 43,
        cpu_usage: 99.0,
        memory_bytes: 2000,
        comm: fs16("mem"),
    }; MAX_TOP_N];
    t.process.top_cpu_count = 1;
    t.process.top_mem_count = 1;
    for idx in 1..MAX_TOP_N {
        t.process.top_cpu[idx] = ProcessStat::new();
        t.process.top_mem[idx] = ProcessStat::new();
    }
    t
}

fn memory_archive() -> TelemetryArchive {
    let mut t = archive(ALL_MEMORY);
    t.memory.ram_total = 16_000_000_000;
    t.memory.ram_free = 8_000_000_000;
    t.memory.ram_used = 8_000_000_000;
    t.memory.buffers = 500;
    t.memory.cached = 1500;
    t.memory.swap_total = 2_000_000_000;
    t.memory.swap_free = 1_000_000_000;
    t.memory.swap_used = 1_000_000_000;
    t.memory.page_faults = 9;
    t.memory.page_faults_per_sec = 7.5;
    t.derived.ram_used_percent = 50.0;
    t.derived.swap_used_percent = 50.0;
    t.derived.ram_tone = TONE_RED;
    t.derived.swap_tone = TONE_YELLOW;
    t
}

fn storage_archive() -> TelemetryArchive {
    let mut t = archive(ALL_STORAGE);
    t.storage.disks = [DiskStat {
        name: fs16("sda"),
        major: 8,
        minor: 0,
        read_bytes: 10,
        write_bytes: 20,
        read_bytes_per_sec: 1500.0,
        write_bytes_per_sec: 2_000_000.0,
        read_iops: 10.5,
        write_iops: 20.5,
        queue_depth: 3,
        read_latency_ms: 1.5,
        write_latency_ms: 2.5,
        _pad0: [0; 4],
    }; MAX_DISKS];
    t.storage.disk_count = 1;
    for disk in t.storage.disks[1..].iter_mut() {
        // SAFETY: `DiskStat` is `Zeroable`; the all-zero bit pattern is valid.
        *disk = unsafe { std::mem::zeroed() };
    }
    t.storage.mounts = [MountStat {
        mountpoint: fixed("/"),
        fstype: fs16("ext4"),
        total: 100_000_000_000,
        available: 25_000_000_000,
        used: 75_000_000_000,
        percent: 75.0,
        _pad0: [0; 4],
    }; MAX_MOUNTS];
    t.storage.mount_count = 1;
    for mount in t.storage.mounts[1..].iter_mut() {
        // SAFETY: `MountStat` is `Zeroable`; the all-zero bit pattern is valid.
        *mount = unsafe { std::mem::zeroed() };
    }
    t
}

fn network_archive() -> TelemetryArchive {
    let mut t = archive(ALL_NETWORK);
    t.network.interfaces = [NetIfStat {
        name: fs16("eth0"),
        rx_bytes: 1000,
        tx_bytes: 2000,
        rx_bytes_per_sec: 1000.0,
        tx_bytes_per_sec: 2000.0,
    }; MAX_NETIFS];
    t.network.if_count = 1;
    for iface in t.network.interfaces[1..].iter_mut() {
        *iface = NetIfStat::new();
    }
    t.derived.aggregate_rx_bytes_per_sec = 1500.0;
    t.derived.aggregate_tx_bytes_per_sec = 2500.0;
    t
}

fn meta_archive() -> TelemetryArchive {
    let mut t = archive(ALL_META);
    t.meta.wallclock_ns = 1_700_000_000_000_000_000;
    t.meta.uptime_secs = 3600;
    t.meta.load_avg_1m = 0.5;
    t.meta.load_avg_5m = 1.25;
    t.meta.load_avg_15m = 2.75;
    t.meta.timezone_name = fixed("UTC");
    t.meta.timezone_offset_secs = 0;
    t.meta.os.os_type = fs16("linux");
    t.meta.os.os_id = fs16("ubuntu");
    t.meta.os.os_version_id = fs16("24.04");
    t.meta.os.version_codename = fs16("noble");
    t.meta.os.version = fixed("24.04");
    t.meta.os.os_pretty_name = fixed("Ubuntu 24.04");
    t
}

fn gpu_record(tone: u8) -> GpuStat {
    GpuStat {
        name: fs16("nv"),
        memory_total: 16_000_000_000,
        memory_used: 8_000_000_000,
        utilization_percent: 55.5,
        power_watts: 200.5,
        temperature_celsius: 85,
        available: 1,
        tone,
        _pad0: [0; 4],
        capabilities: ALL_GPU_RECORD,
    }
}

fn gpu_archive() -> TelemetryArchive {
    let mut t = archive(CAP_GPU_ENUMERATION);
    t.gpu.gpus = [gpu_record(TONE_RED); MAX_GPUS];
    t.gpu.gpu_count = 1;
    for gpu in t.gpu.gpus[1..].iter_mut() {
        // SAFETY: `GpuStat` is `Zeroable`; the all-zero bit pattern is valid.
        *gpu = unsafe { std::mem::zeroed() };
    }
    t.gpu.nvml_available = 1;
    t
}

// ---------------------------------------------------------------- parser ---

fn parse_module(text: &str) -> Module {
    Args::try_parse_from(["aura-cli", "-m", text])
        .unwrap()
        .module
}

#[test]
fn module_literal_cpu() {
    assert_eq!(parse_module("cpu"), Module::Cpu);
}

#[test]
fn module_literal_process() {
    assert_eq!(parse_module("process"), Module::Process);
}

#[test]
fn module_literal_mem() {
    assert_eq!(parse_module("mem"), Module::Mem);
}

#[test]
fn module_literal_swap() {
    assert_eq!(parse_module("swap"), Module::Swap);
}

#[test]
fn module_literal_disk() {
    assert_eq!(parse_module("disk"), Module::Disk);
}

#[test]
fn module_literal_net() {
    assert_eq!(parse_module("net"), Module::Net);
}

#[test]
fn module_literal_os() {
    assert_eq!(parse_module("os"), Module::Os);
}

#[test]
fn module_literal_gpu() {
    assert_eq!(parse_module("gpu"), Module::Gpu);
}

#[test]
fn module_literal_all() {
    assert_eq!(parse_module("all"), Module::All);
}

#[test]
fn module_alias_proc() {
    assert_eq!(parse_module("proc"), Module::Process);
}

#[test]
fn module_alias_memory() {
    assert_eq!(parse_module("memory"), Module::Mem);
}

#[test]
fn module_alias_storage() {
    assert_eq!(parse_module("storage"), Module::Disk);
}

#[test]
fn module_alias_network() {
    assert_eq!(parse_module("network"), Module::Net);
}

#[test]
fn module_alias_meta() {
    assert_eq!(parse_module("meta"), Module::Os);
}

#[test]
fn module_rejects_uppercase_cpu() {
    assert!(Args::try_parse_from(["aura-cli", "-m", "CPU"]).is_err());
}

#[test]
fn module_rejects_uppercase_all() {
    assert!(Args::try_parse_from(["aura-cli", "-m", "ALL"]).is_err());
}

#[test]
fn module_rejects_mixed_case_alias() {
    assert!(Args::try_parse_from(["aura-cli", "-m", "Proc"]).is_err());
}

#[test]
fn color_literal_none() {
    let a = Args::try_parse_from(["aura-cli", "--color", "none"]).unwrap();
    assert_eq!(a.color, ColorMode::None);
}

#[test]
fn color_literal_ansi() {
    let a = Args::try_parse_from(["aura-cli", "--color", "ansi"]).unwrap();
    assert_eq!(a.color, ColorMode::Ansi);
}

#[test]
fn color_literal_tmux() {
    let a = Args::try_parse_from(["aura-cli", "--color", "tmux"]).unwrap();
    assert_eq!(a.color, ColorMode::Tmux);
}

#[test]
fn color_literal_zellij() {
    let a = Args::try_parse_from(["aura-cli", "--color", "zellij"]).unwrap();
    assert_eq!(a.color, ColorMode::Zellij);
}

#[test]
fn color_rejects_uppercase_ansi() {
    assert!(Args::try_parse_from(["aura-cli", "--color", "ANSI"]).is_err());
}

#[test]
fn color_has_no_aliases() {
    assert!(Args::try_parse_from(["aura-cli", "--color", "off"]).is_err());
    assert!(Args::try_parse_from(["aura-cli", "--color", "plain"]).is_err());
}

#[test]
fn defaults_are_all_ansi_human() {
    let a = Args::try_parse_from(["aura-cli"]).unwrap();
    assert_eq!(a.module, Module::All);
    assert_eq!(a.color, ColorMode::Ansi);
    assert_eq!(a.format, OutputFormat::Human);
}

#[test]
fn format_literals() {
    for (text, want) in [
        ("human", OutputFormat::Human),
        ("json", OutputFormat::Json),
        ("value", OutputFormat::Value),
    ] {
        let a = Args::try_parse_from(["aura-cli", "--format", text]).unwrap();
        assert_eq!(a.format, want);
    }
}

#[test]
fn format_rejects_uppercase() {
    assert!(Args::try_parse_from(["aura-cli", "--format", "JSON"]).is_err());
}

// Green baseline per the raw-format plan: clap's case-sensitive parser already
// rejects `Raw`, so this is a parser regression guard, not a red-first test.
#[test]
fn format_rejects_uppercase_raw() {
    assert!(Args::try_parse_from(["aura-cli", "--format", "Raw"]).is_err());
}

// ------------------------------------------------------------ human cpu ---

#[test]
fn human_cpu_unsupported_exact() {
    let t = archive(0);
    assert_eq!(
        human(Module::Cpu, &t),
        "CPU\n  usage: N/A\n  context switches/s: N/A\n  cores:\n    N/A"
    );
}

#[test]
fn human_cpu_supported_exact() {
    let t = cpu_archive();
    assert_eq!(
        human(Module::Cpu, &t),
        "CPU\n  usage: 42.0%\n  context switches/s: 1234.5\n  cores:\n    cpu0: 10.0%\n    cpu1: 20.5%"
    );
}

#[test]
fn human_cpu_supported_empty_cores() {
    let mut t = cpu_archive();
    t.cpu.core_count = 0;
    assert_eq!(
        human(Module::Cpu, &t),
        "CPU\n  usage: 42.0%\n  context switches/s: 1234.5\n  cores:\n    (none)"
    );
}

#[test]
fn human_cpu_partial_without_context() {
    let mut t = cpu_archive();
    t.capabilities = CAP_CPU_GLOBAL | CAP_CPU_PER_CORE;
    assert_eq!(
        human(Module::Cpu, &t),
        "CPU\n  usage: 42.0%\n  context switches/s: N/A\n  cores:\n    cpu0: 10.0%\n    cpu1: 20.5%"
    );
}

#[test]
fn human_cpu_ansi_colors_only_usage() {
    let t = cpu_archive();
    assert_eq!(
        human_color(Module::Cpu, ColorMode::Ansi, &t),
        "CPU\n  usage: \x1b[32m42.0%\x1b[0m\n  context switches/s: 1234.5\n  cores:\n    cpu0: 10.0%\n    cpu1: 20.5%"
    );
}

#[test]
fn human_cpu_zellij_uses_sgr() {
    let t = cpu_archive();
    assert_eq!(
        human_color(Module::Cpu, ColorMode::Zellij, &t),
        "CPU\n  usage: \x1b[32m42.0%\x1b[0m\n  context switches/s: 1234.5\n  cores:\n    cpu0: 10.0%\n    cpu1: 20.5%"
    );
}

#[test]
fn human_cpu_tmux_wraps_usage() {
    let t = cpu_archive();
    assert_eq!(
        human_color(Module::Cpu, ColorMode::Tmux, &t),
        "CPU\n  usage: #[fg=green]42.0%#[default]\n  context switches/s: 1234.5\n  cores:\n    cpu0: 10.0%\n    cpu1: 20.5%"
    );
}

#[test]
fn human_cpu_tone_red_ansi() {
    let mut t = cpu_archive();
    t.derived.cpu_tone = TONE_RED;
    assert!(
        human_color(Module::Cpu, ColorMode::Ansi, &t).contains("  usage: \x1b[31m42.0%\x1b[0m\n")
    );
}

#[test]
fn human_cpu_unsupported_never_colored() {
    let t = archive(0);
    let out = human_color(Module::Cpu, ColorMode::Ansi, &t);
    assert!(!out.contains('\x1b'));
}

// -------------------------------------------------------- human process ---

#[test]
fn human_process_unsupported_exact() {
    let t = archive(0);
    assert_eq!(
        human(Module::Process, &t),
        "PROCESS\n  total: N/A\n  running: N/A\n  blocked: N/A\n  sleeping: N/A\n  top cpu:\n    N/A\n  top memory:\n    N/A"
    );
}

#[test]
fn human_process_supported_exact() {
    let t = process_archive();
    assert_eq!(
        human(Module::Process, &t),
        "PROCESS\n  total: 10\n  running: 2\n  blocked: 1\n  sleeping: 3\n  top cpu:\n    42 top 50.0% 1.0KB\n  top memory:\n    43 mem 2.0KB"
    );
}

#[test]
fn human_process_top_memory_row_has_no_cpu_field() {
    let t = process_archive();
    let out = human(Module::Process, &t);
    let mem_section = out.split("  top memory:\n").nth(1).unwrap();
    assert_eq!(mem_section, "    43 mem 2.0KB");
}

#[test]
fn human_process_supported_empty_tops() {
    let mut t = process_archive();
    t.process.top_cpu_count = 0;
    t.process.top_mem_count = 0;
    assert_eq!(
        human(Module::Process, &t),
        "PROCESS\n  total: 10\n  running: 2\n  blocked: 1\n  sleeping: 3\n  top cpu:\n    (none)\n  top memory:\n    (none)"
    );
}

#[test]
fn human_process_partial_counts() {
    let mut t = process_archive();
    t.capabilities = CAP_PROCESS_TOTAL | CAP_PROCESS_TOP_CPU;
    assert_eq!(
        human(Module::Process, &t),
        "PROCESS\n  total: 10\n  running: N/A\n  blocked: N/A\n  sleeping: N/A\n  top cpu:\n    42 top 50.0% 1.0KB\n  top memory:\n    N/A"
    );
}

// --------------------------------------------------------- human memory ---

#[test]
fn human_memory_unsupported_exact() {
    let t = archive(0);
    assert_eq!(
        human(Module::Mem, &t),
        "MEMORY\n  ram: N/A / N/A (N/A)\n  free: N/A\n  buffers: N/A\n  cached: N/A\n  page faults/s: N/A"
    );
}

#[test]
fn human_memory_supported_exact() {
    let t = memory_archive();
    assert_eq!(
        human(Module::Mem, &t),
        "MEMORY\n  ram: 8.0GB / 16.0GB (50.0%)\n  free: 8.0GB\n  buffers: 500.0B\n  cached: 1.5KB\n  page faults/s: 7.5"
    );
}

#[test]
fn human_memory_ansi_colors_only_percent() {
    let t = memory_archive();
    assert_eq!(
        human_color(Module::Mem, ColorMode::Ansi, &t),
        "MEMORY\n  ram: 8.0GB / 16.0GB (\x1b[31m50.0%\x1b[0m)\n  free: 8.0GB\n  buffers: 500.0B\n  cached: 1.5KB\n  page faults/s: 7.5"
    );
}

#[test]
fn human_memory_partial_total_only() {
    let mut t = memory_archive();
    t.capabilities = CAP_MEMORY_RAM_TOTAL;
    assert_eq!(
        human(Module::Mem, &t),
        "MEMORY\n  ram: N/A / 16.0GB (N/A)\n  free: N/A\n  buffers: N/A\n  cached: N/A\n  page faults/s: N/A"
    );
}

#[test]
fn human_memory_percent_requires_total_and_used() {
    let mut t = memory_archive();
    t.capabilities = CAP_MEMORY_RAM_USED;
    let out = human(Module::Mem, &t);
    assert!(out.contains("  ram: 8.0GB / N/A (N/A)"));
}

#[test]
fn human_swap_unsupported_exact() {
    let t = archive(0);
    assert_eq!(human(Module::Swap, &t), "SWAP\n  used: N/A / N/A (N/A)");
}

#[test]
fn human_swap_supported_exact() {
    let t = memory_archive();
    assert_eq!(
        human(Module::Swap, &t),
        "SWAP\n  used: 1.0GB / 2.0GB (50.0%)"
    );
}

#[test]
fn human_swap_ansi_colors_percent() {
    let t = memory_archive();
    assert_eq!(
        human_color(Module::Swap, ColorMode::Ansi, &t),
        "SWAP\n  used: 1.0GB / 2.0GB (\x1b[33m50.0%\x1b[0m)"
    );
}

// -------------------------------------------------------- human storage ---

#[test]
fn human_storage_unsupported_exact() {
    let t = archive(0);
    assert_eq!(
        human(Module::Disk, &t),
        "STORAGE\n  disks:\n    N/A\n  mounts:\n    N/A"
    );
}

#[test]
fn human_storage_supported_exact() {
    let t = storage_archive();
    assert_eq!(
        human(Module::Disk, &t),
        "STORAGE\n  disks:\n    sda read=1.5KB/s write=2.0MB/s riops=10.5 wiops=20.5 queue=3 rlat=1.5ms wlat=2.5ms\n  mounts:\n    / ext4 75.0GB / 100.0GB (75.0%) avail=25.0GB"
    );
}

#[test]
fn human_storage_supported_empty() {
    let mut t = storage_archive();
    t.storage.disk_count = 0;
    t.storage.mount_count = 0;
    assert_eq!(
        human(Module::Disk, &t),
        "STORAGE\n  disks:\n    (none)\n  mounts:\n    (none)"
    );
}

#[test]
fn human_storage_partial_rates_only_row_stays() {
    let mut t = storage_archive();
    t.capabilities = CAP_STORAGE_DISK_RATES | CAP_STORAGE_MOUNTS;
    assert_eq!(
        human(Module::Disk, &t),
        "STORAGE\n  disks:\n    N/A read=1.5KB/s write=2.0MB/s riops=N/A wiops=N/A queue=N/A rlat=N/A wlat=N/A\n  mounts:\n    / ext4 75.0GB / 100.0GB (75.0%) avail=25.0GB"
    );
}

#[test]
fn human_storage_rate_tokens_absorb_suffix() {
    let mut t = storage_archive();
    t.capabilities = CAP_STORAGE_DISK_BYTES;
    t.storage.mount_count = 0;
    let out = human(Module::Disk, &t);
    assert!(out.contains("read=N/A write=N/A"));
    assert!(!out.contains("N/A/s"));
}

// -------------------------------------------------------- human network ---

#[test]
fn human_network_unsupported_exact() {
    let t = archive(0);
    assert_eq!(
        human(Module::Net, &t),
        "NETWORK\n  total: rx=N/A tx=N/A\n  interfaces:\n    N/A"
    );
}

#[test]
fn human_network_supported_exact() {
    let t = network_archive();
    assert_eq!(
        human(Module::Net, &t),
        "NETWORK\n  total: rx=1.5KB/s tx=2.5KB/s\n  interfaces:\n    eth0 rx=1.0KB/s tx=2.0KB/s"
    );
}

#[test]
fn human_network_supported_empty_interfaces() {
    let mut t = network_archive();
    t.network.if_count = 0;
    assert_eq!(
        human(Module::Net, &t),
        "NETWORK\n  total: rx=1.5KB/s tx=2.5KB/s\n  interfaces:\n    (none)"
    );
}

#[test]
fn human_network_partial_bytes_only() {
    let mut t = network_archive();
    t.capabilities = CAP_NETWORK_BYTES;
    assert_eq!(
        human(Module::Net, &t),
        "NETWORK\n  total: rx=N/A tx=N/A\n  interfaces:\n    eth0 rx=N/A tx=N/A"
    );
}

// ----------------------------------------------------------- human meta ---

#[test]
fn human_meta_unsupported_exact() {
    let t = archive(0);
    assert_eq!(
        human(Module::Os, &t),
        "META\n  wallclock_ns: N/A\n  uptime: N/A\n  load: N/A N/A N/A\n  timezone: N/A (N/A)\n  os: N/A id=N/A version=N/A version_id=N/A codename=N/A"
    );
}

#[test]
fn human_meta_supported_exact() {
    let t = meta_archive();
    assert_eq!(
        human(Module::Os, &t),
        "META\n  wallclock_ns: 1700000000000000000\n  uptime: 3600s\n  load: 0.50 1.25 2.75\n  timezone: UTC (0)\n  os: Ubuntu 24.04 id=ubuntu version=24.04 version_id=24.04 codename=noble"
    );
}

#[test]
fn human_meta_partial_identity_only() {
    let mut t = meta_archive();
    t.capabilities = CAP_META_OS_IDENTITY;
    assert_eq!(
        human(Module::Os, &t),
        "META\n  wallclock_ns: N/A\n  uptime: N/A\n  load: N/A N/A N/A\n  timezone: N/A (N/A)\n  os: Ubuntu 24.04 id=ubuntu version=N/A version_id=N/A codename=N/A"
    );
}

// ------------------------------------------------------------ human gpu ---

#[test]
fn human_gpu_unsupported_exact() {
    let t = archive(0);
    assert_eq!(human(Module::Gpu, &t), "GPU\n  devices:\n    N/A");
}

#[test]
fn human_gpu_supported_empty() {
    let mut t = gpu_archive();
    t.gpu.gpu_count = 0;
    assert_eq!(human(Module::Gpu, &t), "GPU\n  devices:\n    (none)");
}

#[test]
fn human_gpu_supported_exact() {
    let t = gpu_archive();
    assert_eq!(
        human(Module::Gpu, &t),
        "GPU\n  devices:\n    0 nv memory=8.0GB/16.0GB util=55.5% power=200.5W temp=85C"
    );
}

#[test]
fn human_gpu_partial_power_na_whole_token() {
    let mut t = gpu_archive();
    t.gpu.gpus[0].capabilities = ALL_GPU_RECORD & !GPU_CAP_POWER;
    assert_eq!(
        human(Module::Gpu, &t),
        "GPU\n  devices:\n    0 nv memory=8.0GB/16.0GB util=55.5% power=N/A temp=85C"
    );
}

#[test]
fn human_gpu_temp_na_uncolored() {
    let mut t = gpu_archive();
    t.gpu.gpus[0].capabilities = ALL_GPU_RECORD & !GPU_CAP_TEMPERATURE;
    let out = human_color(Module::Gpu, ColorMode::Ansi, &t);
    assert_eq!(
        out,
        "GPU\n  devices:\n    0 nv memory=8.0GB/16.0GB util=55.5% power=200.5W temp=N/A"
    );
    assert!(!out.contains('\x1b'));
}

#[test]
fn human_gpu_ansi_colors_only_temp() {
    let t = gpu_archive();
    assert_eq!(
        human_color(Module::Gpu, ColorMode::Ansi, &t),
        "GPU\n  devices:\n    0 nv memory=8.0GB/16.0GB util=55.5% power=200.5W temp=\x1b[31m85C\x1b[0m"
    );
}

#[test]
fn human_gpu_tmux_colors_temp() {
    let mut t = gpu_archive();
    t.gpu.gpus[0].tone = TONE_YELLOW;
    t.gpu.gpus[0].temperature_celsius = 70;
    assert_eq!(
        human_color(Module::Gpu, ColorMode::Tmux, &t),
        "GPU\n  devices:\n    0 nv memory=8.0GB/16.0GB util=55.5% power=200.5W temp=#[fg=yellow]70C#[default]"
    );
}

// ------------------------------------------------------------ human all ---

#[test]
fn human_all_unsupported_exact() {
    let t = archive(0);
    let expected = "CPU\n  usage: N/A\n  context switches/s: N/A\n  cores:\n    N/A\n\
        \n\
        PROCESS\n  total: N/A\n  running: N/A\n  blocked: N/A\n  sleeping: N/A\n  top cpu:\n    N/A\n  top memory:\n    N/A\n\
        \n\
        MEMORY\n  ram: N/A / N/A (N/A)\n  free: N/A\n  buffers: N/A\n  cached: N/A\n  page faults/s: N/A\n\
        \n\
        SWAP\n  used: N/A / N/A (N/A)\n\
        \n\
        STORAGE\n  disks:\n    N/A\n  mounts:\n    N/A\n\
        \n\
        NETWORK\n  total: rx=N/A tx=N/A\n  interfaces:\n    N/A\n\
        \n\
        META\n  wallclock_ns: N/A\n  uptime: N/A\n  load: N/A N/A N/A\n  timezone: N/A (N/A)\n  os: N/A id=N/A version=N/A version_id=N/A codename=N/A\n\
        \n\
        GPU\n  devices:\n    N/A";
    assert_eq!(human(Module::All, &t), expected);
}

#[test]
fn human_all_block_order_and_blank_separator() {
    let mut t = archive(0);
    t.capabilities = 0;
    let out = human(Module::All, &t);
    let headers: Vec<&str> = out
        .lines()
        .filter(|l| !l.starts_with(' ') && !l.is_empty())
        .collect();
    assert_eq!(
        headers,
        ["CPU", "PROCESS", "MEMORY", "SWAP", "STORAGE", "NETWORK", "META", "GPU"]
    );
    assert_eq!(out.matches("\n\n").count(), 7);
    assert!(!out.ends_with('\n'));
}

#[test]
fn human_renderers_omit_final_newline() {
    for module in [
        Module::Cpu,
        Module::Process,
        Module::Mem,
        Module::Swap,
        Module::Disk,
        Module::Net,
        Module::Os,
        Module::Gpu,
        Module::All,
    ] {
        let t = archive(0);
        assert!(
            !human(module, &t).ends_with('\n'),
            "{module:?} trailing newline"
        );
    }
}

// ----------------------------------------------------------------- value ---

#[test]
fn value_cpu_exact() {
    assert_eq!(value(Module::Cpu, &cpu_archive()), "cpu=42.0%");
}

#[test]
fn value_process_exact() {
    assert_eq!(value(Module::Process, &process_archive()), "process=50.0%");
}

#[test]
fn value_mem_exact() {
    assert_eq!(value(Module::Mem, &memory_archive()), "mem=50.0%");
}

#[test]
fn value_swap_exact() {
    assert_eq!(value(Module::Swap, &memory_archive()), "swap=50.0%");
}

#[test]
fn value_disk_exact() {
    assert_eq!(
        value(Module::Disk, &storage_archive()),
        "disk=read=1.5KB/s,write=2.0MB/s"
    );
}

#[test]
fn value_net_exact() {
    assert_eq!(
        value(Module::Net, &network_archive()),
        "net=rx=1.5KB/s,tx=2.5KB/s"
    );
}

#[test]
fn value_os_exact() {
    assert_eq!(value(Module::Os, &meta_archive()), "os=\u{f31b}");
}

#[test]
fn value_os_macos_identity_exact() {
    let mut t = meta_archive();
    t.meta.os.os_type = fs16("Darwin");
    t.meta.os.os_id = fs16("macos");
    t.meta.os.os_pretty_name = fixed("macOS");
    assert_eq!(value(Module::Os, &t), "os=\u{f302}");
}

#[test]
fn value_gpu_exact() {
    assert_eq!(value(Module::Gpu, &gpu_archive()), "gpu=55.5%");
}

#[test]
fn value_cpu_na_when_global_clear() {
    assert_eq!(value(Module::Cpu, &archive(0)), "cpu=N/A");
}

#[test]
fn value_process_na_when_cap_clear() {
    assert_eq!(value(Module::Process, &archive(0)), "process=N/A");
}

#[test]
fn value_process_na_when_top_cpu_empty() {
    let mut t = process_archive();
    t.process.top_cpu_count = 0;
    assert_eq!(value(Module::Process, &t), "process=N/A");
}

#[test]
fn value_mem_na_when_used_clear() {
    let mut t = memory_archive();
    t.capabilities = CAP_MEMORY_RAM_TOTAL;
    assert_eq!(value(Module::Mem, &t), "mem=N/A");
}

#[test]
fn value_swap_na_when_cap_clear() {
    let mut t = memory_archive();
    t.capabilities &= !CAP_MEMORY_SWAP;
    assert_eq!(value(Module::Swap, &t), "swap=N/A");
}

#[test]
fn value_disk_na_when_rates_clear() {
    let mut t = storage_archive();
    t.capabilities = CAP_STORAGE_DISK_BYTES;
    assert_eq!(value(Module::Disk, &t), "disk=N/A");
}

#[test]
fn value_disk_na_when_no_disks() {
    let mut t = storage_archive();
    t.storage.disk_count = 0;
    assert_eq!(value(Module::Disk, &t), "disk=N/A");
}

#[test]
fn value_net_na_when_rates_clear() {
    let mut t = network_archive();
    t.capabilities = CAP_NETWORK_BYTES;
    assert_eq!(value(Module::Net, &t), "net=N/A");
}

#[test]
fn value_net_na_when_no_interfaces() {
    let mut t = network_archive();
    t.network.if_count = 0;
    assert_eq!(value(Module::Net, &t), "net=N/A");
}

#[test]
fn value_os_na_when_identity_clear() {
    let mut t = meta_archive();
    t.capabilities &= !CAP_META_OS_IDENTITY;
    assert_eq!(value(Module::Os, &t), "os=N/A");
}

#[test]
fn value_gpu_na_when_enumeration_clear() {
    assert_eq!(value(Module::Gpu, &archive(0)), "gpu=N/A");
}

#[test]
fn value_gpu_na_when_utilization_clear() {
    let mut t = gpu_archive();
    t.gpu.gpus[0].capabilities &= !GPU_CAP_UTILIZATION;
    assert_eq!(value(Module::Gpu, &t), "gpu=N/A");
}

#[test]
fn value_gpu_na_when_no_gpus() {
    let mut t = gpu_archive();
    t.gpu.gpu_count = 0;
    assert_eq!(value(Module::Gpu, &t), "gpu=N/A");
}

#[test]
fn value_si_decimal_boundaries() {
    let mut t = storage_archive();
    t.storage.disks[0].read_bytes_per_sec = 999.0;
    t.storage.disks[0].write_bytes_per_sec = 1000.0;
    assert_eq!(value(Module::Disk, &t), "disk=read=999.0B/s,write=1.0KB/s");
}

#[test]
fn value_all_exact_eight_rows() {
    let mut t = archive(
        ALL_CPU
            | ALL_PROCESS
            | ALL_MEMORY
            | ALL_STORAGE
            | ALL_NETWORK
            | ALL_META
            | CAP_GPU_ENUMERATION,
    );
    t.cpu.usage_percent = 1.5;
    t.process.top_cpu = process_archive().process.top_cpu;
    t.process.top_cpu_count = 1;
    t.derived.ram_used_percent = 2.5;
    t.derived.swap_used_percent = 3.5;
    t.derived.aggregate_rx_bytes_per_sec = 6.0;
    t.derived.aggregate_tx_bytes_per_sec = 7.0;
    t.storage.disks = storage_archive().storage.disks;
    t.storage.disk_count = 1;
    t.network.if_count = 1;
    t.meta = meta_archive().meta;
    t.gpu.gpus = gpu_archive().gpu.gpus;
    t.gpu.gpu_count = 1;
    assert_eq!(
        value(Module::All, &t),
        "cpu=1.5%\nprocess=50.0%\nmem=2.5%\nswap=3.5%\ndisk=read=1.5KB/s,write=2.0MB/s\nnet=rx=6.0B/s,tx=7.0B/s\nos=\u{f31b}\ngpu=55.5%"
    );
}

#[test]
fn value_all_unsupported_exact() {
    let t = archive(0);
    assert_eq!(
        value(Module::All, &t),
        "cpu=N/A\nprocess=N/A\nmem=N/A\nswap=N/A\ndisk=N/A\nnet=N/A\nos=N/A\ngpu=N/A"
    );
}

#[test]
fn value_renderer_omits_final_newline() {
    assert!(!value(Module::All, &archive(0)).ends_with('\n'));
    assert!(!value(Module::Cpu, &cpu_archive()).ends_with('\n'));
}

// ------------------------------------------------------------------ json ---

const CAPABILITY_KEYS: [&str; 33] = [
    "cpu_global",
    "cpu_per_core",
    "cpu_context_switches",
    "process_total",
    "process_running",
    "process_blocked",
    "process_sleeping",
    "process_top_cpu",
    "process_top_memory",
    "memory_ram_total",
    "memory_ram_free",
    "memory_ram_used",
    "memory_buffers",
    "memory_cached",
    "memory_swap",
    "memory_page_faults",
    "storage_disk_bytes",
    "storage_disk_rates",
    "storage_disk_iops",
    "storage_disk_queue_depth",
    "storage_disk_latency",
    "storage_mounts",
    "network_bytes",
    "network_rates",
    "meta_uptime",
    "meta_load_average",
    "meta_timezone",
    "meta_os_identity",
    "meta_os_version",
    "meta_os_version_id",
    "meta_os_codename",
    "meta_wallclock",
    "gpu_enumeration",
];

#[test]
fn json_capabilities_keys_in_declaration_order() {
    let t = archive(0);
    let out = json(Module::All, &t);
    let mut last = 0;
    for key in CAPABILITY_KEYS {
        let needle = format!("\"{key}\":");
        let pos = out[last..]
            .find(&needle)
            .unwrap_or_else(|| panic!("missing {key}"))
            + last;
        assert!(pos > last || key == "cpu_global", "{key} out of order");
        last = pos;
    }
}

#[test]
fn json_root_declaration_order() {
    let t = archive(0);
    let out = json(Module::All, &t);
    let keys = [
        "\"version\":",
        "\"capabilities\":",
        "\"cpu\":",
        "\"process\":",
        "\"memory\":",
        "\"storage\":",
        "\"network\":",
        "\"meta\":",
        "\"gpu\":",
    ];
    let mut last = 0;
    for key in keys {
        let pos = out.find(key).unwrap_or_else(|| panic!("missing {key}"));
        assert!(pos >= last, "{key} out of order");
        last = pos;
    }
}

#[test]
fn json_unsupported_all_exact() {
    let t = archive(0);
    let out = json(Module::All, &t);
    let caps = CAPABILITY_KEYS
        .iter()
        .map(|k| format!("    \"{k}\": false,"))
        .collect::<Vec<_>>()
        .join("\n");
    let caps = caps.strip_suffix(',').unwrap();
    let expected = format!(
        "{{\n  \"version\": 2,\n  \"capabilities\": {{\n{caps}\n  }},\n  \"cpu\": {{\n    \"user_ticks\": null,\n    \"system_ticks\": null,\n    \"idle_ticks\": null,\n    \"total_ticks\": null,\n    \"context_switches\": null,\n    \"context_switches_per_sec\": null,\n    \"usage_percent\": null,\n    \"cores\": null\n  }},\n  \"process\": {{\n    \"total\": null,\n    \"running\": null,\n    \"blocked\": null,\n    \"sleeping\": null,\n    \"truncated\": null,\n    \"top_cpu\": null,\n    \"top_memory\": null\n  }},\n  \"memory\": {{\n    \"ram_total\": null,\n    \"ram_free\": null,\n    \"ram_used\": null,\n    \"ram_used_percent\": null,\n    \"buffers\": null,\n    \"cached\": null,\n    \"swap_total\": null,\n    \"swap_free\": null,\n    \"swap_used\": null,\n    \"swap_used_percent\": null,\n    \"page_faults\": null,\n    \"page_faults_per_sec\": null\n  }},\n  \"storage\": {{\n    \"disk_truncated\": null,\n    \"mount_truncated\": null,\n    \"disks\": null,\n    \"mounts\": null\n  }},\n  \"network\": {{\n    \"truncated\": null,\n    \"aggregate_rx_bytes_per_sec\": null,\n    \"aggregate_tx_bytes_per_sec\": null,\n    \"interfaces\": null\n  }},\n  \"meta\": {{\n    \"timestamp_ns\": 1,\n    \"wallclock_ns\": null,\n    \"uptime_secs\": null,\n    \"load_avg_1m\": null,\n    \"load_avg_5m\": null,\n    \"load_avg_15m\": null,\n    \"timezone_name\": null,\n    \"timezone_offset_secs\": null,\n    \"os\": {{\n      \"os_type\": null,\n      \"os_id\": null,\n      \"version\": null,\n      \"version_id\": null,\n      \"version_codename\": null,\n      \"pretty_name\": null\n    }}\n  }},\n  \"gpu\": {{\n    \"nvml_available\": null,\n    \"gpu_truncated\": null,\n    \"gpus\": null\n  }}\n}}"
    );
    assert_eq!(out, expected);
}

#[test]
fn json_two_space_pretty_no_trailing_newline() {
    let t = cpu_archive();
    let out = json(Module::Cpu, &t);
    assert!(out.contains("\n  \"cpu\": {"));
    assert!(out.contains("\n    \"usage_percent\": 42.0"));
    assert!(!out.ends_with('\n'));
}

#[test]
fn json_cpu_module_exact() {
    let mut t = archive(CAP_CPU_GLOBAL);
    t.cpu.user_ticks = 100;
    t.cpu.system_ticks = 50;
    t.cpu.idle_ticks = 850;
    t.cpu.total_ticks = 1000;
    t.cpu.usage_percent = 15.0;
    let out = json(Module::Cpu, &t);
    let caps = CAPABILITY_KEYS
        .iter()
        .map(|k| format!("    \"{k}\": {}", *k == "cpu_global"))
        .collect::<Vec<_>>()
        .join(",\n");
    let expected = format!(
        "{{\n  \"version\": 2,\n  \"capabilities\": {{\n{caps}\n  }},\n  \"cpu\": {{\n    \"user_ticks\": 100,\n    \"system_ticks\": 50,\n    \"idle_ticks\": 850,\n    \"total_ticks\": 1000,\n    \"context_switches\": null,\n    \"context_switches_per_sec\": null,\n    \"usage_percent\": 15.0,\n    \"cores\": null\n  }}\n}}"
    );
    assert_eq!(out, expected);
}

#[test]
fn json_module_selection_omits_unselected_dimensions() {
    let t = cpu_archive();
    let v: serde_json::Value = serde_json::from_str(&json(Module::Cpu, &t)).unwrap();
    let obj = v.as_object().unwrap();
    assert_eq!(obj.len(), 3);
    assert!(obj.contains_key("version"));
    assert!(obj.contains_key("capabilities"));
    assert!(obj.contains_key("cpu"));
}

#[test]
fn json_swap_module_selects_memory() {
    let t = memory_archive();
    let v: serde_json::Value = serde_json::from_str(&json(Module::Swap, &t)).unwrap();
    let obj = v.as_object().unwrap();
    assert_eq!(obj.len(), 3);
    assert!(obj.contains_key("memory"));
    assert_eq!(v["memory"]["swap_used_percent"], 50.0);
}

#[test]
fn json_process_supported_empty_tops() {
    let mut t = archive(CAP_PROCESS_TOP_CPU | CAP_PROCESS_TOP_MEMORY);
    t.process.top_cpu_count = 0;
    t.process.top_mem_count = 0;
    let v: serde_json::Value = serde_json::from_str(&json(Module::Process, &t)).unwrap();
    assert_eq!(v["process"]["top_cpu"], serde_json::json!([]));
    assert_eq!(v["process"]["top_memory"], serde_json::json!([]));
}

#[test]
fn json_storage_supported_empty_disks() {
    let mut t = archive(CAP_STORAGE_DISK_RATES);
    t.storage.disk_count = 0;
    let v: serde_json::Value = serde_json::from_str(&json(Module::Disk, &t)).unwrap();
    assert_eq!(v["storage"]["disks"], serde_json::json!([]));
    assert_eq!(v["storage"]["disk_truncated"], false);
    assert_eq!(v["storage"]["mounts"], serde_json::Value::Null);
    assert_eq!(v["storage"]["mount_truncated"], serde_json::Value::Null);
}

#[test]
fn json_network_supported_empty_interfaces() {
    let mut t = archive(CAP_NETWORK_BYTES);
    t.network.if_count = 0;
    let v: serde_json::Value = serde_json::from_str(&json(Module::Net, &t)).unwrap();
    assert_eq!(v["network"]["interfaces"], serde_json::json!([]));
    assert_eq!(v["network"]["truncated"], false);
    assert_eq!(
        v["network"]["aggregate_rx_bytes_per_sec"],
        serde_json::Value::Null
    );
}

#[test]
fn json_gpu_supported_empty_gpus() {
    let mut t = archive(CAP_GPU_ENUMERATION);
    t.gpu.gpu_count = 0;
    t.gpu.nvml_available = 1;
    let v: serde_json::Value = serde_json::from_str(&json(Module::Gpu, &t)).unwrap();
    assert_eq!(v["gpu"]["gpus"], serde_json::json!([]));
    assert_eq!(v["gpu"]["nvml_available"], true);
    assert_eq!(v["gpu"]["gpu_truncated"], false);
}

#[test]
fn json_gpu_partial_record_nulls_only_unowned_metrics() {
    let mut t = gpu_archive();
    t.gpu.gpus[0].capabilities = ALL_GPU_RECORD & !GPU_CAP_POWER;
    let v: serde_json::Value = serde_json::from_str(&json(Module::Gpu, &t)).unwrap();
    let rec = &v["gpu"]["gpus"][0];
    assert_eq!(rec["power_watts"], serde_json::Value::Null);
    assert_eq!(rec["name"], "nv");
    assert_eq!(rec["utilization_percent"], 55.5);
    assert_eq!(rec["temperature_celsius"], 85);
    assert_eq!(rec["tone"], "red");
    assert_eq!(rec["available"], true);
    assert_eq!(rec["capabilities"]["power"], false);
    assert_eq!(rec["capabilities"]["name"], true);
}

#[test]
fn json_gpu_tone_null_without_temperature_capability() {
    let mut t = gpu_archive();
    t.gpu.gpus[0].capabilities = ALL_GPU_RECORD & !GPU_CAP_TEMPERATURE;
    let v: serde_json::Value = serde_json::from_str(&json(Module::Gpu, &t)).unwrap();
    let rec = &v["gpu"]["gpus"][0];
    assert_eq!(rec["tone"], serde_json::Value::Null);
    assert_eq!(rec["temperature_celsius"], serde_json::Value::Null);
}

#[test]
fn json_gpu_tone_strings_cover_all_four_tones() {
    let mut t = archive(CAP_GPU_ENUMERATION);
    let tones = [TONE_GREEN, TONE_MAGENTA, TONE_YELLOW, TONE_RED];
    for (idx, tone) in tones.iter().enumerate() {
        t.gpu.gpus[idx] = gpu_record(*tone);
    }
    t.gpu.gpu_count = 4;
    let v: serde_json::Value = serde_json::from_str(&json(Module::Gpu, &t)).unwrap();
    let names = ["green", "magenta", "yellow", "red"];
    for (idx, name) in names.iter().enumerate() {
        assert_eq!(v["gpu"]["gpus"][idx]["tone"], *name);
    }
}

#[test]
fn json_top_memory_records_never_expose_cpu_usage() {
    let t = process_archive();
    let out = json(Module::Process, &t);
    let section = out.split("\"top_memory\": [").nth(1).unwrap();
    let record = section.split(']').next().unwrap();
    assert!(!record.contains("cpu_usage"));
    let keys = ["\"pid\":", "\"memory_bytes\":", "\"comm\":"];
    let mut last = 0;
    for key in keys {
        let pos = record[last..]
            .find(key)
            .unwrap_or_else(|| panic!("missing {key}"))
            + last;
        assert!(pos >= last, "{key} out of order");
        last = pos;
    }
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    let rec = &v["process"]["top_memory"][0];
    assert_eq!(rec["pid"], 43);
    assert_eq!(rec["memory_bytes"], 2000);
    assert_eq!(rec["comm"], "mem");
}

#[test]
fn json_top_cpu_record_exact_fields() {
    let t = process_archive();
    let v: serde_json::Value = serde_json::from_str(&json(Module::Process, &t)).unwrap();
    let rec = &v["process"]["top_cpu"][0];
    assert_eq!(rec["pid"], 42);
    assert_eq!(rec["cpu_usage"], 50.0);
    assert_eq!(rec["memory_bytes"], 1000);
    assert_eq!(rec["comm"], "top");
}

#[test]
fn json_process_truncated_boolean_or_owned() {
    let mut t = archive(CAP_PROCESS_RUNNING);
    t.process.flags = PROCESS_TRUNCATED;
    let v: serde_json::Value = serde_json::from_str(&json(Module::Process, &t)).unwrap();
    assert_eq!(v["process"]["truncated"], true);

    let t2 = archive(CAP_PROCESS_RUNNING);
    let v2: serde_json::Value = serde_json::from_str(&json(Module::Process, &t2)).unwrap();
    assert_eq!(v2["process"]["truncated"], false);

    let t3 = archive(0);
    let v3: serde_json::Value = serde_json::from_str(&json(Module::Process, &t3)).unwrap();
    assert_eq!(v3["process"]["truncated"], serde_json::Value::Null);
}

#[test]
fn json_meta_timestamp_always_integer() {
    let t = archive(0);
    let v: serde_json::Value = serde_json::from_str(&json(Module::Os, &t)).unwrap();
    assert_eq!(v["meta"]["timestamp_ns"], 1);
    assert!(v["meta"]["timestamp_ns"].is_u64());
}

#[test]
fn json_meta_supported_exact_schema_order() {
    let t = meta_archive();
    let out = json(Module::Os, &t);
    let meta_pos = out.find("\"meta\":").unwrap();
    let keys = [
        "\"timestamp_ns\":",
        "\"wallclock_ns\":",
        "\"uptime_secs\":",
        "\"load_avg_1m\":",
        "\"load_avg_5m\":",
        "\"load_avg_15m\":",
        "\"timezone_name\":",
        "\"timezone_offset_secs\":",
        "\"os\":",
        "\"os_type\":",
        "\"os_id\":",
        "\"version\":",
        "\"version_id\":",
        "\"version_codename\":",
        "\"pretty_name\":",
    ];
    let mut last = meta_pos;
    for key in keys {
        let pos = out[last..]
            .find(key)
            .unwrap_or_else(|| panic!("missing {key}"))
            + last;
        assert!(pos >= last, "{key} out of order");
        last = pos;
    }
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["meta"]["wallclock_ns"], 1_700_000_000_000_000_000u64);
    assert_eq!(v["meta"]["load_avg_1m"], 0.5);
    assert_eq!(v["meta"]["timezone_offset_secs"], 0);
    assert_eq!(v["meta"]["os"]["pretty_name"], "Ubuntu 24.04");
    assert_eq!(v["meta"]["os"]["version"], "24.04");
}

#[test]
fn json_memory_schema_and_derived_gating() {
    let t = memory_archive();
    let v: serde_json::Value = serde_json::from_str(&json(Module::Mem, &t)).unwrap();
    assert_eq!(v["memory"]["ram_total"], 16_000_000_000u64);
    assert_eq!(v["memory"]["ram_used_percent"], 50.0);
    assert_eq!(v["memory"]["swap_used_percent"], 50.0);
    assert_eq!(v["memory"]["page_faults"], 9);
    assert_eq!(v["memory"]["page_faults_per_sec"], 7.5);

    let partial = archive(CAP_MEMORY_RAM_USED);
    let v2: serde_json::Value = serde_json::from_str(&json(Module::Mem, &partial)).unwrap();
    assert_eq!(v2["memory"]["ram_used"], 0);
    assert_eq!(v2["memory"]["ram_used_percent"], serde_json::Value::Null);
}

#[test]
fn json_storage_record_field_ownership() {
    let t = storage_archive();
    let v: serde_json::Value = serde_json::from_str(&json(Module::Disk, &t)).unwrap();
    let d = &v["storage"]["disks"][0];
    assert_eq!(d["name"], "sda");
    assert_eq!(d["major"], 8);
    assert_eq!(d["read_bytes_per_sec"], 1500.0);
    assert_eq!(d["read_iops"], 10.5);
    assert_eq!(d["queue_depth"], 3);
    assert_eq!(d["read_latency_ms"], 1.5);
    let m = &v["storage"]["mounts"][0];
    assert_eq!(m["mountpoint"], "/");
    assert_eq!(m["fstype"], "ext4");
    assert_eq!(m["percent"], 75.0);
}

#[test]
fn json_network_aggregate_requires_bytes_and_rates() {
    let t = network_archive();
    let v: serde_json::Value = serde_json::from_str(&json(Module::Net, &t)).unwrap();
    assert_eq!(v["network"]["aggregate_rx_bytes_per_sec"], 1500.0);
    assert_eq!(v["network"]["aggregate_tx_bytes_per_sec"], 2500.0);
    let iface = &v["network"]["interfaces"][0];
    assert_eq!(iface["name"], "eth0");
    assert_eq!(iface["rx_bytes_per_sec"], 1000.0);
}

#[test]
fn json_cpu_per_core_records() {
    let t = cpu_archive();
    let v: serde_json::Value = serde_json::from_str(&json(Module::Cpu, &t)).unwrap();
    let cores = v["cpu"]["cores"].as_array().unwrap();
    assert_eq!(cores.len(), 2);
    assert_eq!(cores[0]["core_index"], 0);
    assert_eq!(cores[0]["usage_percent"], 10.0);
    assert_eq!(cores[1]["usage_percent"], 20.5);
    assert_eq!(v["cpu"]["context_switches"], 500);
}

// ------------------------------------------------------ binary end-to-end ---

fn temp_shm_dir(tag: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!(
        "aura-output-contract-{tag}-{}-{nanos}",
        std::process::id()
    ));
    std::fs::DirBuilder::new().mode(0o700).create(&dir).unwrap();
    // macOS temp dirs live under /var, a symlink the SHM security layer rejects.
    std::fs::canonicalize(&dir).unwrap()
}

fn write_shm(path: &Path, archive: &TelemetryArchive) {
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)
        .unwrap();
    file.set_len(SHM_SIZE as u64).unwrap();
    // SAFETY: the temp file was just sized to `SHM_SIZE`, matching the mapping length.
    let mut mmap = unsafe { MmapOptions::new().len(SHM_SIZE).map_mut(&file).unwrap() };
    let mut snapshot = *archive;
    snapshot.checksum = snapshot.calculate_checksum();
    // SAFETY: the mapping is a full writable SHM region and the snapshot is initialized.
    unsafe {
        write_double_buffer(mmap.as_mut_ptr(), &snapshot).expect("publish snapshot");
    }
    mmap.flush().unwrap();
}

#[test]
fn binary_stdout_newline_empty_stderr_exit_zero() {
    let dir = temp_shm_dir("bin");
    let shm = dir.join("state.dat");
    let mut t = cpu_archive();
    t.meta.timestamp_ns = monotonic_ns();
    write_shm(&shm, &t);

    let out = std::process::Command::new(env!("CARGO_BIN_EXE_aura-cli"))
        .args(["-m", "cpu", "--color", "none", "--shm-path"])
        .arg(&shm)
        .output()
        .unwrap();
    let _ = std::fs::remove_dir_all(&dir);

    assert_eq!(out.status.code(), Some(0));
    assert!(out.stderr.is_empty());
    let expected = format!(
        "{}\n",
        "CPU\n  usage: 42.0%\n  context switches/s: 1234.5\n  cores:\n    cpu0: 10.0%\n    cpu1: 20.5%"
    );
    assert_eq!(String::from_utf8(out.stdout).unwrap(), expected);
}

#[test]
fn binary_value_format_exact_row() {
    let dir = temp_shm_dir("bin-value");
    let shm = dir.join("state.dat");
    let mut t = cpu_archive();
    t.meta.timestamp_ns = monotonic_ns();
    write_shm(&shm, &t);

    let out = std::process::Command::new(env!("CARGO_BIN_EXE_aura-cli"))
        .args(["-m", "cpu", "--format", "value", "--shm-path"])
        .arg(&shm)
        .output()
        .unwrap();
    let _ = std::fs::remove_dir_all(&dir);

    assert_eq!(out.status.code(), Some(0));
    assert!(out.stderr.is_empty());
    assert_eq!(String::from_utf8(out.stdout).unwrap(), "cpu=42.0%\n");
}

#[test]
fn binary_value_os_exact_row() {
    let dir = temp_shm_dir("bin-value-os");
    let shm = dir.join("state.dat");
    let mut t = meta_archive();
    t.meta.timestamp_ns = monotonic_ns();
    write_shm(&shm, &t);

    let out = std::process::Command::new(env!("CARGO_BIN_EXE_aura-cli"))
        .args([
            "-m",
            "os",
            "--format",
            "value",
            "--color",
            "none",
            "--shm-path",
        ])
        .arg(&shm)
        .output()
        .unwrap();
    let _ = std::fs::remove_dir_all(&dir);

    assert_eq!(out.status.code(), Some(0));
    assert!(out.stderr.is_empty());
    assert_eq!(String::from_utf8(out.stdout).unwrap(), "os=\u{f31b}\n");
}

#[test]
fn binary_alias_module_accepted() {
    let dir = temp_shm_dir("bin-alias");
    let shm = dir.join("state.dat");
    let mut t = meta_archive();
    t.meta.timestamp_ns = monotonic_ns();
    write_shm(&shm, &t);

    let out = std::process::Command::new(env!("CARGO_BIN_EXE_aura-cli"))
        .args(["-m", "meta", "--color", "none", "--shm-path"])
        .arg(&shm)
        .output()
        .unwrap();
    let _ = std::fs::remove_dir_all(&dir);

    assert_eq!(out.status.code(), Some(0));
    assert!(String::from_utf8(out.stdout).unwrap().starts_with("META\n"));
}

// ------------------------------------------------------------------- raw ---

fn run_binary_raw(module: &str, t: &TelemetryArchive, tag: &str) -> std::process::Output {
    let dir = temp_shm_dir(tag);
    let shm = dir.join("state.dat");
    let mut snapshot = *t;
    snapshot.meta.timestamp_ns = monotonic_ns();
    write_shm(&shm, &snapshot);

    let out = std::process::Command::new(env!("CARGO_BIN_EXE_aura-cli"))
        .args(["--format", "raw", "-m", module, "--shm-path"])
        .arg(&shm)
        .output()
        .unwrap();
    let _ = std::fs::remove_dir_all(&dir);
    out
}

fn assert_raw_exact(module: &str, t: &TelemetryArchive, tag: &str, expected_row: &str) {
    let out = run_binary_raw(module, t, tag);
    assert_eq!(
        out.status.code(),
        Some(0),
        "{tag}: exit; stdout={:?} stderr={:?}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(out.stderr.is_empty(), "{tag}: stderr must stay empty");
    assert_eq!(
        String::from_utf8(out.stdout).unwrap(),
        format!("{expected_row}\n"),
        "{tag}: exact stdout with one trailing newline"
    );
}

type RawCase = (&'static str, fn() -> TelemetryArchive, &'static str);
type RawNaCase = (&'static str, &'static str, fn() -> TelemetryArchive);

#[test]
fn raw_binary_supported_all_modules_exact() {
    let cases: [RawCase; 8] = [
        ("cpu", cpu_archive, "42.0%"),
        ("process", process_reader_archive, "50.0%"),
        ("mem", memory_archive, "50.0%"),
        ("swap", memory_archive, "50.0%"),
        ("disk", storage_archive, "read=1.5KB/s,write=2.0MB/s"),
        ("net", network_archive, "rx=1.5KB/s,tx=2.5KB/s"),
        ("os", meta_archive, "\u{f31b}"),
        ("gpu", gpu_archive, "55.5%"),
    ];
    for (module, build, expected_row) in cases {
        assert_raw_exact(module, &build(), &format!("raw-ok-{module}"), expected_row);
    }
}

// The binary path runs full archive validation, so every raw fixture must be
// reader-valid: capability-gated fields are zeroed when their owner bit is
// clear, and CAP_PROCESS_TOP_CPU requires CAP_CPU_GLOBAL with core_count >= 1.

fn process_reader_archive() -> TelemetryArchive {
    let mut t = process_archive();
    let cpu = cpu_archive();
    t.capabilities |= cpu.capabilities;
    t.cpu = cpu.cpu;
    for record in t.process.top_mem.iter_mut() {
        record.cpu_usage = 0.0;
    }
    t
}

fn mem_total_only_archive() -> TelemetryArchive {
    let mut t = archive(CAP_MEMORY_RAM_TOTAL);
    t.memory.ram_total = 16_000_000_000;
    t
}

fn swap_cap_clear_archive() -> TelemetryArchive {
    let mut t = memory_archive();
    t.capabilities &= !CAP_MEMORY_SWAP;
    t.memory.swap_total = 0;
    t.memory.swap_free = 0;
    t.memory.swap_used = 0;
    t.derived.swap_used_percent = 0.0;
    t.derived.swap_tone = 0;
    t
}

fn disk_bytes_only_archive() -> TelemetryArchive {
    let mut t = archive(CAP_STORAGE_DISK_BYTES);
    let mut disk = storage_archive().storage.disks[0];
    disk.read_bytes_per_sec = 0.0;
    disk.write_bytes_per_sec = 0.0;
    disk.read_iops = 0.0;
    disk.write_iops = 0.0;
    disk.queue_depth = 0;
    disk.read_latency_ms = 0.0;
    disk.write_latency_ms = 0.0;
    t.storage.disks[0] = disk;
    t.storage.disk_count = 1;
    t
}

fn disk_empty_archive() -> TelemetryArchive {
    let mut t = storage_archive();
    t.storage.disk_count = 0;
    for disk in t.storage.disks.iter_mut() {
        // SAFETY: `DiskStat` is `Zeroable`; the all-zero bit pattern is valid.
        *disk = unsafe { std::mem::zeroed() };
    }
    t
}

fn net_bytes_only_archive() -> TelemetryArchive {
    let mut t = archive(CAP_NETWORK_BYTES);
    let mut iface = network_archive().network.interfaces[0];
    iface.rx_bytes_per_sec = 0.0;
    iface.tx_bytes_per_sec = 0.0;
    t.network.interfaces[0] = iface;
    t.network.if_count = 1;
    t
}

fn net_empty_archive() -> TelemetryArchive {
    let mut t = network_archive();
    t.network.if_count = 0;
    for iface in t.network.interfaces.iter_mut() {
        *iface = NetIfStat::new();
    }
    t
}

fn os_identity_clear_archive() -> TelemetryArchive {
    let mut t = meta_archive();
    t.capabilities &= !CAP_META_OS_IDENTITY;
    t.meta.os.os_type = FixedString16::new();
    t.meta.os.os_id = FixedString16::new();
    t.meta.os.os_pretty_name = [0; 128];
    t
}

fn gpu_util_clear_archive() -> TelemetryArchive {
    let mut t = gpu_archive();
    t.gpu.gpus[0].capabilities &= !GPU_CAP_UTILIZATION;
    t.gpu.gpus[0].utilization_percent = 0.0;
    t
}

fn gpu_empty_archive() -> TelemetryArchive {
    let mut t = gpu_archive();
    t.gpu.gpu_count = 0;
    for gpu in t.gpu.gpus.iter_mut() {
        // SAFETY: `GpuStat` is `Zeroable`; the all-zero bit pattern is valid.
        *gpu = unsafe { std::mem::zeroed() };
    }
    t
}

#[test]
fn raw_binary_unsupported_is_na() {
    let cases: [RawNaCase; 13] = [
        ("cpu", "cpu-cap", || archive(0)),
        ("process", "process-cap", || archive(0)),
        ("process", "process-empty", || {
            let mut t = process_reader_archive();
            t.process.top_cpu_count = 0;
            t
        }),
        ("mem", "mem-used-clear", mem_total_only_archive),
        ("swap", "swap-cap", swap_cap_clear_archive),
        ("disk", "disk-rates-clear", disk_bytes_only_archive),
        ("disk", "disk-empty", disk_empty_archive),
        ("net", "net-rates-clear", net_bytes_only_archive),
        ("net", "net-empty", net_empty_archive),
        ("os", "os-identity-clear", os_identity_clear_archive),
        ("gpu", "gpu-enum-clear", || archive(0)),
        ("gpu", "gpu-util-clear", gpu_util_clear_archive),
        ("gpu", "gpu-empty", gpu_empty_archive),
    ];
    for (module, tag, build) in cases {
        assert_raw_exact(module, &build(), &format!("raw-na-{tag}"), "N/A");
    }
}
