use aura_common::{
    TelemetryArchive, CAP_MEMORY_BUFFERS, CAP_MEMORY_CACHED, CAP_MEMORY_PAGE_FAULTS,
    CAP_MEMORY_RAM_FREE, CAP_MEMORY_RAM_TOTAL, CAP_MEMORY_RAM_USED, CAP_MEMORY_SWAP,
};

use crate::args::ColorMode;

use super::color::{self, NA};
use super::si::si;

fn ram_percent_owned(caps: u64) -> bool {
    caps & CAP_MEMORY_RAM_TOTAL != 0 && caps & CAP_MEMORY_RAM_USED != 0
}

fn scalar(out: &mut String, label: &str, owned: bool, rendered: String) {
    out.push_str(label);
    if owned {
        out.push_str(&rendered);
    } else {
        out.push_str(NA);
    }
}

pub fn render(color: ColorMode, t: &TelemetryArchive) -> String {
    let caps = t.capabilities;
    let m = &t.memory;
    let mut out = String::from("MEMORY\n");

    out.push_str("  ram: ");
    if caps & CAP_MEMORY_RAM_USED != 0 {
        out.push_str(&si(m.ram_used as f64));
    } else {
        out.push_str(NA);
    }
    out.push_str(" / ");
    if caps & CAP_MEMORY_RAM_TOTAL != 0 {
        out.push_str(&si(m.ram_total as f64));
    } else {
        out.push_str(NA);
    }
    out.push_str(" (");
    if ram_percent_owned(caps) {
        let text = format!("{:.1}%", t.derived.ram_used_percent);
        out.push_str(&color::paint(color, t.derived.ram_tone, &text));
    } else {
        out.push_str(NA);
    }
    out.push(')');

    scalar(
        &mut out,
        "\n  free: ",
        caps & CAP_MEMORY_RAM_FREE != 0,
        si(m.ram_free as f64),
    );
    scalar(
        &mut out,
        "\n  buffers: ",
        caps & CAP_MEMORY_BUFFERS != 0,
        si(m.buffers as f64),
    );
    scalar(
        &mut out,
        "\n  cached: ",
        caps & CAP_MEMORY_CACHED != 0,
        si(m.cached as f64),
    );
    scalar(
        &mut out,
        "\n  page faults/s: ",
        caps & CAP_MEMORY_PAGE_FAULTS != 0,
        format!("{:.1}", m.page_faults_per_sec),
    );

    out
}

pub fn render_swap(color: ColorMode, t: &TelemetryArchive) -> String {
    let caps = t.capabilities;
    let m = &t.memory;
    let owned = caps & CAP_MEMORY_SWAP != 0;
    let mut out = String::from("SWAP\n  used: ");

    if owned {
        out.push_str(&si(m.swap_used as f64));
        out.push_str(" / ");
        out.push_str(&si(m.swap_total as f64));
        out.push_str(" (");
        let text = format!("{:.1}%", t.derived.swap_used_percent);
        out.push_str(&color::paint(color, t.derived.swap_tone, &text));
        out.push(')');
    } else {
        out.push_str(NA);
        out.push_str(" / ");
        out.push_str(NA);
        out.push_str(" (");
        out.push_str(NA);
        out.push(')');
    }

    out
}
