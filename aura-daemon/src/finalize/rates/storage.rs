//! Disk rate finalization and storage byte ownership. Byte rates are
//! `delta(sectors)*512/dt`, IOPS are completed-operation deltas over `dt`,
//! latency is `delta(ms)/delta(completed)` (zero when no I/O occurred while
//! the capability stays set), and queue depth is the collector's current
//! gauge. Counter decrease reseeds the device with one zero-rate cycle.

use aura_common::{
    TelemetryArchive, CAP_STORAGE_DISK_BYTES, CAP_STORAGE_DISK_IOPS, CAP_STORAGE_DISK_LATENCY,
    CAP_STORAGE_DISK_QUEUE_DEPTH, CAP_STORAGE_DISK_RATES, CAP_STORAGE_MOUNTS, MAX_DISKS,
    MAX_MOUNTS,
};

use crate::collectors::storage::state::DiskKey;
use crate::collectors::FixedCollectorState;

const DISK_MASK: u64 = CAP_STORAGE_DISK_BYTES
    | CAP_STORAGE_DISK_RATES
    | CAP_STORAGE_DISK_IOPS
    | CAP_STORAGE_DISK_QUEUE_DEPTH
    | CAP_STORAGE_DISK_LATENCY;

pub(super) fn finalize(state: &mut FixedCollectorState, elapsed: f64, warmed: bool) {
    if state.archive.capabilities & CAP_STORAGE_DISK_BYTES == 0 {
        return;
    }
    let caps = state.archive.capabilities;
    let represented = (state.archive.storage.disk_count as usize).min(MAX_DISKS);
    let next_generation = state.baselines.disk.generation.wrapping_add(1);

    for index in 0..represented {
        let key = DiskKey {
            major: state.archive.storage.disks[index].major,
            minor: state.archive.storage.disks[index].minor,
        };
        let current = state.disk_raw[index];
        let previous = if warmed {
            state.baselines.disk.get(&key).copied()
        } else {
            None
        };

        let mut derived = Derived::default();
        if let Some(slot) = previous {
            if current.covers(&slot.counters) {
                derived = derive(&current, &slot.counters, elapsed);
            }
        }

        let disk = &mut state.archive.storage.disks[index];
        if caps & CAP_STORAGE_DISK_RATES != 0 {
            disk.read_bytes_per_sec = derived.read_bytes_per_sec;
            disk.write_bytes_per_sec = derived.write_bytes_per_sec;
        }
        if caps & CAP_STORAGE_DISK_IOPS != 0 {
            disk.read_iops = derived.read_iops;
            disk.write_iops = derived.write_iops;
        }
        if caps & CAP_STORAGE_DISK_LATENCY != 0 {
            disk.read_latency_ms = derived.read_latency_ms;
            disk.write_latency_ms = derived.write_latency_ms;
        }

        state.baselines.disk.insert(key, next_generation, current);
    }

    state.baselines.disk.sweep(next_generation);
    state.baselines.disk.generation = next_generation;
}

#[derive(Default)]
struct Derived {
    read_bytes_per_sec: f32,
    write_bytes_per_sec: f32,
    read_iops: f32,
    write_iops: f32,
    read_latency_ms: f32,
    write_latency_ms: f32,
}

fn derive(
    current: &crate::collectors::storage::state::DiskRawSnapshot,
    previous: &crate::collectors::storage::state::DiskRawSnapshot,
    elapsed: f64,
) -> Derived {
    let delta_sectors_read = current.sectors_read - previous.sectors_read;
    let delta_sectors_written = current.sectors_written - previous.sectors_written;
    let delta_reads = current.reads_completed - previous.reads_completed;
    let delta_writes = current.writes_completed - previous.writes_completed;
    let delta_read_ms = current.read_ms - previous.read_ms;
    let delta_write_ms = current.write_ms - previous.write_ms;

    Derived {
        read_bytes_per_sec: (delta_sectors_read as f64 * 512.0 / elapsed) as f32,
        write_bytes_per_sec: (delta_sectors_written as f64 * 512.0 / elapsed) as f32,
        read_iops: (delta_reads as f64 / elapsed) as f32,
        write_iops: (delta_writes as f64 / elapsed) as f32,
        read_latency_ms: if delta_reads > 0 {
            delta_read_ms as f32 / delta_reads as f32
        } else {
            0.0
        },
        write_latency_ms: if delta_writes > 0 {
            delta_write_ms as f32 / delta_writes as f32
        } else {
            0.0
        },
    }
}

/// Zeroes storage bytes their capability bits do not own; trailing slots
/// beyond the published counts are always cleared.
pub(in crate::finalize) fn zero_unowned(
    archive: &mut TelemetryArchive,
    zero: &TelemetryArchive,
    caps: u64,
) {
    let storage = &mut archive.storage;
    if caps & DISK_MASK == 0 {
        storage.disks = zero.storage.disks;
        storage.disk_count = 0;
        storage.disk_truncated = 0;
    } else {
        let listed = (storage.disk_count as usize).min(MAX_DISKS);
        for disk in storage.disks[..listed].iter_mut() {
            if caps & CAP_STORAGE_DISK_BYTES == 0 {
                disk.read_bytes = 0;
                disk.write_bytes = 0;
            }
            if caps & CAP_STORAGE_DISK_RATES == 0 {
                disk.read_bytes_per_sec = 0.0;
                disk.write_bytes_per_sec = 0.0;
            }
            if caps & CAP_STORAGE_DISK_IOPS == 0 {
                disk.read_iops = 0.0;
                disk.write_iops = 0.0;
            }
            if caps & CAP_STORAGE_DISK_QUEUE_DEPTH == 0 {
                disk.queue_depth = 0;
            }
            if caps & CAP_STORAGE_DISK_LATENCY == 0 {
                disk.read_latency_ms = 0.0;
                disk.write_latency_ms = 0.0;
            }
        }
        for disk in storage.disks[listed..].iter_mut() {
            *disk = zero.storage.disks[0];
        }
    }
    if caps & CAP_STORAGE_MOUNTS == 0 {
        storage.mounts = zero.storage.mounts;
        storage.mount_count = 0;
        storage.mount_truncated = 0;
    } else {
        let listed = (storage.mount_count as usize).min(MAX_MOUNTS);
        for mount in storage.mounts[listed..].iter_mut() {
            *mount = zero.storage.mounts[0];
        }
    }
}
