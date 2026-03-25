use aura_common::TelemetryArchive;

use crate::ColorMode;

use super::ansi;

pub fn render(color: ColorMode, telemetry: &TelemetryArchive) -> String {
    let network = &telemetry.network;
    let mut out = String::new();
    out.push_str(&ansi::style(color, ansi::BOLD, "=== NETWORK ==="));

    for idx in 0..network.if_count as usize {
        let iface = &network.interfaces[idx];
        out.push('\n');
        out.push_str(&format!(
            "{}: rx={} tx={}",
            iface.name.as_str(),
            ansi::fmt_bps(iface.rx_bytes_per_sec),
            ansi::fmt_bps(iface.tx_bytes_per_sec)
        ));
    }

    out
}
