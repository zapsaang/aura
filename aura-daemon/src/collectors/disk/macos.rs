use std::ffi::{CStr, CString};

use aura_common::{
    AuraResult, DiskStat, FixedString16, MountStat, StorageStats, MAX_DISKS, MAX_MOUNTS,
};

use crate::collectors::DiskSectorSnapshot;

fn should_skip_mount(mountpoint: &[u8], fstype: &[u8]) -> bool {
    mountpoint.starts_with(b"/dev")
        || mountpoint.starts_with(b"/net")
        || fstype == b"devfs"
        || fstype == b"autofs"
        || fstype == b"procfs"
        || fstype == b"linprocfs"
}

fn disk_name_from_device(device: &[u8]) -> &[u8] {
    let last = device.rsplit(|b| *b == b'/').next().unwrap_or(device);
    if last.is_empty() {
        b"disk"
    } else {
        last
    }
}

pub fn collect(
    _diskstats_buf: &mut Vec<u8>,
    _mounts_buf: &mut Vec<u8>,
    out: &mut StorageStats,
    prev: &mut DiskSectorSnapshot,
    _delta_secs: f64,
) -> AuraResult<()> {
    out.disk_count = 0;
    out.mount_count = 0;

    let mut mounts_ptr: *mut libc::statfs = std::ptr::null_mut();
    let mount_count = unsafe { libc::getmntinfo(&mut mounts_ptr, libc::MNT_NOWAIT) };

    if mount_count <= 0 || mounts_ptr.is_null() {
        prev.count = 0;
        return Ok(());
    }

    let mounts = unsafe { std::slice::from_raw_parts(mounts_ptr, mount_count as usize) };
    let mut disk_index = 0usize;
    let mut mount_index = 0usize;

    for mount in mounts {
        let mountpoint = unsafe { CStr::from_ptr(mount.f_mntonname.as_ptr()) }.to_bytes();
        let fstype = unsafe { CStr::from_ptr(mount.f_fstypename.as_ptr()) }.to_bytes();

        if should_skip_mount(mountpoint, fstype) {
            continue;
        }

        if mount_index < MAX_MOUNTS {
            let mut mp = [0u8; 256];
            let mp_len = mountpoint.len().min(255);
            mp[..mp_len].copy_from_slice(&mountpoint[..mp_len]);

            let (total, available, used, percent) = get_fs_stats(mountpoint);
            out.mounts[mount_index] = MountStat {
                mountpoint: mp,
                fstype: FixedString16::from_bytes(fstype),
                total,
                available,
                used,
                percent,
                _pad0: [0; 4],
            };
            mount_index += 1;
        }

        if disk_index < MAX_DISKS {
            let from_device = unsafe { CStr::from_ptr(mount.f_mntfromname.as_ptr()) }.to_bytes();
            out.disks[disk_index] = DiskStat {
                name: FixedString16::from_bytes(disk_name_from_device(from_device)),
                rx_bytes: 0,
                wx_bytes: 0,
                rx_per_sec: 0.0,
                wx_per_sec: 0.0,
            };
            prev.devices[disk_index] = (0, 0);
            disk_index += 1;
        }

        if mount_index >= MAX_MOUNTS && disk_index >= MAX_DISKS {
            break;
        }
    }

    out.mount_count = mount_index as u16;
    out.disk_count = disk_index as u8;
    prev.count = disk_index;
    Ok(())
}

fn get_fs_stats(mountpoint: &[u8]) -> (u64, u64, u64, f32) {
    if mountpoint.is_empty() {
        return (0, 0, 0, 0.0);
    }

    let cpath = match CString::new(mountpoint) {
        Ok(v) => v,
        Err(_) => return (0, 0, 0, 0.0),
    };

    let mut s = std::mem::MaybeUninit::<libc::statvfs>::uninit();
    let rc = unsafe { libc::statvfs(cpath.as_ptr(), s.as_mut_ptr()) };
    if rc != 0 {
        return (0, 0, 0, 0.0);
    }

    let s = unsafe { s.assume_init() };
    #[allow(clippy::unnecessary_cast)]
    let frsize = s.f_frsize as u64;
    #[allow(clippy::unnecessary_cast)]
    let total = (s.f_blocks as u64).saturating_mul(frsize);
    #[allow(clippy::unnecessary_cast)]
    let available = (s.f_bavail as u64).saturating_mul(frsize);
    let used = total.saturating_sub(available);
    let percent = if total > 0 {
        ((used as f64 / total as f64) * 100.0) as f32
    } else {
        0.0
    };
    (total, available, used, percent)
}
