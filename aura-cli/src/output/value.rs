use aura_common::{
    AuraError, AuraResult, TelemetryArchive, CAP_CPU_GLOBAL, CAP_GPU_ENUMERATION,
    CAP_MEMORY_RAM_TOTAL, CAP_MEMORY_RAM_USED, CAP_MEMORY_SWAP, CAP_META_OS_IDENTITY,
    CAP_NETWORK_BYTES, CAP_NETWORK_RATES, CAP_PROCESS_TOP_CPU, CAP_STORAGE_DISK_RATES,
    GPU_CAP_UTILIZATION,
};

use super::{meta::os_icon, si::si};
use crate::args::Module;

#[derive(Clone, Copy)]
enum KeyMode {
    Labelled,
    Bare,
}

impl KeyMode {
    fn prefix(self, label: &str) -> &str {
        match self {
            KeyMode::Labelled => label,
            KeyMode::Bare => "",
        }
    }
}

fn cpu_row(t: &TelemetryArchive, mode: KeyMode) -> String {
    let key = mode.prefix("cpu=");
    if t.capabilities & CAP_CPU_GLOBAL != 0 {
        format!("{key}{:.1}%", t.cpu.usage_percent)
    } else {
        format!("{key}N/A")
    }
}

fn process_row(t: &TelemetryArchive, mode: KeyMode) -> String {
    let key = mode.prefix("process=");
    let p = &t.process;
    if t.capabilities & CAP_PROCESS_TOP_CPU != 0 && p.top_cpu_count > 0 {
        format!("{key}{:.1}%", p.top_cpu[0].cpu_usage)
    } else {
        format!("{key}N/A")
    }
}

fn mem_row(t: &TelemetryArchive, mode: KeyMode) -> String {
    let key = mode.prefix("mem=");
    let caps = t.capabilities;
    if caps & CAP_MEMORY_RAM_TOTAL != 0 && caps & CAP_MEMORY_RAM_USED != 0 {
        format!("{key}{:.1}%", t.derived.ram_used_percent)
    } else {
        format!("{key}N/A")
    }
}

fn swap_row(t: &TelemetryArchive, mode: KeyMode) -> String {
    let key = mode.prefix("swap=");
    if t.capabilities & CAP_MEMORY_SWAP != 0 {
        format!("{key}{:.1}%", t.derived.swap_used_percent)
    } else {
        format!("{key}N/A")
    }
}

fn disk_row(t: &TelemetryArchive, mode: KeyMode) -> String {
    let key = mode.prefix("disk=");
    let s = &t.storage;
    if t.capabilities & CAP_STORAGE_DISK_RATES != 0 && s.disk_count > 0 {
        let d = &s.disks[0];
        format!(
            "{key}read={}/s,write={}/s",
            si(d.read_bytes_per_sec as f64),
            si(d.write_bytes_per_sec as f64)
        )
    } else {
        format!("{key}N/A")
    }
}

fn net_row(t: &TelemetryArchive, mode: KeyMode) -> String {
    let key = mode.prefix("net=");
    let caps = t.capabilities;
    if caps & CAP_NETWORK_BYTES != 0 && caps & CAP_NETWORK_RATES != 0 && t.network.if_count > 0 {
        format!(
            "{key}rx={}/s,tx={}/s",
            si(t.derived.aggregate_rx_bytes_per_sec as f64),
            si(t.derived.aggregate_tx_bytes_per_sec as f64)
        )
    } else {
        format!("{key}N/A")
    }
}

fn os_row(t: &TelemetryArchive, mode: KeyMode) -> String {
    let key = mode.prefix("os=");
    if t.capabilities & CAP_META_OS_IDENTITY != 0 {
        format!(
            "{key}{}",
            os_icon(t.meta.os.os_id.as_str(), t.meta.os.os_type.as_str())
        )
    } else {
        format!("{key}N/A")
    }
}

fn gpu_row(t: &TelemetryArchive, mode: KeyMode) -> String {
    let key = mode.prefix("gpu=");
    let g = &t.gpu;
    if t.capabilities & CAP_GPU_ENUMERATION != 0
        && g.gpu_count > 0
        && g.gpus[0].capabilities & GPU_CAP_UTILIZATION != 0
    {
        format!("{key}{:.1}%", g.gpus[0].utilization_percent)
    } else {
        format!("{key}N/A")
    }
}

pub fn render(module: Module, telemetry: &TelemetryArchive) -> String {
    match module {
        Module::Cpu => cpu_row(telemetry, KeyMode::Labelled),
        Module::Process => process_row(telemetry, KeyMode::Labelled),
        Module::Mem => mem_row(telemetry, KeyMode::Labelled),
        Module::Swap => swap_row(telemetry, KeyMode::Labelled),
        Module::Disk => disk_row(telemetry, KeyMode::Labelled),
        Module::Net => net_row(telemetry, KeyMode::Labelled),
        Module::Os => os_row(telemetry, KeyMode::Labelled),
        Module::Gpu => gpu_row(telemetry, KeyMode::Labelled),
        Module::All => [
            cpu_row(telemetry, KeyMode::Labelled),
            process_row(telemetry, KeyMode::Labelled),
            mem_row(telemetry, KeyMode::Labelled),
            swap_row(telemetry, KeyMode::Labelled),
            disk_row(telemetry, KeyMode::Labelled),
            net_row(telemetry, KeyMode::Labelled),
            os_row(telemetry, KeyMode::Labelled),
            gpu_row(telemetry, KeyMode::Labelled),
        ]
        .join("\n"),
    }
}

pub(crate) fn render_raw(module: Module, telemetry: &TelemetryArchive) -> AuraResult<String> {
    let row = match module {
        Module::Cpu => cpu_row(telemetry, KeyMode::Bare),
        Module::Process => process_row(telemetry, KeyMode::Bare),
        Module::Mem => mem_row(telemetry, KeyMode::Bare),
        Module::Swap => swap_row(telemetry, KeyMode::Bare),
        Module::Disk => disk_row(telemetry, KeyMode::Bare),
        Module::Net => net_row(telemetry, KeyMode::Bare),
        Module::Os => os_row(telemetry, KeyMode::Bare),
        Module::Gpu => gpu_row(telemetry, KeyMode::Bare),
        Module::All => {
            return Err(AuraError::InvalidArgument(
                "--format raw requires a single module (got --module all)".to_string(),
            ));
        }
    };
    Ok(row)
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_common::{
        CpuCoreStat, DiskStat, FixedString16, GpuStat, NetIfStat, ProcessStat, ARCHIVE_VERSION,
        MAX_CORES, MAX_DISKS, MAX_GPUS, MAX_NETIFS, MAX_TOP_N,
    };

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

    fn supported() -> TelemetryArchive {
        let mut t = archive(
            CAP_CPU_GLOBAL
                | CAP_PROCESS_TOP_CPU
                | CAP_MEMORY_RAM_TOTAL
                | CAP_MEMORY_RAM_USED
                | CAP_MEMORY_SWAP
                | CAP_META_OS_IDENTITY
                | CAP_NETWORK_BYTES
                | CAP_NETWORK_RATES
                | CAP_STORAGE_DISK_RATES
                | CAP_GPU_ENUMERATION,
        );
        t.cpu.usage_percent = 42.0;
        t.cpu.cores = [CpuCoreStat {
            core_index: 0,
            _pad0: [0; 7],
            user_ticks: 0,
            system_ticks: 0,
            idle_ticks: 0,
            total_ticks: 0,
            usage_percent: 0.0,
            _pad1: [0; 4],
        }; MAX_CORES];
        t.process.top_cpu = [ProcessStat {
            pid: 42,
            cpu_usage: 50.0,
            memory_bytes: 1000,
            comm: fs16("top"),
        }; MAX_TOP_N];
        t.process.top_cpu_count = 1;
        t.derived.ram_used_percent = 50.0;
        t.derived.swap_used_percent = 25.0;
        t.derived.aggregate_rx_bytes_per_sec = 1500.0;
        t.derived.aggregate_tx_bytes_per_sec = 2500.0;
        t.storage.disks = [DiskStat {
            name: fs16("sda"),
            major: 8,
            minor: 0,
            read_bytes: 0,
            write_bytes: 0,
            read_bytes_per_sec: 1500.0,
            write_bytes_per_sec: 2_000_000.0,
            read_iops: 0.0,
            write_iops: 0.0,
            queue_depth: 0,
            read_latency_ms: 0.0,
            write_latency_ms: 0.0,
            _pad0: [0; 4],
        }; MAX_DISKS];
        t.storage.disk_count = 1;
        t.network.interfaces = [NetIfStat {
            name: fs16("eth0"),
            rx_bytes: 0,
            tx_bytes: 0,
            rx_bytes_per_sec: 0.0,
            tx_bytes_per_sec: 0.0,
        }; MAX_NETIFS];
        t.network.if_count = 1;
        t.meta.os.os_type = fs16("linux");
        t.meta.os.os_id = fs16("ubuntu");
        t.gpu.gpus = [GpuStat {
            name: fs16("nv"),
            memory_total: 0,
            memory_used: 0,
            utilization_percent: 55.5,
            power_watts: 0.0,
            temperature_celsius: 0,
            available: 1,
            tone: 0,
            _pad0: [0; 4],
            capabilities: GPU_CAP_UTILIZATION,
        }; MAX_GPUS];
        t.gpu.gpu_count = 1;
        t
    }

    #[test]
    fn raw_bare_rows_match_labelled_values() {
        let t = supported();
        let cases: [(Module, &str); 8] = [
            (Module::Cpu, "42.0%"),
            (Module::Process, "50.0%"),
            (Module::Mem, "50.0%"),
            (Module::Swap, "25.0%"),
            (Module::Disk, "read=1.5KB/s,write=2.0MB/s"),
            (Module::Net, "rx=1.5KB/s,tx=2.5KB/s"),
            (Module::Os, "\u{f31b}"),
            (Module::Gpu, "55.5%"),
        ];
        for (module, expected) in cases {
            assert_eq!(render_raw(module, &t).unwrap(), expected, "{module:?}");
        }
    }

    #[test]
    fn raw_unavailable_rows_are_na() {
        let t = archive(0);
        for module in [
            Module::Cpu,
            Module::Process,
            Module::Mem,
            Module::Swap,
            Module::Disk,
            Module::Net,
            Module::Os,
            Module::Gpu,
        ] {
            assert_eq!(render_raw(module, &t).unwrap(), "N/A", "{module:?}");
        }

        let mut empty_disk = supported();
        empty_disk.storage.disk_count = 0;
        assert_eq!(render_raw(Module::Disk, &empty_disk).unwrap(), "N/A");

        let mut empty_net = supported();
        empty_net.network.if_count = 0;
        assert_eq!(render_raw(Module::Net, &empty_net).unwrap(), "N/A");

        let mut empty_process = supported();
        empty_process.process.top_cpu_count = 0;
        assert_eq!(render_raw(Module::Process, &empty_process).unwrap(), "N/A");

        let mut empty_gpu = supported();
        empty_gpu.gpu.gpu_count = 0;
        assert_eq!(render_raw(Module::Gpu, &empty_gpu).unwrap(), "N/A");
    }

    #[test]
    fn raw_all_is_invalid_argument_not_panic() {
        let t = archive(0);
        match render_raw(Module::All, &t) {
            Err(AuraError::InvalidArgument(message)) => {
                assert_eq!(
                    message,
                    "--format raw requires a single module (got --module all)"
                );
            }
            other => panic!("expected InvalidArgument, got {other:?}"),
        }
    }
}
