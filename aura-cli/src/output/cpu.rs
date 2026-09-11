use aura_common::{TelemetryArchive, CAP_CPU_CONTEXT_SWITCHES, CAP_CPU_GLOBAL, CAP_CPU_PER_CORE};

use crate::args::ColorMode;

use super::color::{self, ARRAY_NA, ARRAY_NONE, NA};

pub fn render(color: ColorMode, t: &TelemetryArchive) -> String {
    let caps = t.capabilities;
    let cpu = &t.cpu;
    let mut out = String::from("CPU\n");

    out.push_str("  usage: ");
    if caps & CAP_CPU_GLOBAL != 0 {
        let text = format!("{:.1}%", cpu.usage_percent);
        out.push_str(&color::paint(color, t.derived.cpu_tone, &text));
    } else {
        out.push_str(NA);
    }

    out.push_str("\n  context switches/s: ");
    if caps & CAP_CPU_CONTEXT_SWITCHES != 0 {
        out.push_str(&format!("{:.1}", cpu.context_switches_per_sec));
    } else {
        out.push_str(NA);
    }

    out.push_str("\n  cores:\n");
    if caps & CAP_CPU_PER_CORE == 0 {
        out.push_str(ARRAY_NA);
    } else if cpu.core_count == 0 {
        out.push_str(ARRAY_NONE);
    } else {
        let rows: Vec<String> = (0..cpu.core_count as usize)
            .map(|idx| {
                let core = &cpu.cores[idx];
                format!("    cpu{}: {:.1}%", core.core_index, core.usage_percent)
            })
            .collect();
        out.push_str(&rows.join("\n"));
    }

    out
}
