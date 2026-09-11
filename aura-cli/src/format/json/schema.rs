use serde::Serialize;

#[derive(Serialize)]
pub struct TelemetryJson {
    pub(super) version: u64,
    pub(super) capabilities: CapabilitiesJson,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) cpu: Option<CpuJson>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) process: Option<ProcessJson>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) memory: Option<MemoryJson>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) storage: Option<StorageJson>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) network: Option<NetworkJson>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) meta: Option<MetaJson>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) gpu: Option<GpuJson>,
}

#[derive(Serialize)]
pub(super) struct CapabilitiesJson {
    pub(super) cpu_global: bool,
    pub(super) cpu_per_core: bool,
    pub(super) cpu_context_switches: bool,
    pub(super) process_total: bool,
    pub(super) process_running: bool,
    pub(super) process_blocked: bool,
    pub(super) process_sleeping: bool,
    pub(super) process_top_cpu: bool,
    pub(super) process_top_memory: bool,
    pub(super) memory_ram_total: bool,
    pub(super) memory_ram_free: bool,
    pub(super) memory_ram_used: bool,
    pub(super) memory_buffers: bool,
    pub(super) memory_cached: bool,
    pub(super) memory_swap: bool,
    pub(super) memory_page_faults: bool,
    pub(super) storage_disk_bytes: bool,
    pub(super) storage_disk_rates: bool,
    pub(super) storage_disk_iops: bool,
    pub(super) storage_disk_queue_depth: bool,
    pub(super) storage_disk_latency: bool,
    pub(super) storage_mounts: bool,
    pub(super) network_bytes: bool,
    pub(super) network_rates: bool,
    pub(super) meta_uptime: bool,
    pub(super) meta_load_average: bool,
    pub(super) meta_timezone: bool,
    pub(super) meta_os_identity: bool,
    pub(super) meta_os_version: bool,
    pub(super) meta_os_version_id: bool,
    pub(super) meta_os_codename: bool,
    pub(super) meta_wallclock: bool,
    pub(super) gpu_enumeration: bool,
}

#[derive(Serialize)]
pub(super) struct CpuJson {
    pub(super) user_ticks: Option<u64>,
    pub(super) system_ticks: Option<u64>,
    pub(super) idle_ticks: Option<u64>,
    pub(super) total_ticks: Option<u64>,
    pub(super) context_switches: Option<u64>,
    pub(super) context_switches_per_sec: Option<f32>,
    pub(super) usage_percent: Option<f32>,
    pub(super) cores: Option<Vec<CpuCoreJson>>,
}

#[derive(Serialize)]
pub(super) struct CpuCoreJson {
    pub(super) core_index: u8,
    pub(super) user_ticks: u64,
    pub(super) system_ticks: u64,
    pub(super) idle_ticks: u64,
    pub(super) total_ticks: u64,
    pub(super) usage_percent: f32,
}

#[derive(Serialize)]
pub(super) struct ProcessJson {
    pub(super) total: Option<u32>,
    pub(super) running: Option<u32>,
    pub(super) blocked: Option<u32>,
    pub(super) sleeping: Option<u32>,
    pub(super) truncated: Option<bool>,
    pub(super) top_cpu: Option<Vec<TopCpuJson>>,
    pub(super) top_memory: Option<Vec<TopMemoryJson>>,
}

#[derive(Serialize)]
pub(super) struct TopCpuJson {
    pub(super) pid: u32,
    pub(super) cpu_usage: f32,
    pub(super) memory_bytes: u64,
    pub(super) comm: String,
}

#[derive(Serialize)]
pub(super) struct TopMemoryJson {
    pub(super) pid: u32,
    pub(super) memory_bytes: u64,
    pub(super) comm: String,
}

#[derive(Serialize)]
pub(super) struct MemoryJson {
    pub(super) ram_total: Option<u64>,
    pub(super) ram_free: Option<u64>,
    pub(super) ram_used: Option<u64>,
    pub(super) ram_used_percent: Option<f32>,
    pub(super) buffers: Option<u64>,
    pub(super) cached: Option<u64>,
    pub(super) swap_total: Option<u64>,
    pub(super) swap_free: Option<u64>,
    pub(super) swap_used: Option<u64>,
    pub(super) swap_used_percent: Option<f32>,
    pub(super) page_faults: Option<u64>,
    pub(super) page_faults_per_sec: Option<f32>,
}

#[derive(Serialize)]
pub(super) struct StorageJson {
    pub(super) disk_truncated: Option<bool>,
    pub(super) mount_truncated: Option<bool>,
    pub(super) disks: Option<Vec<DiskJson>>,
    pub(super) mounts: Option<Vec<MountJson>>,
}

#[derive(Serialize)]
pub(super) struct DiskJson {
    pub(super) name: Option<String>,
    pub(super) major: Option<u32>,
    pub(super) minor: Option<u32>,
    pub(super) read_bytes: Option<u64>,
    pub(super) write_bytes: Option<u64>,
    pub(super) read_bytes_per_sec: Option<f32>,
    pub(super) write_bytes_per_sec: Option<f32>,
    pub(super) read_iops: Option<f32>,
    pub(super) write_iops: Option<f32>,
    pub(super) queue_depth: Option<u32>,
    pub(super) read_latency_ms: Option<f32>,
    pub(super) write_latency_ms: Option<f32>,
}

#[derive(Serialize)]
pub(super) struct MountJson {
    pub(super) mountpoint: String,
    pub(super) fstype: String,
    pub(super) total: u64,
    pub(super) available: u64,
    pub(super) used: u64,
    pub(super) percent: f32,
}

#[derive(Serialize)]
pub(super) struct NetworkJson {
    pub(super) truncated: Option<bool>,
    pub(super) aggregate_rx_bytes_per_sec: Option<f32>,
    pub(super) aggregate_tx_bytes_per_sec: Option<f32>,
    pub(super) interfaces: Option<Vec<NetIfJson>>,
}

#[derive(Serialize)]
pub(super) struct NetIfJson {
    pub(super) name: Option<String>,
    pub(super) rx_bytes: Option<u64>,
    pub(super) tx_bytes: Option<u64>,
    pub(super) rx_bytes_per_sec: Option<f32>,
    pub(super) tx_bytes_per_sec: Option<f32>,
}

#[derive(Serialize)]
pub(super) struct MetaJson {
    pub(super) timestamp_ns: u64,
    pub(super) wallclock_ns: Option<u64>,
    pub(super) uptime_secs: Option<u64>,
    pub(super) load_avg_1m: Option<f32>,
    pub(super) load_avg_5m: Option<f32>,
    pub(super) load_avg_15m: Option<f32>,
    pub(super) timezone_name: Option<String>,
    pub(super) timezone_offset_secs: Option<i32>,
    pub(super) os: OsJson,
}

#[derive(Serialize)]
pub(super) struct OsJson {
    pub(super) os_type: Option<String>,
    pub(super) os_id: Option<String>,
    pub(super) version: Option<String>,
    pub(super) version_id: Option<String>,
    pub(super) version_codename: Option<String>,
    pub(super) pretty_name: Option<String>,
}

#[derive(Serialize)]
pub(super) struct GpuJson {
    pub(super) nvml_available: Option<bool>,
    pub(super) gpu_truncated: Option<bool>,
    pub(super) gpus: Option<Vec<GpuRecordJson>>,
}

#[derive(Serialize)]
pub(super) struct GpuRecordJson {
    pub(super) available: bool,
    pub(super) name: Option<String>,
    pub(super) memory_total: Option<u64>,
    pub(super) memory_used: Option<u64>,
    pub(super) utilization_percent: Option<f32>,
    pub(super) power_watts: Option<f32>,
    pub(super) temperature_celsius: Option<i16>,
    pub(super) tone: Option<&'static str>,
    pub(super) capabilities: GpuRecordCapsJson,
}

#[derive(Serialize)]
pub(super) struct GpuRecordCapsJson {
    pub(super) name: bool,
    pub(super) memory_total: bool,
    pub(super) memory_used: bool,
    pub(super) utilization: bool,
    pub(super) power: bool,
    pub(super) temperature: bool,
}
