use aura_common::{
    DiskStat, TelemetryArchive, CAP_STORAGE_DISK_BYTES, CAP_STORAGE_DISK_IOPS,
    CAP_STORAGE_DISK_LATENCY, CAP_STORAGE_DISK_QUEUE_DEPTH, CAP_STORAGE_DISK_RATES,
    CAP_STORAGE_MOUNTS,
};

use crate::args::ColorMode;

use super::color::{ARRAY_NA, ARRAY_NONE, NA};
use super::si::si;

const DISK_ARRAY_CAPS: u64 = CAP_STORAGE_DISK_BYTES
    | CAP_STORAGE_DISK_RATES
    | CAP_STORAGE_DISK_IOPS
    | CAP_STORAGE_DISK_QUEUE_DEPTH
    | CAP_STORAGE_DISK_LATENCY;

fn disk_row(caps: u64, d: &DiskStat) -> String {
    let mut row = format!(
        "    {}",
        if caps & CAP_STORAGE_DISK_BYTES != 0 {
            d.name.as_str()
        } else {
            NA
        }
    );

    row.push_str(" read=");
    if caps & CAP_STORAGE_DISK_RATES != 0 {
        row.push_str(&format!("{}/s", si(d.read_bytes_per_sec as f64)));
    } else {
        row.push_str(NA);
    }
    row.push_str(" write=");
    if caps & CAP_STORAGE_DISK_RATES != 0 {
        row.push_str(&format!("{}/s", si(d.write_bytes_per_sec as f64)));
    } else {
        row.push_str(NA);
    }
    row.push_str(" riops=");
    if caps & CAP_STORAGE_DISK_IOPS != 0 {
        row.push_str(&format!("{:.1}", d.read_iops));
    } else {
        row.push_str(NA);
    }
    row.push_str(" wiops=");
    if caps & CAP_STORAGE_DISK_IOPS != 0 {
        row.push_str(&format!("{:.1}", d.write_iops));
    } else {
        row.push_str(NA);
    }
    row.push_str(" queue=");
    if caps & CAP_STORAGE_DISK_QUEUE_DEPTH != 0 {
        row.push_str(&d.queue_depth.to_string());
    } else {
        row.push_str(NA);
    }
    row.push_str(" rlat=");
    if caps & CAP_STORAGE_DISK_LATENCY != 0 {
        row.push_str(&format!("{:.1}ms", d.read_latency_ms));
    } else {
        row.push_str(NA);
    }
    row.push_str(" wlat=");
    if caps & CAP_STORAGE_DISK_LATENCY != 0 {
        row.push_str(&format!("{:.1}ms", d.write_latency_ms));
    } else {
        row.push_str(NA);
    }
    row
}

pub fn render(_color: ColorMode, t: &TelemetryArchive) -> String {
    let caps = t.capabilities;
    let s = &t.storage;
    let mut out = String::from("STORAGE\n  disks:\n");

    if caps & DISK_ARRAY_CAPS == 0 {
        out.push_str(ARRAY_NA);
    } else if s.disk_count == 0 {
        out.push_str(ARRAY_NONE);
    } else {
        let rows: Vec<String> = (0..s.disk_count as usize)
            .map(|idx| disk_row(caps, &s.disks[idx]))
            .collect();
        out.push_str(&rows.join("\n"));
    }

    out.push_str("\n  mounts:\n");
    if caps & CAP_STORAGE_MOUNTS == 0 {
        out.push_str(ARRAY_NA);
    } else if s.mount_count == 0 {
        out.push_str(ARRAY_NONE);
    } else {
        let rows: Vec<String> = (0..s.mount_count as usize)
            .map(|idx| {
                let m = &s.mounts[idx];
                format!(
                    "    {} {} {} / {} ({:.1}%) avail={}",
                    aura_common::bytes_to_string(&m.mountpoint),
                    m.fstype.as_str(),
                    si(m.used as f64),
                    si(m.total as f64),
                    m.percent,
                    si(m.available as f64)
                )
            })
            .collect();
        out.push_str(&rows.join("\n"));
    }

    out
}
