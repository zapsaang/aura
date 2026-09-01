use aura_common::{bytes_to_string, TelemetryArchive};

use super::schema::{
    CpuCoreStatJson, CpuGlobalStatJson, GpuStatJson, GpuStatsJson, MemoryStatsJson, MetaStatsJson,
    NetIfStatJson, NetworkStatsJson, OsFingerprintJson, ProcessStatJson, ProcessStatsJson,
    TelemetryJson,
};
use crate::Module;

impl TelemetryJson {
    pub(super) fn from_telemetry(module: Module, telemetry: &TelemetryArchive) -> Self {
        let include_cpu = matches!(module, Module::All | Module::Cpu);
        let include_mem = matches!(module, Module::All | Module::Mem | Module::Swap);
        let include_net = matches!(module, Module::All | Module::Net);
        let include_meta = matches!(module, Module::All | Module::Os);
        let include_rest = matches!(module, Module::All);

        Self {
            version: telemetry.version,
            cpu: include_cpu.then(|| cpu_to_json(telemetry)),
            process: include_rest.then(|| process_to_json(telemetry)),
            memory: include_mem.then(|| memory_to_json(telemetry)),
            network: include_net.then(|| network_to_json(telemetry)),
            meta: include_meta.then(|| meta_to_json(telemetry)),
            gpu: include_rest.then(|| gpu_to_json(telemetry)),
        }
    }
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
        timezone_name: bytes_to_string(&meta.timezone_name),
        timezone_offset_secs: meta.timezone_offset_secs,
        os: OsFingerprintJson {
            os_type: meta.os.os_type.as_str().to_string(),
            os_id: meta.os.os_id.as_str().to_string(),
            os_version_id: meta.os.os_version_id.as_str().to_string(),
            os_pretty_name: bytes_to_string(&meta.os.os_pretty_name),
        },
    }
}

fn gpu_to_json(telemetry: &TelemetryArchive) -> GpuStatsJson {
    let gpu = &telemetry.gpu;
    GpuStatsJson {
        nvml_available: gpu.nvml_available != 0,
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
                    available: item.available != 0,
                }
            })
            .collect(),
    }
}
