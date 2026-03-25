use aura_common::{AuraError, AuraResult, TelemetryArchive};
use serde::Serialize;

use crate::Module;

#[derive(Serialize)]
pub struct TelemetryJson {
    version: u64,
    cpu: Option<CpuGlobalStatJson>,
    process: Option<ProcessStatsJson>,
    memory: Option<MemoryStatsJson>,
    storage: Option<StorageStatsJson>,
    network: Option<NetworkStatsJson>,
    meta: Option<MetaStatsJson>,
    gpu: Option<GpuStatsJson>,
}

#[derive(Serialize)]
struct CpuGlobalStatJson {
    user_ticks: u64,
    system_ticks: u64,
    idle_ticks: u64,
    total_ticks: u64,
    context_switches: u64,
    context_switches_per_sec: f32,
    usage_percent: f32,
    cores: Vec<CpuCoreStatJson>,
}

#[derive(Serialize)]
struct CpuCoreStatJson {
    core_index: u8,
    user_ticks: u64,
    system_ticks: u64,
    idle_ticks: u64,
    total_ticks: u64,
    usage_percent: f32,
}

#[derive(Serialize)]
struct ProcessStatsJson {
    total: u32,
    running: u32,
    blocked: u32,
    sleeping: u32,
    top_cpu: Vec<ProcessStatJson>,
    top_mem: Vec<ProcessStatJson>,
}

#[derive(Serialize)]
struct ProcessStatJson {
    pid: u32,
    cpu_usage: f32,
    memory_bytes: u64,
    comm: String,
}

#[derive(Serialize)]
struct MemoryStatsJson {
    ram_total: u64,
    ram_free: u64,
    ram_used: u64,
    buffers: u64,
    cached: u64,
    swap_total: u64,
    swap_free: u64,
    swap_used: u64,
    page_faults: u64,
    page_faults_per_sec: f32,
}

#[derive(Serialize)]
struct StorageStatsJson {
    disks: Vec<DiskStatJson>,
    mounts: Vec<MountStatJson>,
}

#[derive(Serialize)]
struct DiskStatJson {
    name: String,
    rx_bytes: u64,
    wx_bytes: u64,
    rx_per_sec: f32,
    wx_per_sec: f32,
}

#[derive(Serialize)]
struct MountStatJson {
    mountpoint: String,
    fstype: String,
    total: u64,
    available: u64,
    used: u64,
    percent: f32,
}

#[derive(Serialize)]
struct NetworkStatsJson {
    interfaces: Vec<NetIfStatJson>,
}

#[derive(Serialize)]
struct NetIfStatJson {
    name: String,
    rx_bytes: u64,
    tx_bytes: u64,
    rx_bytes_per_sec: f32,
    tx_bytes_per_sec: f32,
}

#[derive(Serialize)]
struct MetaStatsJson {
    timestamp_ns: u64,
    uptime_secs: u64,
    load_avg_1m: f32,
    load_avg_5m: f32,
    load_avg_15m: f32,
    timezone_name: String,
    timezone_offset_secs: i32,
    os: OsFingerprintJson,
}

#[derive(Serialize)]
struct OsFingerprintJson {
    os_type: String,
    os_id: String,
    os_version_id: String,
    os_pretty_name: String,
}

#[derive(Serialize)]
struct GpuStatsJson {
    nvml_available: bool,
    gpus: Vec<GpuStatJson>,
}

#[derive(Serialize)]
struct GpuStatJson {
    name: String,
    memory_total: u64,
    memory_used: u64,
    utilization_percent: f32,
    power_watts: f32,
    temperature_celsius: i16,
    available: bool,
}

impl TelemetryJson {
    fn from_telemetry(module: Module, telemetry: &TelemetryArchive) -> Self {
        let include_cpu = matches!(module, Module::All | Module::Cpu);
        let include_mem = matches!(module, Module::All | Module::Mem | Module::Swap);
        let include_disk = matches!(module, Module::All | Module::Disk);
        let include_net = matches!(module, Module::All | Module::Net);
        let include_meta = matches!(module, Module::All | Module::Os);
        let include_rest = matches!(module, Module::All);

        Self {
            version: telemetry.version,
            cpu: include_cpu.then(|| cpu_to_json(telemetry)),
            process: include_rest.then(|| process_to_json(telemetry)),
            memory: include_mem.then(|| memory_to_json(telemetry)),
            storage: include_disk.then(|| storage_to_json(telemetry)),
            network: include_net.then(|| network_to_json(telemetry)),
            meta: include_meta.then(|| meta_to_json(telemetry)),
            gpu: include_rest.then(|| gpu_to_json(telemetry)),
        }
    }
}

pub fn render(module: Module, telemetry: &TelemetryArchive) -> AuraResult<String> {
    let json = TelemetryJson::from_telemetry(module, telemetry);
    serde_json::to_string_pretty(&json)
        .map_err(|e| AuraError::ParseError(format!("failed to serialize JSON output: {e}")))
}

fn cpu_to_json(telemetry: &TelemetryArchive) -> CpuGlobalStatJson {
    let cpu = &telemetry.cpu;
    CpuGlobalStatJson {
        user_ticks: cpu.user_ticks,
        system_ticks: cpu.system_ticks,
        idle_ticks: cpu.idle_ticks,
        total_ticks: cpu.total_ticks,
        context_switches: cpu.context_switches,
        context_switches_per_sec: cpu.context_switches_per_sec,
        usage_percent: cpu.usage_percent,
        cores: (0..cpu.core_count as usize)
            .map(|idx| {
                let core = &cpu.cores[idx];
                CpuCoreStatJson {
                    core_index: core.core_index,
                    user_ticks: core.user_ticks,
                    system_ticks: core.system_ticks,
                    idle_ticks: core.idle_ticks,
                    total_ticks: core.total_ticks,
                    usage_percent: core.usage_percent,
                }
            })
            .collect(),
    }
}

fn process_to_json(telemetry: &TelemetryArchive) -> ProcessStatsJson {
    let process = &telemetry.process;
    ProcessStatsJson {
        total: process.total,
        running: process.running,
        blocked: process.blocked,
        sleeping: process.sleeping,
        top_cpu: process.top_cpu.iter().map(process_stat_to_json).collect(),
        top_mem: process.top_mem.iter().map(process_stat_to_json).collect(),
    }
}

fn process_stat_to_json(stat: &aura_common::ProcessStat) -> ProcessStatJson {
    ProcessStatJson {
        pid: stat.pid,
        cpu_usage: stat.cpu_usage,
        memory_bytes: stat.memory_bytes,
        comm: stat.comm.as_str().to_string(),
    }
}

fn memory_to_json(telemetry: &TelemetryArchive) -> MemoryStatsJson {
    let memory = &telemetry.memory;
    MemoryStatsJson {
        ram_total: memory.ram_total,
        ram_free: memory.ram_free,
        ram_used: memory.ram_used,
        buffers: memory.buffers,
        cached: memory.cached,
        swap_total: memory.swap_total,
        swap_free: memory.swap_free,
        swap_used: memory.swap_used,
        page_faults: memory.page_faults,
        page_faults_per_sec: memory.page_faults_per_sec,
    }
}

fn storage_to_json(telemetry: &TelemetryArchive) -> StorageStatsJson {
    let storage = &telemetry.storage;
    StorageStatsJson {
        disks: (0..storage.disk_count as usize)
            .map(|idx| {
                let disk = &storage.disks[idx];
                DiskStatJson {
                    name: disk.name.as_str().to_string(),
                    rx_bytes: disk.rx_bytes,
                    wx_bytes: disk.wx_bytes,
                    rx_per_sec: disk.rx_per_sec,
                    wx_per_sec: disk.wx_per_sec,
                }
            })
            .collect(),
        mounts: (0..storage.mount_count as usize)
            .map(|idx| {
                let mount = &storage.mounts[idx];
                MountStatJson {
                    mountpoint: trim_zero_terminated(&mount.mountpoint),
                    fstype: mount.fstype.as_str().to_string(),
                    total: mount.total,
                    available: mount.available,
                    used: mount.used,
                    percent: mount.percent,
                }
            })
            .collect(),
    }
}

fn network_to_json(telemetry: &TelemetryArchive) -> NetworkStatsJson {
    let network = &telemetry.network;
    NetworkStatsJson {
        interfaces: (0..network.if_count as usize)
            .map(|idx| {
                let iface = &network.interfaces[idx];
                NetIfStatJson {
                    name: iface.name.as_str().to_string(),
                    rx_bytes: iface.rx_bytes,
                    tx_bytes: iface.tx_bytes,
                    rx_bytes_per_sec: iface.rx_bytes_per_sec,
                    tx_bytes_per_sec: iface.tx_bytes_per_sec,
                }
            })
            .collect(),
    }
}

fn meta_to_json(telemetry: &TelemetryArchive) -> MetaStatsJson {
    let meta = &telemetry.meta;
    MetaStatsJson {
        timestamp_ns: meta.timestamp_ns,
        uptime_secs: meta.uptime_secs,
        load_avg_1m: meta.load_avg_1m,
        load_avg_5m: meta.load_avg_5m,
        load_avg_15m: meta.load_avg_15m,
        timezone_name: trim_zero_terminated(&meta.timezone_name),
        timezone_offset_secs: meta.timezone_offset_secs,
        os: OsFingerprintJson {
            os_type: meta.os.os_type.as_str().to_string(),
            os_id: meta.os.os_id.as_str().to_string(),
            os_version_id: meta.os.os_version_id.as_str().to_string(),
            os_pretty_name: trim_zero_terminated(&meta.os.os_pretty_name),
        },
    }
}

fn gpu_to_json(telemetry: &TelemetryArchive) -> GpuStatsJson {
    let gpu = &telemetry.gpu;
    GpuStatsJson {
        nvml_available: gpu.nvml_available,
        gpus: (0..gpu.gpu_count as usize)
            .map(|idx| {
                let item = &gpu.gpus[idx];
                GpuStatJson {
                    name: item.name.as_str().to_string(),
                    memory_total: item.memory_total,
                    memory_used: item.memory_used,
                    utilization_percent: item.utilization_percent,
                    power_watts: item.power_watts,
                    temperature_celsius: item.temperature_celsius,
                    available: item.available,
                }
            })
            .collect(),
    }
}

fn trim_zero_terminated(bytes: &[u8]) -> String {
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    String::from_utf8_lossy(&bytes[..end]).to_string()
}
