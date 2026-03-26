use aura_common::TelemetryArchive;

use crate::ColorMode;

use super::ansi;

pub fn render(color: ColorMode, telemetry: &TelemetryArchive) -> String {
    let cpu = &telemetry.cpu;
    let mut out = String::new();

    out.push_str(&ansi::style(color, ansi::BOLD, "=== CPU ==="));
    out.push('\n');
    out.push_str("Usage: ");
    out.push_str(&ansi::fmt_pct(
        color,
        cpu.usage_percent,
        ansi::cpu_color(cpu.usage_percent),
    ));
    out.push('\n');
    out.push_str(&format!(
        "Context Switches: {:>10.0}/s",
        cpu.context_switches_per_sec
    ));

    if cpu.core_count > 0 {
        out.push('\n');
        out.push_str(&ansi::style(color, ansi::DIM, "Per-core:"));
        for idx in 0..cpu.core_count as usize {
            let core = &cpu.cores[idx];
            out.push('\n');
            out.push_str(&format!(
                "cpu{:>2}: {} user={:>10} system={:>10}",
                core.core_index,
                ansi::fmt_pct(
                    color,
                    core.usage_percent,
                    ansi::cpu_color(core.usage_percent)
                ),
                core.user_ticks,
                core.system_ticks
            ));
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use aura_common::{CpuCoreStat, CpuGlobalStat, TelemetryArchive, MAX_CORES};

    use crate::ColorMode;

    use super::render;

    #[test]
    fn render_cpu_contains_usage_and_core_lines() {
        let mut telemetry = unsafe { std::mem::zeroed::<TelemetryArchive>() };
        telemetry.cpu = CpuGlobalStat {
            user_ticks: 0,
            system_ticks: 0,
            idle_ticks: 0,
            total_ticks: 0,
            context_switches: 0,
            context_switches_per_sec: 123.0,
            usage_percent: 84.0,
            cores: [CpuCoreStat {
                core_index: 0,
                _pad0: [0; 7],
                user_ticks: 11,
                system_ticks: 22,
                idle_ticks: 0,
                total_ticks: 33,
                usage_percent: 84.0,
                _pad1: [0; 4],
            }; MAX_CORES],
            core_count: 1,
            _pad0: [0; 7],
        };

        let out = render(ColorMode::None, &telemetry);
        assert!(out.contains("=== CPU ==="));
        assert!(out.contains("84.0%"));
        assert!(out.contains("cpu 0:"));
    }
}
