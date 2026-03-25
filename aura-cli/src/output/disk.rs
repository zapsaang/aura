use aura_common::TelemetryArchive;

use crate::ColorMode;

use super::{ansi, trim_zero_terminated};

pub fn render(color: ColorMode, telemetry: &TelemetryArchive) -> String {
    let storage = &telemetry.storage;

    let mut out = String::new();
    out.push_str(&ansi::style(color, ansi::BOLD, "=== DISK ==="));

    for idx in 0..storage.disk_count as usize {
        let disk = &storage.disks[idx];
        out.push('\n');
        out.push_str(&format!(
            "{}: rx={} tx={}",
            disk.name.as_str(),
            ansi::fmt_bps(disk.rx_per_sec),
            ansi::fmt_bps(disk.wx_per_sec)
        ));
    }

    if storage.mount_count > 0 {
        out.push('\n');
        out.push_str(&ansi::style(color, ansi::DIM, "Mounts:"));
        for idx in 0..storage.mount_count as usize {
            let mount = &storage.mounts[idx];
            let label = trim_zero_terminated(&mount.mountpoint);
            out.push('\n');
            out.push_str(&format!(
                "{} ({}) {:>5.1}% used={} total={}",
                label,
                mount.fstype.as_str(),
                mount.percent,
                ansi::fmt_bytes(mount.used),
                ansi::fmt_bytes(mount.total)
            ));
        }
    }

    out
}
