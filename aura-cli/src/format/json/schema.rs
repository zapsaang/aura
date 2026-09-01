use serde::Serialize;

#[derive(Serialize)]
pub struct TelemetryJson {
    pub(super) version: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) cpu: Option<CpuGlobalStatJson>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) process: Option<ProcessStatsJson>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) memory: Option<MemoryStatsJson>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) network: Option<NetworkStatsJson>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) meta: Option<MetaStatsJson>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) gpu: Option<GpuStatsJson>,
}

#[derive(Serialize)]
pub(super) struct CpuGlobalStatJson {
    pub(super) user_ticks: u64,
    pub(super) system_ticks: u64,
    pub(super) idle_ticks: u64,
    pub(super) total_ticks: u64,
    pub(super) context_switches: u64,
    pub(super) context_switches_per_sec: f32,
    pub(super) usage_percent: f32,
    pub(super) cores: Vec<CpuCoreStatJson>,
}

#[derive(Serialize)]
pub(super) struct CpuCoreStatJson {
    pub(super) core_index: u8,
    pub(super) user_ticks: u64,
    pub(super) system_ticks: u64,
    pub(super) idle_ticks: u64,
    pub(super) total_ticks: u64,
    pub(super) usage_percent: f32,
}

#[derive(Serialize)]
pub(super) struct ProcessStatsJson {
    pub(super) total: u32,
    pub(super) running: u32,
    pub(super) blocked: u32,
    pub(super) sleeping: u32,
    pub(super) top_cpu: Vec<ProcessStatJson>,
    pub(super) top_mem: Vec<ProcessStatJson>,
}

#[derive(Serialize)]
pub(super) struct ProcessStatJson {
    pub(super) pid: u32,
    pub(super) cpu_usage: f32,
    pub(super) memory_bytes: u64,
    pub(super) comm: String,
}

#[derive(Serialize)]
pub(super) struct MemoryStatsJson {
    pub(super) ram_total: u64,
    pub(super) ram_free: u64,
    pub(super) ram_used: u64,
    pub(super) buffers: u64,
    pub(super) cached: u64,
    pub(super) swap_total: u64,
    pub(super) swap_free: u64,
    pub(super) swap_used: u64,
    pub(super) page_faults: u64,
    pub(super) page_faults_per_sec: f32,
}

#[derive(Serialize)]
pub(super) struct NetworkStatsJson {
    pub(super) interfaces: Vec<NetIfStatJson>,
}

#[derive(Serialize)]
pub(super) struct NetIfStatJson {
    pub(super) name: String,
    pub(super) rx_bytes: u64,
    pub(super) tx_bytes: u64,
    pub(super) rx_bytes_per_sec: f32,
    pub(super) tx_bytes_per_sec: f32,
}

#[derive(Serialize)]
pub(super) struct MetaStatsJson {
    pub(super) timestamp_ns: u64,
    pub(super) uptime_secs: u64,
    pub(super) load_avg_1m: f32,
    pub(super) load_avg_5m: f32,
    pub(super) load_avg_15m: f32,
    pub(super) timezone_name: String,
    pub(super) timezone_offset_secs: i32,
    pub(super) os: OsFingerprintJson,
}

#[derive(Serialize)]
pub(super) struct OsFingerprintJson {
    pub(super) os_type: String,
    pub(super) os_id: String,
    pub(super) os_version_id: String,
    pub(super) os_pretty_name: String,
}

#[derive(Serialize)]
pub(super) struct GpuStatsJson {
    pub(super) nvml_available: bool,
    pub(super) gpus: Vec<GpuStatJson>,
}

#[derive(Serialize)]
pub(super) struct GpuStatJson {
    pub(super) name: String,
    pub(super) memory_total: u64,
    pub(super) memory_used: u64,
    pub(super) utilization_percent: f32,
    pub(super) power_watts: f32,
    pub(super) temperature_celsius: i16,
    pub(super) available: bool,
}
