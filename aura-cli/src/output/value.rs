use crate::Module;
use aura_common::TelemetryArchive;

pub fn render(module: Module, telemetry: &TelemetryArchive) -> String {
    match module {
        Module::Cpu => format!("cpu:{}", telemetry.cpu.usage_percent),
        Module::Mem => {
            let mem = &telemetry.memory;
            let used = mem.ram_used as f32 / mem.ram_total as f32 * 100.0;
            format!("memory:{}", used)
        }
        Module::Swap => {
            let mem = &telemetry.memory;
            if mem.swap_total > 0 {
                format!(
                    "swap:{}",
                    mem.swap_used as f32 / mem.swap_total as f32 * 100.0
                )
            } else {
                "swap:0.0".to_string()
            }
        }
        Module::All => [
            format!("cpu:{}", telemetry.cpu.usage_percent),
            format!("memory:{}", {
                let mem = &telemetry.memory;
                if mem.ram_total > 0 {
                    mem.ram_used as f32 / mem.ram_total as f32 * 100.0
                } else {
                    0.0
                }
            }),
        ]
        .join("\n"),
        _ => "".to_string(),
    }
}
