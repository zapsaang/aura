//! Storage collector contract tests (Todo 9).
//!
//! Locks the Linux `/proc/diskstats` + `/proc/self/mountinfo` parsers, the
//! normative mount capacity formula, disk baseline/rate semantics, and the
//! macOS getfsstat publication path (via the platform-neutral
//! `publish_statfs_mounts` seam with injected 31/32/33 record counts).

#[allow(dead_code)]
mod support;

use aura_common::{
    FixedString16, StorageStats, TelemetryArchive, CAP_STORAGE_DISK_BYTES, CAP_STORAGE_DISK_IOPS,
    CAP_STORAGE_DISK_LATENCY, CAP_STORAGE_DISK_QUEUE_DEPTH, CAP_STORAGE_DISK_RATES,
    CAP_STORAGE_MOUNTS, MAX_DISKS, MAX_MOUNTS,
};
use aura_daemon::collectors::storage::state::{
    DiskBaselineMap, DiskKey, DiskRawSnapshot, DISK_MAP_CAPACITY,
};
use aura_daemon::collectors::storage::{
    mount_capacity, publish_statfs_mounts, FsCapacity, MountSample, StorageAvailability,
};
use aura_daemon::collectors::FixedCollectorState;
use aura_daemon::finalize::{Clock, ClockSample, SystemFinalizer};
use aura_daemon::lifecycle::Finalizer;

#[cfg(target_os = "linux")]
use aura_daemon::collectors::storage::linux;

const DISK_CAPS: u64 = CAP_STORAGE_DISK_BYTES
    | CAP_STORAGE_DISK_RATES
    | CAP_STORAGE_DISK_IOPS
    | CAP_STORAGE_DISK_QUEUE_DEPTH
    | CAP_STORAGE_DISK_LATENCY;

fn empty_storage() -> StorageStats {
    TelemetryArchive::zeroed().storage
}

fn empty_raw() -> [DiskRawSnapshot; MAX_DISKS] {
    [DiskRawSnapshot::zero(); MAX_DISKS]
}

fn mountpoint_of(mount: &aura_common::MountStat) -> &[u8] {
    let end = mount
        .mountpoint
        .iter()
        .position(|&b| b == 0)
        .unwrap_or(mount.mountpoint.len());
    &mount.mountpoint[..end]
}

// ---------------------------------------------------------------------
// Linux /proc/diskstats parsing
// ---------------------------------------------------------------------

#[cfg(target_os = "linux")]
#[test]
fn diskstats_fixture_parses_devices_in_kernel_order() {
    let fixture = include_bytes!("fixtures/proc_diskstats_sample.txt");
    let mut out = empty_storage();
    let mut raw = empty_raw();
    linux::parse_diskstats(fixture, &mut out, &mut raw).expect("parse diskstats");

    assert_eq!(out.disk_count, 7);
    assert_eq!(out.disk_truncated, 0);

    let expected: [(&[u8], u32, u32); 7] = [
        (b"sda", 8, 0),
        (b"sda1", 8, 1),
        (b"nvme0n1", 259, 0),
        (b"nvme0n1p1", 259, 1),
        (b"mmcblk0", 179, 0),
        (b"mmcblk0p1", 179, 1),
        (b"sr0", 11, 0),
    ];
    for (index, (name, major, minor)) in expected.iter().enumerate() {
        let disk = &out.disks[index];
        assert_eq!(disk.name.as_str(), std::str::from_utf8(name).unwrap());
        assert_eq!(disk.major, *major, "disk {index} major");
        assert_eq!(disk.minor, *minor, "disk {index} minor");
    }

    let sda = &out.disks[0];
    assert_eq!(sda.read_bytes, 20480 * 512);
    assert_eq!(sda.write_bytes, 81920 * 512);
    assert_eq!(sda.queue_depth, 0);
    assert_eq!(sda.read_bytes_per_sec, 0.0);
    assert_eq!(sda.write_bytes_per_sec, 0.0);
    assert_eq!(sda.read_iops, 0.0);
    assert_eq!(sda.write_iops, 0.0);
    assert_eq!(sda.read_latency_ms, 0.0);
    assert_eq!(sda.write_latency_ms, 0.0);
    assert_eq!(raw[0].sectors_read, 20480);
    assert_eq!(raw[0].sectors_written, 81920);
    assert_eq!(raw[0].reads_completed, 1000);
    assert_eq!(raw[0].read_ms, 500);
    assert_eq!(raw[0].writes_completed, 2000);
    assert_eq!(raw[0].write_ms, 800);

    let sr0 = &out.disks[6];
    assert_eq!(sr0.read_bytes, 32 * 512);
    assert_eq!(sr0.write_bytes, 0);
    assert_eq!(sr0.queue_depth, 2);
}

#[cfg(target_os = "linux")]
#[test]
fn diskstats_skips_loop_ram_and_dm_devices_with_partitions() {
    let buf = b"   7       0 loop0 1 0 8 1 2 0 16 2 0 3 6\n\
                \x20  7       1 loop0p1 1 0 8 1 2 0 16 2 0 3 6\n\
                \x20  1       0 ram0 0 0 0 0 0 0 0 0 0 0 0\n\
                \x20  1      15 ram15 0 0 0 0 0 0 0 0 0 0 0\n\
                \x20253       0 dm-0 1 0 8 1 2 0 16 2 0 3 6\n\
                \x20253      15 dm-15 1 0 8 1 2 0 16 2 0 3 6\n\
                \x20  8       0 sda 1 0 8 1 2 0 16 2 0 3 6\n";
    let mut out = empty_storage();
    let mut raw = empty_raw();
    linux::parse_diskstats(buf, &mut out, &mut raw).expect("parse");
    assert_eq!(out.disk_count, 1);
    assert_eq!(out.disks[0].name.as_str(), "sda");
    assert_eq!(out.disk_truncated, 0);
}

#[cfg(target_os = "linux")]
#[test]
fn diskstats_malformed_line_skips_and_truncates() {
    let buf = b"   8       0 sda 1 0 8 1 2 0 16 2 0 3 6\n\
                \x20  8      16 sdb 1 0 8\n\
                \x20  8      32 sdc nope 0 8 1 2 0 16 2 0 3 6\n";
    let mut out = empty_storage();
    let mut raw = empty_raw();
    linux::parse_diskstats(buf, &mut out, &mut raw).expect("parse");
    assert_eq!(out.disk_count, 1);
    assert_eq!(out.disks[0].name.as_str(), "sda");
    assert_eq!(out.disk_truncated, 1);
}

#[cfg(target_os = "linux")]
#[test]
fn diskstats_sector_overflow_skips_and_truncates() {
    let buf = b"   8       0 sda 1 0 36028797018963968 1 2 0 16 2 0 3 6\n\
                \x20  8      16 sdb 1 0 8 1 2 0 16 2 0 3 6\n";
    let mut out = empty_storage();
    let mut raw = empty_raw();
    linux::parse_diskstats(buf, &mut out, &mut raw).expect("parse");
    assert_eq!(out.disk_count, 1);
    assert_eq!(out.disks[0].name.as_str(), "sdb");
    assert_eq!(out.disk_truncated, 1);
}

#[cfg(target_os = "linux")]
#[test]
fn diskstats_duplicate_device_skips_and_truncates() {
    let buf = b"   8       0 sda 1 0 8 1 2 0 16 2 0 3 6\n\
                \x20  8       0 sda 9 9 9 9 9 9 9 9 9 9 9\n\
                \x20  8      16 sdb 1 0 8 1 2 0 16 2 0 3 6\n";
    let mut out = empty_storage();
    let mut raw = empty_raw();
    linux::parse_diskstats(buf, &mut out, &mut raw).expect("parse");
    assert_eq!(out.disk_count, 2);
    assert_eq!(
        out.disks[0].read_bytes,
        8 * 512,
        "first occurrence of (8,0) wins"
    );
    assert_eq!(out.disks[1].name.as_str(), "sdb");
    assert_eq!(out.disk_truncated, 1);
}

#[cfg(target_os = "linux")]
#[test]
fn diskstats_caps_at_max_disks_and_truncates() {
    let mut buf = Vec::new();
    for index in 0..17u32 {
        let line = format!(
            "   8 {:>7} sd{} 1 0 8 1 2 0 16 2 0 3 6\n",
            index * 16,
            (b'a' + index as u8) as char
        );
        buf.extend_from_slice(line.as_bytes());
    }
    let mut out = empty_storage();
    let mut raw = empty_raw();
    linux::parse_diskstats(&buf, &mut out, &mut raw).expect("parse");
    assert_eq!(out.disk_count as usize, MAX_DISKS);
    assert_eq!(out.disk_truncated, 1);
    assert_eq!(out.disks[MAX_DISKS - 1].name.as_str(), "sdp");
}

#[cfg(target_os = "linux")]
#[test]
fn diskstats_unterminated_tail_line_skips_and_truncates() {
    let buf = b"   8       0 sda 1 0 8 1 2 0 16 2 0 3 6\n   8      16 sdb 1 0 8 1 2 0 16 2 0 3 6";
    let mut out = empty_storage();
    let mut raw = empty_raw();
    linux::parse_diskstats(buf, &mut out, &mut raw).expect("parse");
    assert_eq!(out.disk_count, 1);
    assert_eq!(out.disks[0].name.as_str(), "sda");
    assert_eq!(out.disk_truncated, 1);
}

#[cfg(target_os = "linux")]
#[test]
fn missing_diskstats_file_is_fatal() {
    let mut buf = Vec::with_capacity(4096);
    let mut out = empty_storage();
    let mut raw = empty_raw();
    let result = linux::collect_from_paths(
        b"/nonexistent/aura/diskstats",
        b"/nonexistent/aura/mountinfo",
        &mut buf,
        &mut out,
        &mut raw,
    );
    assert!(result.is_err(), "missing diskstats must fail the cycle");
}

#[cfg(target_os = "linux")]
#[test]
fn missing_mountinfo_file_is_fatal() {
    use std::os::unix::ffi::OsStrExt;

    let directory = tempfile::tempdir().expect("tempdir");
    let diskstats = directory.path().join("diskstats");
    std::fs::write(&diskstats, b"   8       0 sda 1 0 8 1 2 0 16 2 0 3 6\n").expect("write");

    let diskstats_path = diskstats.as_os_str().as_bytes().to_vec();

    let mut buf = Vec::with_capacity(4096);
    let mut out = empty_storage();
    let mut raw = empty_raw();
    let result = linux::collect_from_paths(
        &diskstats_path,
        b"/nonexistent/aura/mountinfo",
        &mut buf,
        &mut out,
        &mut raw,
    );
    assert!(result.is_err(), "missing mountinfo must fail the cycle");
    assert_eq!(out.disk_count, 1, "diskstats phase completed first");
}

// ---------------------------------------------------------------------
// Linux /proc/self/mountinfo parsing
// ---------------------------------------------------------------------

#[cfg(target_os = "linux")]
fn scripted_capacity(path: &[u8]) -> Option<FsCapacity> {
    if path == b"/" {
        Some(FsCapacity {
            blocks: 1000,
            bfree: 600,
            bavail: 500,
            unit: 4096,
        })
    } else {
        Some(FsCapacity {
            blocks: 100,
            bfree: 50,
            bavail: 40,
            unit: 512,
        })
    }
}

#[cfg(target_os = "linux")]
#[test]
fn mountinfo_fixture_parses_eligible_mounts_in_kernel_order() {
    let fixture = include_bytes!("fixtures/proc_mountinfo_sample.txt");
    let mut out = empty_storage();
    linux::parse_mountinfo(fixture, &mut out, &mut scripted_capacity).expect("parse mountinfo");

    assert_eq!(out.mount_count, 5);
    assert_eq!(out.mount_truncated, 1);

    let expected: [(&[u8], &[u8]); 5] = [
        (b"/", b"ext4"),
        (b"/snap/core", b"squashfs"),
        (b"/boot/efi", b"vfat"),
        (b"/mnt/space dir", b"ext4"),
        (b"/mnt/back\\slash", b"ext4"),
    ];
    for (index, (mountpoint, fstype)) in expected.iter().enumerate() {
        let mount = &out.mounts[index];
        assert_eq!(mountpoint_of(mount), *mountpoint, "mount {index} path");
        assert_eq!(mount.fstype.as_str(), std::str::from_utf8(fstype).unwrap());
    }

    let root = &out.mounts[0];
    assert_eq!(root.total, 1000 * 4096);
    assert_eq!(root.used, 400 * 4096);
    assert_eq!(root.available, 500 * 4096);
    assert_eq!(root.percent, 40.0);

    let snap = &out.mounts[1];
    assert_eq!(snap.total, 100 * 512);
    assert_eq!(snap.used, 50 * 512);
    assert_eq!(snap.available, 40 * 512);
    assert_eq!(snap.percent, 50.0);
}

#[cfg(target_os = "linux")]
#[test]
fn mountinfo_excluded_fstypes_do_not_set_truncation() {
    let buf = b"36 24 8:1 / / rw,relatime - ext4 /dev/sda1 rw\n\
                \x2039 36 0:20 / /proc rw - proc proc rw\n\
                \x2040 36 0:21 / /sys rw - sysfs sysfs rw\n\
                \x2041 36 0:5 / /dev rw - devtmpfs udev rw\n\
                \x2042 36 0:22 / /run rw - tmpfs tmpfs rw\n\
                \x2043 36 0:23 / /dev/pts rw - devpts devpts rw\n\
                \x2044 36 0:24 / /sys/fs/cgroup rw - cgroup2 cgroup2 rw\n\
                \x2045 36 0:35 / /sys/fs/cgroup/unified rw - cgroup cgroup rw\n\
                \x2046 36 0:25 / /sys/kernel/security rw - securityfs securityfs rw\n\
                \x2047 36 0:26 / /sys/fs/pstore rw - pstore pstore rw\n\
                \x2048 36 0:27 / /sys/kernel/debug rw - debugfs debugfs rw\n\
                \x2049 36 0:28 / /sys/kernel/tracing rw - tracefs tracefs rw\n\
                \x2050 36 0:29 / /sys/kernel/config rw - configfs configfs rw\n\
                \x2051 36 0:30 / /sys/fs/fuse/connections rw - fusectl fusectl rw\n\
                \x2052 36 0:31 / /dev/mqueue rw - mqueue mqueue rw\n\
                \x2053 36 0:32 / /dev/hugepages rw - hugetlbfs hugetlbfs rw\n\
                \x2054 36 0:33 / /proc/sys/fs/binfmt_misc rw - binfmt_misc binfmt_misc rw\n\
                \x2055 36 0:34 / /var/lib/autofs rw - autofs autofs rw\n";
    let mut out = empty_storage();
    linux::parse_mountinfo(buf, &mut out, &mut scripted_capacity).expect("parse");
    assert_eq!(out.mount_count, 1);
    assert_eq!(mountpoint_of(&out.mounts[0]), b"/");
    assert_eq!(out.mount_truncated, 0);
}

#[cfg(target_os = "linux")]
#[test]
fn mountinfo_malformed_structure_is_fatal() {
    let buf = b"36 24 8:1 / / rw,relatime ext4 /dev/sda1 rw\n";
    let mut out = empty_storage();
    let result = linux::parse_mountinfo(buf, &mut out, &mut scripted_capacity);
    assert!(result.is_err(), "missing separator must be Fatal");
}

#[cfg(target_os = "linux")]
#[test]
fn mountinfo_statvfs_failure_skips_and_truncates() {
    let buf = b"36 24 8:1 / / rw,relatime - ext4 /dev/sda1 rw\n\
                \x2037 36 8:2 / /broken rw,relatime - ext4 /dev/sda2 rw\n";
    let mut out = empty_storage();
    let mut failing = |path: &[u8]| -> Option<FsCapacity> {
        if path == b"/broken" {
            None
        } else {
            scripted_capacity(path)
        }
    };
    linux::parse_mountinfo(buf, &mut out, &mut failing).expect("parse");
    assert_eq!(out.mount_count, 1);
    assert_eq!(mountpoint_of(&out.mounts[0]), b"/");
    assert_eq!(out.mount_truncated, 1);
}

#[cfg(target_os = "linux")]
#[test]
fn mountinfo_overflow_caps_at_32_and_truncates() {
    let mut buf = Vec::new();
    for index in 0..33u32 {
        let line = format!(
            "{} 36 8:1 / /mnt/vol{} rw,relatime - ext4 /dev/sda1 rw\n",
            36 + index,
            index
        );
        buf.extend_from_slice(line.as_bytes());
    }
    let mut out = empty_storage();
    linux::parse_mountinfo(&buf, &mut out, &mut scripted_capacity).expect("parse");
    assert_eq!(out.mount_count as usize, MAX_MOUNTS);
    assert_eq!(out.mount_truncated, 1);
    assert_eq!(mountpoint_of(&out.mounts[0]), b"/mnt/vol0");
    assert_eq!(mountpoint_of(&out.mounts[31]), b"/mnt/vol31");
}

#[cfg(target_os = "linux")]
#[test]
fn mountinfo_duplicate_mountpoint_skips_and_truncates() {
    let buf = b"36 24 8:1 / / rw,relatime - ext4 /dev/sda1 rw\n\
                \x2037 25 8:2 / / rw,relatime - ext4 /dev/sda2 rw\n";
    let mut out = empty_storage();
    linux::parse_mountinfo(buf, &mut out, &mut scripted_capacity).expect("parse");
    assert_eq!(out.mount_count, 1);
    assert_eq!(out.mount_truncated, 1);
}

#[cfg(target_os = "linux")]
#[test]
fn mountinfo_invalid_utf8_mountpoint_skips_and_truncates() {
    let buf = b"36 24 8:1 / /mnt/bad\xffpath rw,relatime - ext4 /dev/sda1 rw\n\
                \x2037 25 8:2 / /good rw,relatime - ext4 /dev/sda2 rw\n";
    let mut out = empty_storage();
    linux::parse_mountinfo(buf, &mut out, &mut scripted_capacity).expect("parse");
    assert_eq!(out.mount_count, 1);
    assert_eq!(mountpoint_of(&out.mounts[0]), b"/good");
    assert_eq!(out.mount_truncated, 1);
}

#[cfg(target_os = "linux")]
#[test]
fn mountinfo_oversized_mountpoint_skips_and_truncates() {
    let mut line = b"36 24 8:1 / /mnt/".to_vec();
    line.extend_from_slice(&[b'x'; 400]);
    line.extend_from_slice(b" rw,relatime - ext4 /dev/sda1 rw\n");
    let mut out = empty_storage();
    linux::parse_mountinfo(&line, &mut out, &mut scripted_capacity).expect("parse");
    assert_eq!(out.mount_count, 0);
    assert_eq!(out.mount_truncated, 1);
}

#[cfg(target_os = "linux")]
#[test]
fn mountinfo_oversized_fstype_skips_and_truncates() {
    let buf = b"36 24 8:1 / / rw,relatime - averyverylongfsttypename /dev/sda1 rw\n";
    let mut out = empty_storage();
    linux::parse_mountinfo(buf, &mut out, &mut scripted_capacity).expect("parse");
    assert_eq!(out.mount_count, 0);
    assert_eq!(out.mount_truncated, 1);
}

// ---------------------------------------------------------------------
// Normative mount capacity formula (shared by Linux statvfs and macOS)
// ---------------------------------------------------------------------

#[test]
fn mount_capacity_reserved_blocks_golden() {
    let capacity = FsCapacity {
        blocks: 1000,
        bfree: 600,
        bavail: 500,
        unit: 4096,
    };
    let (total, used, available, percent) = mount_capacity(&capacity).expect("valid capacity");
    assert_eq!(total, 4_096_000);
    assert_eq!(used, 1_638_400);
    assert_eq!(available, 2_048_000);
    assert_eq!(percent, 40.0);
    assert_ne!(
        used + available,
        total,
        "reserved blocks may separate used+available from total"
    );
}

#[test]
fn mount_capacity_zero_total_has_zero_percent() {
    let capacity = FsCapacity {
        blocks: 0,
        bfree: 0,
        bavail: 0,
        unit: 4096,
    };
    let (total, used, available, percent) = mount_capacity(&capacity).expect("zero capacity");
    assert_eq!((total, used, available), (0, 0, 0));
    assert_eq!(percent, 0.0);
}

#[test]
fn mount_capacity_overflow_and_inconsistent_inputs_return_none() {
    assert!(mount_capacity(&FsCapacity {
        blocks: u64::MAX,
        bfree: 0,
        bavail: 0,
        unit: 4096,
    })
    .is_none());
    assert!(mount_capacity(&FsCapacity {
        blocks: 100,
        bfree: 200,
        bavail: 0,
        unit: 1,
    })
    .is_none());
    assert!(mount_capacity(&FsCapacity {
        blocks: 100,
        bfree: 50,
        bavail: 500,
        unit: 1,
    })
    .is_none());
}

// ---------------------------------------------------------------------
// macOS getfsstat publication seam (platform-neutral record injection)
// ---------------------------------------------------------------------

fn mounted_sample(index: u32) -> MountSample {
    let mountpoint = format!("/mnt/vol{index}");
    MountSample::new(mountpoint.as_bytes(), b"apfs", 1000, 600, 500, 4096).expect("valid sample")
}

#[test]
fn publish_statfs_mounts_31_records_no_truncation() {
    let samples: Vec<MountSample> = (0..31).map(mounted_sample).collect();
    let mut out = empty_storage();
    publish_statfs_mounts(&samples, false, &mut out);
    assert_eq!(out.mount_count, 31);
    assert_eq!(out.mount_truncated, 0);
    assert_eq!(mountpoint_of(&out.mounts[30]), b"/mnt/vol30");
}

#[test]
fn publish_statfs_mounts_32_records_no_truncation() {
    let samples: Vec<MountSample> = (0..32).map(mounted_sample).collect();
    let mut out = empty_storage();
    publish_statfs_mounts(&samples, false, &mut out);
    assert_eq!(out.mount_count, 32);
    assert_eq!(out.mount_truncated, 0);
}

#[test]
fn publish_statfs_mounts_33_records_publishes_32_and_truncates() {
    let samples: Vec<MountSample> = (0..33).map(mounted_sample).collect();
    let mut out = empty_storage();
    publish_statfs_mounts(&samples, false, &mut out);
    assert_eq!(out.mount_count, 32);
    assert_eq!(out.mount_truncated, 1);
    assert_eq!(mountpoint_of(&out.mounts[31]), b"/mnt/vol31");
}

#[test]
fn publish_statfs_mounts_invalid_record_skips_and_truncates() {
    let mut samples: Vec<MountSample> = (0..3).map(mounted_sample).collect();
    let mut invalid = MountSample::new(b"/mnt/ctrl", b"apfs", 1, 1, 1, 1).expect("sample");
    invalid.mountpoint[5] = 0x07;
    samples.push(invalid);
    let mut out = empty_storage();
    publish_statfs_mounts(&samples, false, &mut out);
    assert_eq!(out.mount_count, 3);
    assert_eq!(out.mount_truncated, 1);
}

#[test]
fn publish_statfs_mounts_clears_stale_slots() {
    let samples: Vec<MountSample> = (0..2).map(mounted_sample).collect();
    let mut out = empty_storage();
    out.mount_count = 5;
    out.mounts[4].total = 9_999;
    out.mounts[4].mountpoint[0] = b'x';
    publish_statfs_mounts(&samples, false, &mut out);
    assert_eq!(out.mount_count, 2);
    assert_eq!(out.mounts[4].total, 0);
    assert_eq!(out.mounts[4].mountpoint[0], 0);
}

#[test]
fn storage_availability_capability_mask() {
    let full = StorageAvailability {
        disk_metrics: true,
        mounts: true,
    };
    assert_eq!(full.capability_mask(), DISK_CAPS | CAP_STORAGE_MOUNTS);

    let mounts_only = StorageAvailability {
        disk_metrics: false,
        mounts: true,
    };
    assert_eq!(mounts_only.capability_mask(), CAP_STORAGE_MOUNTS);
    assert_eq!(mounts_only.capability_mask() & DISK_CAPS, 0);

    let none = StorageAvailability {
        disk_metrics: false,
        mounts: false,
    };
    assert_eq!(none.capability_mask(), 0);
}

#[test]
fn macos_storage_source_uses_fixed_getfsstat_buffer() {
    let source = include_str!("../src/collectors/storage/macos.rs");
    assert!(source.contains("getfsstat"), "macOS mounts use getfsstat");
    assert!(source.contains("MNT_NOWAIT"), "getfsstat uses MNT_NOWAIT");
    assert!(
        source.contains("MAX_MOUNTS + 1"),
        "fixed 33-record buffer acts as the overflow sentinel"
    );
}

// ---------------------------------------------------------------------
// Disk baseline map semantics
// ---------------------------------------------------------------------

#[test]
fn disk_baseline_map_insert_probe_and_sweep() {
    let mut map = DiskBaselineMap::zero();
    let key = DiskKey { major: 8, minor: 0 };
    assert!(map.get(&key).is_none());

    let counters = DiskRawSnapshot {
        sectors_read: 20480,
        sectors_written: 81920,
        reads_completed: 1000,
        read_ms: 500,
        writes_completed: 2000,
        write_ms: 800,
    };
    assert!(map.insert(key, 1, counters));
    let slot = map.get(&key).expect("inserted");
    assert_eq!(slot.counters.sectors_read, 20480);
    assert_eq!(slot.counters.writes_completed, 2000);

    map.sweep(2);
    assert!(map.get(&key).is_none(), "stale generation swept");

    for index in 0..DISK_MAP_CAPACITY {
        let filled_key = DiskKey {
            major: 8,
            minor: index as u32,
        };
        assert!(map.insert(filled_key, 3, DiskRawSnapshot::zero()));
    }
    let overflow_key = DiskKey {
        major: 9,
        minor: 999,
    };
    assert!(!map.insert(overflow_key, 3, counters));
}

// ---------------------------------------------------------------------
// Finalize-driven disk rate semantics
// ---------------------------------------------------------------------

struct StepClock {
    monotonic_ns: u64,
}

impl Clock for StepClock {
    fn sample(&mut self) -> aura_common::AuraResult<ClockSample> {
        let current = self.monotonic_ns;
        self.monotonic_ns += 1_000_000_000;
        Ok(ClockSample {
            monotonic_ns: current,
            wallclock_ns: current + 1_000,
        })
    }
}

fn seed_disk(
    state: &mut FixedCollectorState,
    index: usize,
    major: u32,
    minor: u32,
    name: &[u8],
    raw: DiskRawSnapshot,
    queue_depth: u32,
) {
    let storage = &mut state.archive.storage;
    storage.disks[index].name = FixedString16::from_bytes(name);
    storage.disks[index].major = major;
    storage.disks[index].minor = minor;
    storage.disks[index].read_bytes = raw.sectors_read * 512;
    storage.disks[index].write_bytes = raw.sectors_written * 512;
    storage.disks[index].queue_depth = queue_depth;
    state.disk_raw[index] = raw;
}

fn raw_snapshot(
    reads: u64,
    sectors_read: u64,
    read_ms: u64,
    writes: u64,
    sectors_written: u64,
    write_ms: u64,
) -> DiskRawSnapshot {
    DiskRawSnapshot {
        sectors_read,
        sectors_written,
        reads_completed: reads,
        read_ms,
        writes_completed: writes,
        write_ms,
    }
}

fn storage_state() -> FixedCollectorState {
    let mut state = FixedCollectorState::default();
    state.archive.capabilities = DISK_CAPS | CAP_STORAGE_MOUNTS;
    state
}

#[test]
fn first_cycle_zeroes_disk_rates_and_seeds_baseline() {
    let mut state = storage_state();
    state.archive.storage.disk_count = 1;
    seed_disk(
        &mut state,
        0,
        8,
        0,
        b"sda",
        raw_snapshot(100, 2000, 200, 50, 1000, 100),
        3,
    );

    let mut finalizer = SystemFinalizer::new(StepClock {
        monotonic_ns: 1_000_000_000,
    });
    finalizer.finalize(&mut state).expect("first finalize");

    let disk = &state.archive.storage.disks[0];
    assert_eq!(disk.read_bytes_per_sec, 0.0);
    assert_eq!(disk.write_bytes_per_sec, 0.0);
    assert_eq!(disk.read_iops, 0.0);
    assert_eq!(disk.write_iops, 0.0);
    assert_eq!(disk.read_latency_ms, 0.0);
    assert_eq!(disk.write_latency_ms, 0.0);
    assert_eq!(disk.queue_depth, 3, "queue depth is a current gauge");
    assert_eq!(disk.read_bytes, 2000 * 512, "cumulative bytes preserved");

    let key = DiskKey { major: 8, minor: 0 };
    let slot = state.baselines.disk.get(&key).expect("baseline seeded");
    assert_eq!(slot.counters.sectors_read, 2000);
}

#[test]
fn second_cycle_computes_exact_rates_iops_and_latency() {
    let mut state = storage_state();
    let mut finalizer = SystemFinalizer::new(StepClock {
        monotonic_ns: 1_000_000_000,
    });

    state.archive.storage.disk_count = 1;
    seed_disk(
        &mut state,
        0,
        8,
        0,
        b"sda",
        raw_snapshot(100, 2000, 200, 50, 1000, 100),
        3,
    );
    finalizer.finalize(&mut state).expect("warm cycle");

    seed_disk(
        &mut state,
        0,
        8,
        0,
        b"sda",
        raw_snapshot(150, 2800, 300, 70, 1400, 160),
        7,
    );
    finalizer.finalize(&mut state).expect("measured cycle");

    let disk = &state.archive.storage.disks[0];
    assert_eq!(disk.read_bytes_per_sec, 409_600.0);
    assert_eq!(disk.write_bytes_per_sec, 204_800.0);
    assert_eq!(disk.read_iops, 50.0);
    assert_eq!(disk.write_iops, 20.0);
    assert_eq!(disk.read_latency_ms, 2.0);
    assert_eq!(disk.write_latency_ms, 3.0);
    assert_eq!(disk.queue_depth, 7);
}

#[test]
fn counter_reset_zeroes_rates_for_one_cycle_and_reseeds() {
    let mut state = storage_state();
    let mut finalizer = SystemFinalizer::new(StepClock {
        monotonic_ns: 1_000_000_000,
    });

    state.archive.storage.disk_count = 1;
    seed_disk(
        &mut state,
        0,
        8,
        0,
        b"sda",
        raw_snapshot(100, 2000, 200, 50, 1000, 100),
        1,
    );
    finalizer.finalize(&mut state).expect("warm cycle");
    seed_disk(
        &mut state,
        0,
        8,
        0,
        b"sda",
        raw_snapshot(150, 2800, 300, 70, 1400, 160),
        1,
    );
    finalizer.finalize(&mut state).expect("baseline cycle");
    assert_eq!(state.archive.storage.disks[0].read_iops, 50.0);

    // Counter reset: every counter drops below the baseline.
    seed_disk(
        &mut state,
        0,
        8,
        0,
        b"sda",
        raw_snapshot(10, 400, 30, 8, 200, 12),
        0,
    );
    finalizer.finalize(&mut state).expect("reset cycle");
    let disk = &state.archive.storage.disks[0];
    assert_eq!(disk.read_bytes_per_sec, 0.0);
    assert_eq!(disk.write_bytes_per_sec, 0.0);
    assert_eq!(disk.read_iops, 0.0);
    assert_eq!(disk.write_iops, 0.0);
    assert_eq!(disk.read_latency_ms, 0.0);
    assert_eq!(disk.write_latency_ms, 0.0);

    // Next cycle derives from the reseeded baseline.
    seed_disk(
        &mut state,
        0,
        8,
        0,
        b"sda",
        raw_snapshot(30, 800, 70, 18, 400, 32),
        0,
    );
    finalizer.finalize(&mut state).expect("post-reset cycle");
    let disk = &state.archive.storage.disks[0];
    assert_eq!(disk.read_bytes_per_sec, 204_800.0);
    assert_eq!(disk.read_iops, 20.0);
    assert_eq!(disk.read_latency_ms, 2.0);
}

#[test]
fn hotplug_device_appears_and_disappears_without_stale_baseline() {
    let mut state = storage_state();
    let mut finalizer = SystemFinalizer::new(StepClock {
        monotonic_ns: 1_000_000_000,
    });

    state.archive.storage.disk_count = 1;
    seed_disk(
        &mut state,
        0,
        8,
        0,
        b"sda",
        raw_snapshot(100, 2000, 200, 50, 1000, 100),
        1,
    );
    finalizer.finalize(&mut state).expect("cycle 1");

    // Hotplug: nvme0n1 appears alongside sda; the new device gets a zero-rate seed.
    state.archive.storage.disk_count = 2;
    seed_disk(
        &mut state,
        0,
        8,
        0,
        b"sda",
        raw_snapshot(150, 2800, 300, 70, 1400, 160),
        1,
    );
    seed_disk(
        &mut state,
        1,
        259,
        0,
        b"nvme0n1",
        raw_snapshot(20, 400, 10, 5, 100, 4),
        0,
    );
    finalizer.finalize(&mut state).expect("cycle 2");
    assert_eq!(state.archive.storage.disks[0].read_iops, 50.0);
    assert_eq!(state.archive.storage.disks[1].read_iops, 0.0);
    assert_eq!(state.archive.storage.disks[1].read_bytes_per_sec, 0.0);

    // sda disappears; its baseline is swept and nvme0n1 keeps its identity.
    state.archive.storage.disks[0] = TelemetryArchive::zeroed().storage.disks[0];
    state.archive.storage.disk_count = 1;
    seed_disk(
        &mut state,
        0,
        259,
        0,
        b"nvme0n1",
        raw_snapshot(40, 900, 30, 15, 300, 14),
        2,
    );
    finalizer.finalize(&mut state).expect("cycle 3");

    let sda_key = DiskKey { major: 8, minor: 0 };
    assert!(state.baselines.disk.get(&sda_key).is_none());

    let nvme = &state.archive.storage.disks[0];
    assert_eq!(nvme.name.as_str(), "nvme0n1");
    assert_eq!(nvme.read_bytes_per_sec, 256_000.0);
    assert_eq!(nvme.read_iops, 20.0);
    assert_eq!(nvme.read_latency_ms, 1.0);
    assert_eq!(nvme.queue_depth, 2);
}

#[test]
fn idle_device_reports_zero_latency_with_capability_set() {
    let mut state = storage_state();
    let mut finalizer = SystemFinalizer::new(StepClock {
        monotonic_ns: 1_000_000_000,
    });

    state.archive.storage.disk_count = 1;
    seed_disk(
        &mut state,
        0,
        8,
        0,
        b"sda",
        raw_snapshot(100, 2000, 200, 50, 1000, 100),
        0,
    );
    finalizer.finalize(&mut state).expect("warm cycle");
    seed_disk(
        &mut state,
        0,
        8,
        0,
        b"sda",
        raw_snapshot(100, 2000, 200, 50, 1000, 100),
        0,
    );
    finalizer.finalize(&mut state).expect("idle cycle");

    let disk = &state.archive.storage.disks[0];
    assert_eq!(disk.read_iops, 0.0);
    assert_eq!(disk.read_latency_ms, 0.0, "no I/O delta keeps latency zero");
    assert_eq!(disk.write_latency_ms, 0.0);
    assert_ne!(
        state.archive.capabilities & CAP_STORAGE_DISK_LATENCY,
        0,
        "capability remains set with zero latency"
    );
}

// ---------------------------------------------------------------------
// Production wiring + allocation discipline
// ---------------------------------------------------------------------

#[cfg(target_os = "linux")]
#[test]
fn deterministic_sources_populate_storage_and_validate() {
    use aura_daemon::lifecycle::Heartbeat;
    use support::system_sources::SOURCE_CAPABILITIES;
    use support::transaction::allocation_lifecycle;

    let mut lifecycle = allocation_lifecycle();
    let heartbeat = Heartbeat::from_millis(1).expect("heartbeat");
    lifecycle.warm_up(heartbeat).expect("warm-up");
    lifecycle.cycle().expect("cycle");

    let archive = &lifecycle.state().committed().archive;
    assert_eq!(archive.capabilities & DISK_CAPS, DISK_CAPS);
    assert_ne!(archive.capabilities & CAP_STORAGE_MOUNTS, 0);
    assert_eq!(
        archive.capabilities & SOURCE_CAPABILITIES,
        SOURCE_CAPABILITIES
    );
    assert!(archive.storage.disk_count > 0);
    assert!(archive.storage.mount_count > 0);
    aura_common::validate_archive(archive).expect("storage archive validates");
}

#[cfg(target_os = "linux")]
mod alloc_probe {
    use std::alloc::{GlobalAlloc, Layout, System};
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    pub struct CountingAllocator;

    static ALLOCATIONS: AtomicUsize = AtomicUsize::new(0);
    static ACTIVE: AtomicBool = AtomicBool::new(false);

    unsafe impl GlobalAlloc for CountingAllocator {
        unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
            if ACTIVE.load(Ordering::Relaxed) {
                ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
            }
            unsafe { System.alloc(layout) }
        }
        unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
            unsafe { System.dealloc(ptr, layout) }
        }
    }

    pub fn start() {
        ALLOCATIONS.store(0, Ordering::Relaxed);
        ACTIVE.store(true, Ordering::Relaxed);
    }

    pub fn finish() -> usize {
        ACTIVE.store(false, Ordering::Relaxed);
        ALLOCATIONS.load(Ordering::Relaxed)
    }
}

#[cfg(target_os = "linux")]
#[global_allocator]
static STORAGE_TEST_ALLOCATOR: alloc_probe::CountingAllocator = alloc_probe::CountingAllocator;

#[cfg(target_os = "linux")]
#[test]
fn storage_collection_allocates_zero_after_warmup() {
    const CHILD_MARKER: &str = "AURA_STORAGE_ALLOC_PROBE_CHILD";
    if std::env::var_os(CHILD_MARKER).is_none() {
        let output = std::process::Command::new(std::env::current_exe().expect("test executable"))
            .arg("--exact")
            .arg("storage_collection_allocates_zero_after_warmup")
            .arg("--test-threads=1")
            .env(CHILD_MARKER, "1")
            .output()
            .expect("run isolated allocation probe");
        assert!(
            output.status.success(),
            "isolated allocation probe failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        return;
    }

    use aura_daemon::lifecycle::Heartbeat;
    use support::transaction::allocation_lifecycle;

    let mut lifecycle = allocation_lifecycle();
    let heartbeat = Heartbeat::from_millis(1).expect("heartbeat");
    lifecycle.warm_up(heartbeat).expect("warm-up");
    lifecycle.cycle().expect("warm cycle");

    alloc_probe::start();
    lifecycle.cycle().expect("measured cycle 1");
    lifecycle.cycle().expect("measured cycle 2");
    let calls = alloc_probe::finish();
    assert_eq!(calls, 0, "steady-state storage cycle must not allocate");
    assert_eq!(
        lifecycle.collector().sources().calls,
        [4; 6],
        "storage source ran in every cycle"
    );
}
