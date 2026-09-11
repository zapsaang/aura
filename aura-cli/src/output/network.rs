use aura_common::{TelemetryArchive, CAP_NETWORK_BYTES, CAP_NETWORK_RATES};

use crate::args::ColorMode;

use super::color::{ARRAY_NA, ARRAY_NONE, NA};
use super::si::si;

fn aggregate_owned(caps: u64) -> bool {
    caps & CAP_NETWORK_BYTES != 0 && caps & CAP_NETWORK_RATES != 0
}

pub fn render(_color: ColorMode, t: &TelemetryArchive) -> String {
    let caps = t.capabilities;
    let n = &t.network;
    let mut out = String::from("NETWORK\n  total: rx=");

    if aggregate_owned(caps) {
        out.push_str(&format!(
            "{}/s",
            si(t.derived.aggregate_rx_bytes_per_sec as f64)
        ));
    } else {
        out.push_str(NA);
    }
    out.push_str(" tx=");
    if aggregate_owned(caps) {
        out.push_str(&format!(
            "{}/s",
            si(t.derived.aggregate_tx_bytes_per_sec as f64)
        ));
    } else {
        out.push_str(NA);
    }

    out.push_str("\n  interfaces:\n");
    if caps & (CAP_NETWORK_BYTES | CAP_NETWORK_RATES) == 0 {
        out.push_str(ARRAY_NA);
    } else if n.if_count == 0 {
        out.push_str(ARRAY_NONE);
    } else {
        let rows: Vec<String> = (0..n.if_count as usize)
            .map(|idx| {
                let i = &n.interfaces[idx];
                let mut row = String::from("    ");
                if caps & CAP_NETWORK_BYTES != 0 {
                    row.push_str(i.name.as_str());
                } else {
                    row.push_str(NA);
                }
                row.push_str(" rx=");
                if caps & CAP_NETWORK_RATES != 0 {
                    row.push_str(&format!("{}/s", si(i.rx_bytes_per_sec as f64)));
                } else {
                    row.push_str(NA);
                }
                row.push_str(" tx=");
                if caps & CAP_NETWORK_RATES != 0 {
                    row.push_str(&format!("{}/s", si(i.tx_bytes_per_sec as f64)));
                } else {
                    row.push_str(NA);
                }
                row
            })
            .collect();
        out.push_str(&rows.join("\n"));
    }

    out
}
