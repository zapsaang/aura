use aura_common::TelemetryArchive;

use crate::ColorMode;

use super::ansi;

pub fn render(color: ColorMode, telemetry: &TelemetryArchive) -> String {
    let mem = &telemetry.memory;
    let percent = if mem.ram_total > 0 {
        (mem.ram_used as f32 / mem.ram_total as f32) * 100.0
    } else {
        0.0
    };

    let mut out = String::new();
    out.push_str(&ansi::style(color, ansi::BOLD, "=== MEMORY ==="));
    out.push('\n');
    out.push_str("RAM: ");
    out.push_str(&ansi::fmt_pct(color, percent, ansi::memory_color(percent)));
    out.push_str(&format!(
        "  used={} free={} total={}",
        ansi::fmt_bytes(mem.ram_used),
        ansi::fmt_bytes(mem.ram_free),
        ansi::fmt_bytes(mem.ram_total)
    ));
    out.push('\n');
    out.push_str(&format!(
        "Buffers={} Cached={} PageFaults={:.0}/s",
        ansi::fmt_bytes(mem.buffers),
        ansi::fmt_bytes(mem.cached),
        mem.page_faults_per_sec
    ));
    out
}

pub fn render_swap(color: ColorMode, telemetry: &TelemetryArchive) -> String {
    let mem = &telemetry.memory;
    let percent = if mem.swap_total > 0 {
        (mem.swap_used as f32 / mem.swap_total as f32) * 100.0
    } else {
        0.0
    };

    let mut out = String::new();
    out.push_str(&ansi::style(color, ansi::BOLD, "=== SWAP ==="));
    out.push('\n');
    out.push_str("Swap: ");
    out.push_str(&ansi::fmt_pct(color, percent, ansi::memory_color(percent)));
    out.push_str(&format!(
        "  used={} free={} total={}",
        ansi::fmt_bytes(mem.swap_used),
        ansi::fmt_bytes(mem.swap_free),
        ansi::fmt_bytes(mem.swap_total)
    ));
    out
}

#[cfg(test)]
mod tests {
    use aura_common::{MemoryStats, TelemetryArchive};

    use crate::ColorMode;

    use super::{render, render_swap};

    #[test]
    fn render_mem_contains_percent_and_totals() {
        let mut telemetry = unsafe { std::mem::zeroed::<TelemetryArchive>() };
        telemetry.memory = MemoryStats {
            ram_total: 100,
            ram_free: 25,
            ram_used: 75,
            buffers: 5,
            cached: 10,
            swap_total: 100,
            swap_free: 80,
            swap_used: 20,
            page_faults: 0,
            page_faults_per_sec: 7.0,
        };

        let out = render(ColorMode::None, &telemetry);
        assert!(out.contains("=== MEMORY ==="));
        assert!(out.contains("75.0%"));
        assert!(out.contains("PageFaults=7/s"));
    }

    #[test]
    fn render_swap_contains_swap_percent() {
        let mut telemetry = unsafe { std::mem::zeroed::<TelemetryArchive>() };
        telemetry.memory = MemoryStats {
            ram_total: 0,
            ram_free: 0,
            ram_used: 0,
            buffers: 0,
            cached: 0,
            swap_total: 200,
            swap_free: 50,
            swap_used: 150,
            page_faults: 0,
            page_faults_per_sec: 0.0,
        };

        let out = render_swap(ColorMode::None, &telemetry);
        assert!(out.contains("=== SWAP ==="));
        assert!(out.contains("75.0%"));
    }
}
