use crate::Module;
use aura_common::TelemetryArchive;

pub fn render(module: Module, telemetry: &TelemetryArchive) -> String {
    match module {
        Module::Cpu => format!("{:.1}", telemetry.cpu.usage_percent),
        Module::Mem => {
            let mem = &telemetry.memory;
            if mem.ram_total > 0 {
                format!("{:.1}", mem.ram_used as f32 / mem.ram_total as f32 * 100.0)
            } else {
                "0.0".to_string()
            }
        }
        Module::Swap => {
            let mem = &telemetry.memory;
            if mem.swap_total > 0 {
                format!(
                    "{:.1}",
                    mem.swap_used as f32 / mem.swap_total as f32 * 100.0
                )
            } else {
                "0.0".to_string()
            }
        }
        Module::Disk => {
            let storage = &telemetry.storage;
            let mut total_rate = 0.0f32;
            for i in 0..storage.disk_count as usize {
                total_rate += storage.disks[i].rx_per_sec + storage.disks[i].wx_per_sec;
            }
            if total_rate > 0.0 {
                format!("{:.0}", total_rate)
            } else if storage.mount_count > 0 {
                let mut total_used = 0u64;
                let mut total_cap = 0u64;
                for i in 0..storage.mount_count as usize {
                    total_used += storage.mounts[i].used;
                    total_cap += storage.mounts[i].total;
                }
                if total_cap > 0 {
                    format!("{:.1}", total_used as f32 / total_cap as f32 * 100.0)
                } else {
                    "0.0".to_string()
                }
            } else {
                "0.0".to_string()
            }
        }
        Module::Net => {
            let net = &telemetry.network;
            let mut total_rate = 0.0f32;
            for i in 0..net.if_count as usize {
                total_rate +=
                    net.interfaces[i].rx_bytes_per_sec + net.interfaces[i].tx_bytes_per_sec;
            }
            format!("{:.0}", total_rate)
        }
        Module::Os => "1".to_string(),
        Module::All => {
            let mem = &telemetry.memory;
            let cpu_val = telemetry.cpu.usage_percent;
            let mem_val = if mem.ram_total > 0 {
                mem.ram_used as f32 / mem.ram_total as f32 * 100.0
            } else {
                0.0
            };
            let swap_val = if mem.swap_total > 0 {
                mem.swap_used as f32 / mem.swap_total as f32 * 100.0
            } else {
                0.0
            };
            let storage = &telemetry.storage;
            let mut disk_val = 0.0f32;
            if storage.disk_count > 0 {
                let mut total_rate = 0.0f32;
                for i in 0..storage.disk_count as usize {
                    total_rate += storage.disks[i].rx_per_sec + storage.disks[i].wx_per_sec;
                }
                disk_val = total_rate;
            } else if storage.mount_count > 0 {
                let mut total_used = 0u64;
                let mut total_cap = 0u64;
                for i in 0..storage.mount_count as usize {
                    total_used += storage.mounts[i].used;
                    total_cap += storage.mounts[i].total;
                }
                if total_cap > 0 {
                    disk_val = total_used as f32 / total_cap as f32 * 100.0;
                }
            }
            let net = &telemetry.network;
            let mut net_val = 0.0f32;
            for i in 0..net.if_count as usize {
                net_val += net.interfaces[i].rx_bytes_per_sec + net.interfaces[i].tx_bytes_per_sec;
            }
            [
                format!("{:.1}", cpu_val),
                format!("{:.1}", mem_val),
                format!("{:.1}", swap_val),
                format!("{:.1}", disk_val),
                format!("{:.1}", net_val),
            ]
            .join("\n")
        }
    }
}
