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
            format!("{:.1}", telemetry.cpu.usage_percent)
        }
        Module::Net => "0.0".to_string(),
        Module::Os => "0.0".to_string(),
        Module::All => [
            format!("{:.1}", telemetry.cpu.usage_percent),
            format!("{:.1}", {
                let mem = &telemetry.memory;
                if mem.ram_total > 0 {
                    mem.ram_used as f32 / mem.ram_total as f32 * 100.0
                } else {
                    0.0
                }
            }),
        ]
        .join("\n"),
    }
}
