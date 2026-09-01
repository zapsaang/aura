use crate::error::AuraResult;
use crate::{DiskStat, MountStat, TelemetryArchive};

use super::{
    check_bool, check_nonempty_text, check_percent, check_rate, check_text, expect_zero,
    expect_zero_u8, fault, owned_f32, owned_u32, owned_u64, rate_path,
};
use crate::archive::capabilities::{
    CAP_STORAGE_DISK_BYTES, CAP_STORAGE_DISK_IOPS, CAP_STORAGE_DISK_LATENCY,
    CAP_STORAGE_DISK_QUEUE_DEPTH, CAP_STORAGE_DISK_RATES, CAP_STORAGE_MOUNTS,
};
use crate::archive::{MAX_DISKS, MAX_MOUNTS};

const DISK_MASK: u64 = CAP_STORAGE_DISK_BYTES
    | CAP_STORAGE_DISK_RATES
    | CAP_STORAGE_DISK_IOPS
    | CAP_STORAGE_DISK_QUEUE_DEPTH
    | CAP_STORAGE_DISK_LATENCY;

fn validate_disk(disk: &DiskStat, index: usize, listed: bool, caps: u64) -> AuraResult<()> {
    let base = format!("storage.disks[{index}]");
    if !listed {
        return expect_zero(bytemuck::bytes_of(disk), &base);
    }
    check_nonempty_text(
        &disk.name.bytes,
        &format!("{base}.name"),
        "storage.disk_count",
    )?;
    let bytes_owned = caps & CAP_STORAGE_DISK_BYTES != 0;
    owned_u64(disk.read_bytes, bytes_owned, &format!("{base}.read_bytes"))?;
    owned_u64(
        disk.write_bytes,
        bytes_owned,
        &format!("{base}.write_bytes"),
    )?;
    let rates_owned = caps & CAP_STORAGE_DISK_RATES != 0;
    owned_f32(
        disk.read_bytes_per_sec,
        rates_owned,
        &rate_path(&base, "read_bytes"),
        check_rate,
    )?;
    owned_f32(
        disk.write_bytes_per_sec,
        rates_owned,
        &rate_path(&base, "write_bytes"),
        check_rate,
    )?;
    let iops_owned = caps & CAP_STORAGE_DISK_IOPS != 0;
    owned_f32(
        disk.read_iops,
        iops_owned,
        &format!("{base}.read_iops"),
        check_rate,
    )?;
    owned_f32(
        disk.write_iops,
        iops_owned,
        &format!("{base}.write_iops"),
        check_rate,
    )?;
    owned_u32(
        disk.queue_depth,
        caps & CAP_STORAGE_DISK_QUEUE_DEPTH != 0,
        &format!("{base}.queue_depth"),
    )?;
    let latency_owned = caps & CAP_STORAGE_DISK_LATENCY != 0;
    owned_f32(
        disk.read_latency_ms,
        latency_owned,
        &format!("{base}.read_latency_ms"),
        check_rate,
    )?;
    owned_f32(
        disk.write_latency_ms,
        latency_owned,
        &format!("{base}.write_latency_ms"),
        check_rate,
    )?;
    expect_zero(&disk._pad0, &format!("{base}.padding"))
}

fn validate_mount(mount: &MountStat, index: usize, listed: bool) -> AuraResult<()> {
    let base = format!("storage.mounts[{index}]");
    if !listed {
        return expect_zero(bytemuck::bytes_of(mount), &base);
    }
    check_nonempty_text(
        &mount.mountpoint,
        &format!("{base}.mountpoint"),
        "storage.mount_count",
    )?;
    check_text(&mount.fstype.bytes, &format!("{base}.fstype"))?;
    if mount.available > mount.total {
        return Err(fault(
            format!("{base}.available"),
            format!("{} exceeds {}", mount.available, mount.total),
        ));
    }
    if mount.used > mount.total {
        return Err(fault(
            format!("{base}.used"),
            format!("{} exceeds {}", mount.used, mount.total),
        ));
    }
    check_percent(mount.percent, &format!("{base}.percent"))?;
    expect_zero(&mount._pad0, &format!("{base}.padding"))
}

pub(super) fn validate(a: &TelemetryArchive) -> AuraResult<()> {
    let caps = a.capabilities;
    let s = &a.storage;
    let any_disk = caps & DISK_MASK != 0;
    let mounts_owned = caps & CAP_STORAGE_MOUNTS != 0;

    let disk_listed = (s.disk_count as usize).min(MAX_DISKS);
    for (index, disk) in s.disks.iter().enumerate() {
        validate_disk(disk, index, any_disk && index < disk_listed, caps)?;
    }
    if !any_disk {
        expect_zero_u8(s.disk_count, "storage.disk_count")?;
        expect_zero_u8(s.disk_truncated, "storage.disk_truncated")?;
    } else {
        if s.disk_count as usize > MAX_DISKS {
            return Err(fault(
                "storage.disk_count".to_string(),
                format!("{} exceeds {MAX_DISKS}", s.disk_count),
            ));
        }
        check_bool(s.disk_truncated, "storage.disk_truncated")?;
    }
    expect_zero(&s._pad0, "storage.disk_header_padding")?;

    let mount_listed = (s.mount_count as usize).min(MAX_MOUNTS);
    for (index, mount) in s.mounts.iter().enumerate() {
        validate_mount(mount, index, mounts_owned && index < mount_listed)?;
    }
    if !mounts_owned {
        expect_zero(&s.mount_count.to_le_bytes(), "storage.mount_count")?;
        expect_zero_u8(s.mount_truncated, "storage.mount_truncated")?;
    } else {
        if s.mount_count as usize > MAX_MOUNTS {
            return Err(fault(
                "storage.mount_count".to_string(),
                format!("{} exceeds {MAX_MOUNTS}", s.mount_count),
            ));
        }
        check_bool(s.mount_truncated, "storage.mount_truncated")?;
    }
    expect_zero(&s._pad1, "storage.mount_tail_padding")?;
    Ok(())
}
