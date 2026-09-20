use aura_common::{
    TelemetryArchive, CAP_CPU_GLOBAL, CAP_GPU_ENUMERATION, CAP_MEMORY_RAM_TOTAL,
    CAP_MEMORY_RAM_USED, CAP_MEMORY_SWAP, CAP_META_WALLCLOCK, CAP_NETWORK_BYTES, CAP_NETWORK_RATES,
    CAP_PROCESS_TOP_CPU, CAP_STORAGE_DISK_RATES, GPU_CAP_UTILIZATION,
};

use super::si::si;
use crate::args::Module;

fn cpu_row(t: &TelemetryArchive) -> String {
    if t.capabilities & CAP_CPU_GLOBAL != 0 {
        format!("cpu={:.1}%", t.cpu.usage_percent)
    } else {
        "cpu=N/A".to_string()
    }
}

fn process_row(t: &TelemetryArchive) -> String {
    let p = &t.process;
    if t.capabilities & CAP_PROCESS_TOP_CPU != 0 && p.top_cpu_count > 0 {
        format!("process={:.1}%", p.top_cpu[0].cpu_usage)
    } else {
        "process=N/A".to_string()
    }
}

fn mem_row(t: &TelemetryArchive) -> String {
    let caps = t.capabilities;
    if caps & CAP_MEMORY_RAM_TOTAL != 0 && caps & CAP_MEMORY_RAM_USED != 0 {
        format!("mem={:.1}%", t.derived.ram_used_percent)
    } else {
        "mem=N/A".to_string()
    }
}

fn swap_row(t: &TelemetryArchive) -> String {
    if t.capabilities & CAP_MEMORY_SWAP != 0 {
        format!("swap={:.1}%", t.derived.swap_used_percent)
    } else {
        "swap=N/A".to_string()
    }
}

fn disk_row(t: &TelemetryArchive) -> String {
    let s = &t.storage;
    if t.capabilities & CAP_STORAGE_DISK_RATES != 0 && s.disk_count > 0 {
        let d = &s.disks[0];
        format!(
            "disk=read={}/s,write={}/s",
            si(d.read_bytes_per_sec as f64),
            si(d.write_bytes_per_sec as f64)
        )
    } else {
        "disk=N/A".to_string()
    }
}

fn net_row(t: &TelemetryArchive) -> String {
    let caps = t.capabilities;
    if caps & CAP_NETWORK_BYTES != 0 && caps & CAP_NETWORK_RATES != 0 && t.network.if_count > 0 {
        format!(
            "net=rx={}/s,tx={}/s",
            si(t.derived.aggregate_rx_bytes_per_sec as f64),
            si(t.derived.aggregate_tx_bytes_per_sec as f64)
        )
    } else {
        "net=N/A".to_string()
    }
}

fn os_row(t: &TelemetryArchive) -> String {
    if t.capabilities & CAP_META_WALLCLOCK != 0 {
        format!("os={}", t.meta.wallclock_ns)
    } else {
        "os=N/A".to_string()
    }
}

fn gpu_row(t: &TelemetryArchive) -> String {
    let g = &t.gpu;
    if t.capabilities & CAP_GPU_ENUMERATION != 0
        && g.gpu_count > 0
        && g.gpus[0].capabilities & GPU_CAP_UTILIZATION != 0
    {
        format!("gpu={:.1}%", g.gpus[0].utilization_percent)
    } else {
        "gpu=N/A".to_string()
    }
}

pub fn render(module: Module, telemetry: &TelemetryArchive) -> String {
    match module {
        Module::Cpu => cpu_row(telemetry),
        Module::Process => process_row(telemetry),
        Module::Mem => mem_row(telemetry),
        Module::Swap => swap_row(telemetry),
        Module::Disk => disk_row(telemetry),
        Module::Net => net_row(telemetry),
        Module::Os => os_row(telemetry),
        Module::Gpu => gpu_row(telemetry),
        Module::All => [
            cpu_row(telemetry),
            process_row(telemetry),
            mem_row(telemetry),
            swap_row(telemetry),
            disk_row(telemetry),
            net_row(telemetry),
            os_row(telemetry),
            gpu_row(telemetry),
        ]
        .join("\n"),
    }
}
