//! Storage collection: Linux reads `/proc/diskstats` and
//! `/proc/self/mountinfo`; macOS enumerates mounts with one fixed-buffer
//! `getfsstat` call while disk counters are compile-time unavailable.

use aura_common::{
    DiskStat, FixedString16, MountStat, StorageStats, CAP_STORAGE_DISK_BYTES,
    CAP_STORAGE_DISK_IOPS, CAP_STORAGE_DISK_LATENCY, CAP_STORAGE_DISK_QUEUE_DEPTH,
    CAP_STORAGE_DISK_RATES, CAP_STORAGE_MOUNTS, MAX_MOUNTS,
};

pub mod state;

#[cfg(target_os = "linux")]
pub mod linux;

#[cfg(target_os = "macos")]
mod macos;

#[cfg(target_os = "macos")]
pub(crate) use macos::collect as collect_mounts;

pub(crate) const EMPTY_DISK: DiskStat = DiskStat {
    name: FixedString16::new(),
    major: 0,
    minor: 0,
    read_bytes: 0,
    write_bytes: 0,
    read_bytes_per_sec: 0.0,
    write_bytes_per_sec: 0.0,
    read_iops: 0.0,
    write_iops: 0.0,
    queue_depth: 0,
    read_latency_ms: 0.0,
    write_latency_ms: 0.0,
    _pad0: [0; 4],
};

pub(crate) const EMPTY_MOUNT: MountStat = MountStat {
    mountpoint: [0; 256],
    fstype: FixedString16::new(),
    total: 0,
    available: 0,
    used: 0,
    percent: 0.0,
    _pad0: [0; 4],
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StorageAvailability {
    pub disk_metrics: bool,
    pub mounts: bool,
}

impl StorageAvailability {
    pub const fn capability_mask(self) -> u64 {
        let mut capabilities = 0;
        if self.disk_metrics {
            capabilities |= CAP_STORAGE_DISK_BYTES
                | CAP_STORAGE_DISK_RATES
                | CAP_STORAGE_DISK_IOPS
                | CAP_STORAGE_DISK_QUEUE_DEPTH
                | CAP_STORAGE_DISK_LATENCY;
        }
        if self.mounts {
            capabilities |= CAP_STORAGE_MOUNTS;
        }
        capabilities
    }
}

/// Platform-neutral capacity sample: Linux fills it from `statvfs`
/// (`f_frsize` units), macOS from each `statfs` record (`f_bsize` units).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FsCapacity {
    pub blocks: u64,
    pub bfree: u64,
    pub bavail: u64,
    pub unit: u64,
}

/// Normative mount formula: `total=blocks*unit`, `used=(blocks-bfree)*unit`,
/// `available=bavail*unit`; percent is 0 for zero total and otherwise clamps
/// to 0..=100. Reserved blocks may separate `used+available` from `total`.
/// Returns `None` on arithmetic overflow or an inconsistent record that would
/// violate the published `used|available <= total` invariants.
pub fn mount_capacity(capacity: &FsCapacity) -> Option<(u64, u64, u64, f32)> {
    let total = capacity.blocks.checked_mul(capacity.unit)?;
    let used = capacity
        .blocks
        .checked_sub(capacity.bfree)?
        .checked_mul(capacity.unit)?;
    let available = capacity.bavail.checked_mul(capacity.unit)?;
    if available > total {
        return None;
    }
    let percent = if total == 0 {
        0.0
    } else {
        (100.0 * used as f64 / total as f64).clamp(0.0, 100.0) as f32
    };
    Some((total, used, available, percent))
}

/// One fixed mount record from the macOS `getfsstat` buffer; tests inject
/// these directly to exercise the 31/32/33 return contract.
#[derive(Clone, Copy, Debug)]
pub struct MountSample {
    pub mountpoint: [u8; 256],
    pub mountpoint_len: u16,
    pub fstype: [u8; 16],
    pub fstype_len: u8,
    pub blocks: u64,
    pub bfree: u64,
    pub bavail: u64,
    pub unit: u64,
}

impl MountSample {
    pub const fn zero() -> Self {
        Self {
            mountpoint: [0; 256],
            mountpoint_len: 0,
            fstype: [0; 16],
            fstype_len: 0,
            blocks: 0,
            bfree: 0,
            bavail: 0,
            unit: 0,
        }
    }

    pub fn new(
        mountpoint: &[u8],
        fstype: &[u8],
        blocks: u64,
        bfree: u64,
        bavail: u64,
        unit: u64,
    ) -> Option<Self> {
        if mountpoint.is_empty() || mountpoint.len() > 255 || fstype.is_empty() || fstype.len() > 16
        {
            return None;
        }
        let mut sample = Self::zero();
        sample.mountpoint[..mountpoint.len()].copy_from_slice(mountpoint);
        sample.mountpoint_len = mountpoint.len() as u16;
        sample.fstype[..fstype.len()].copy_from_slice(fstype);
        sample.fstype_len = fstype.len() as u8;
        sample.blocks = blocks;
        sample.bfree = bfree;
        sample.bavail = bavail;
        sample.unit = unit;
        Some(sample)
    }
}

/// Publishes up to `MAX_MOUNTS` validated mount records, preserving sample
/// order. `samples.len() > MAX_MOUNTS` (the 33rd getfsstat record) or
/// `forced_truncated` marks list truncation; an invalid individual record
/// skips that record and sets truncation. Stale slots are cleared.
pub fn publish_statfs_mounts(
    samples: &[MountSample],
    forced_truncated: bool,
    out: &mut StorageStats,
) {
    let mut count = 0usize;
    let mut truncated = forced_truncated || samples.len() > MAX_MOUNTS;
    for sample in samples.iter().take(MAX_MOUNTS) {
        let mountpoint = &sample.mountpoint[..sample.mountpoint_len as usize];
        let fstype = &sample.fstype[..sample.fstype_len as usize];
        if !text_is_publishable(mountpoint) || !text_is_publishable(fstype) {
            truncated = true;
            continue;
        }
        if mount_is_duplicate(out, count, mountpoint) {
            truncated = true;
            continue;
        }
        let capacity = FsCapacity {
            blocks: sample.blocks,
            bfree: sample.bfree,
            bavail: sample.bavail,
            unit: sample.unit,
        };
        let Some(totals) = mount_capacity(&capacity) else {
            truncated = true;
            continue;
        };
        write_mount(out, count, mountpoint, fstype, totals);
        count += 1;
    }
    out.mount_count = count as u16;
    out.mount_truncated = u8::from(truncated);
    clear_stale_mounts(out, count);
}

pub(crate) fn text_is_publishable(text: &[u8]) -> bool {
    let Ok(text) = std::str::from_utf8(text) else {
        return false;
    };
    !text.chars().any(char::is_control)
}

pub(crate) fn mount_is_duplicate(out: &StorageStats, count: usize, mountpoint: &[u8]) -> bool {
    out.mounts[..count].iter().any(|published| {
        let end = published
            .mountpoint
            .iter()
            .position(|&byte| byte == 0)
            .unwrap_or(published.mountpoint.len());
        &published.mountpoint[..end] == mountpoint
    })
}

pub(crate) fn write_mount(
    out: &mut StorageStats,
    count: usize,
    mountpoint: &[u8],
    fstype: &[u8],
    totals: (u64, u64, u64, f32),
) {
    let mount = &mut out.mounts[count];
    *mount = EMPTY_MOUNT;
    mount.mountpoint[..mountpoint.len()].copy_from_slice(mountpoint);
    mount.fstype = FixedString16::from_bytes(fstype);
    mount.total = totals.0;
    mount.used = totals.1;
    mount.available = totals.2;
    mount.percent = totals.3;
}

pub(crate) fn clear_stale_mounts(out: &mut StorageStats, from: usize) {
    for slot in out.mounts.iter_mut().skip(from) {
        *slot = EMPTY_MOUNT;
    }
}
