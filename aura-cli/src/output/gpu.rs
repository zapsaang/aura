use aura_common::{
    GpuStat, TelemetryArchive, CAP_GPU_ENUMERATION, GPU_CAP_MEMORY_TOTAL, GPU_CAP_MEMORY_USED,
    GPU_CAP_NAME, GPU_CAP_POWER, GPU_CAP_TEMPERATURE, GPU_CAP_UTILIZATION,
};

use crate::args::ColorMode;

use super::color::{self, ARRAY_NA, ARRAY_NONE, NA};
use super::si::si;

fn gpu_row(color: ColorMode, idx: usize, g: &GpuStat) -> String {
    let caps = g.capabilities;
    let mut row = format!("    {idx} ");

    if caps & GPU_CAP_NAME != 0 {
        row.push_str(g.name.as_str());
    } else {
        row.push_str(NA);
    }

    row.push_str(" memory=");
    if caps & GPU_CAP_MEMORY_USED != 0 {
        row.push_str(&si(g.memory_used as f64));
    } else {
        row.push_str(NA);
    }
    row.push('/');
    if caps & GPU_CAP_MEMORY_TOTAL != 0 {
        row.push_str(&si(g.memory_total as f64));
    } else {
        row.push_str(NA);
    }

    row.push_str(" util=");
    if caps & GPU_CAP_UTILIZATION != 0 {
        row.push_str(&format!("{:.1}%", g.utilization_percent));
    } else {
        row.push_str(NA);
    }

    row.push_str(" power=");
    if caps & GPU_CAP_POWER != 0 {
        row.push_str(&format!("{:.1}W", g.power_watts));
    } else {
        row.push_str(NA);
    }

    row.push_str(" temp=");
    if caps & GPU_CAP_TEMPERATURE != 0 {
        let text = format!("{}C", g.temperature_celsius);
        row.push_str(&color::paint(color, g.tone, &text));
    } else {
        row.push_str(NA);
    }

    row
}

pub fn render(color: ColorMode, t: &TelemetryArchive) -> String {
    let g = &t.gpu;
    let mut out = String::from("GPU\n  devices:\n");

    if t.capabilities & CAP_GPU_ENUMERATION == 0 {
        out.push_str(ARRAY_NA);
    } else if g.gpu_count == 0 {
        out.push_str(ARRAY_NONE);
    } else {
        let rows: Vec<String> = (0..g.gpu_count as usize)
            .map(|idx| gpu_row(color, idx, &g.gpus[idx]))
            .collect();
        out.push_str(&rows.join("\n"));
    }

    out
}
