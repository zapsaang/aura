use crate::Module;
use aura_common::TelemetryArchive;

use super::meta::os_logo;

fn get_cpu_val(telemetry: &TelemetryArchive) -> f32 {
    telemetry.cpu.usage_percent
}

fn get_mem_val(telemetry: &TelemetryArchive) -> f32 {
    let mem = &telemetry.memory;
    if mem.ram_total > 0 {
        mem.ram_used as f32 / mem.ram_total as f32 * 100.0
    } else {
        0.0
    }
}

fn get_swap_val(telemetry: &TelemetryArchive) -> f32 {
    let mem = &telemetry.memory;
    if mem.swap_total > 0 {
        mem.swap_used as f32 / mem.swap_total as f32 * 100.0
    } else {
        0.0
    }
}

fn get_disk_val(telemetry: &TelemetryArchive) -> f32 {
    let storage = &telemetry.storage;
    if storage.disk_count > 0 {
        storage
            .disks
            .iter()
            .take(storage.disk_count as usize)
            .map(|d| d.rx_per_sec + d.wx_per_sec)
            .sum()
    } else if storage.mount_count > 0 {
        let (total_used, total_cap) = storage
            .mounts
            .iter()
            .take(storage.mount_count as usize)
            .fold((0u64, 0u64), |acc, m| (acc.0 + m.used, acc.1 + m.total));

        if total_cap > 0 {
            total_used as f32 / total_cap as f32 * 100.0
        } else {
            0.0
        }
    } else {
        0.0
    }
}

fn get_net_val(telemetry: &TelemetryArchive) -> f32 {
    let net = &telemetry.network;
    net.interfaces
        .iter()
        .take(net.if_count as usize)
        .map(|i| i.rx_bytes_per_sec + i.tx_bytes_per_sec)
        .sum()
}

fn format_compact(mut val: f32) -> String {
    let units = ["", "K", "M", "G", "T"];
    let mut idx = 0;

    while val >= 999.5 && idx < units.len() - 1 {
        val /= 1024.0;
        idx += 1;
    }

    if idx == 0 {
        format!("{:>3}", val.round() as u32)
    } else {
        format!("{:>2}{}", val.round() as u32, units[idx])
    }
}

pub fn render(module: Module, telemetry: &TelemetryArchive) -> String {
    match module {
        Module::Cpu => format!("{:>3}", get_cpu_val(telemetry).round() as u32),
        Module::Mem => format!("{:>3}", get_mem_val(telemetry).round() as u32),
        Module::Swap => format!("{:>3}", get_swap_val(telemetry).round() as u32),

        Module::Disk => format_compact(get_disk_val(telemetry)),
        Module::Net => format_compact(get_net_val(telemetry)),

        Module::Os => {
            let meta = &telemetry.meta;
            os_logo(meta.os.os_id.as_str(), meta.os.os_type.as_str()).to_string()
        }
        Module::All => [
            format!("{:>3}", get_cpu_val(telemetry).round() as u32),
            format!("{:>3}", get_mem_val(telemetry).round() as u32),
            format!("{:>3}", get_swap_val(telemetry).round() as u32),
            format_compact(get_disk_val(telemetry)),
            format_compact(get_net_val(telemetry)),
        ]
        .join("\n"),
    }
}
