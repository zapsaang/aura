use aura_common::TelemetryArchive;

use crate::ColorMode;

use super::{ansi, trim_zero_terminated};

pub fn render(color: ColorMode, telemetry: &TelemetryArchive) -> String {
    let meta = &telemetry.meta;
    let tz = trim_zero_terminated(&meta.timezone_name);
    let os_type = meta.os.os_type.as_str();
    let os_id = meta.os.os_id.as_str();
    let pretty = trim_zero_terminated(&meta.os.os_pretty_name);

    let mut out = String::new();
    out.push_str(&ansi::style(color, ansi::BOLD, "=== META ==="));
    out.push('\n');
    out.push_str(&format!(
        "OS: {} ({}/{})",
        if pretty.is_empty() {
            "unknown"
        } else {
            &pretty
        },
        os_type,
        os_id
    ));
    out.push('\n');
    out.push_str(&format!(
        "Uptime: {}s  Load: {:.2} {:.2} {:.2}",
        meta.uptime_secs, meta.load_avg_1m, meta.load_avg_5m, meta.load_avg_15m
    ));
    out.push('\n');
    out.push_str(&format!("Timezone: {} ({})", tz, meta.timezone_offset_secs));

    for idx in 0..telemetry.gpu.gpu_count as usize {
        let gpu = &telemetry.gpu.gpus[idx];
        out.push('\n');
        out.push_str(&format!(
            "GPU {}: util={} temp={}",
            gpu.name.as_str(),
            ansi::fmt_pct(
                color,
                gpu.utilization_percent,
                ansi::cpu_color(gpu.utilization_percent)
            ),
            ansi::paint(
                color,
                &format!("{}C", gpu.temperature_celsius),
                ansi::temperature_color(gpu.temperature_celsius)
            )
        ));
    }

    out
}
