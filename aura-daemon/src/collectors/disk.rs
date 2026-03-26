use std::ffi::CString;
use std::fs::File;
use std::io::Read;

#[cfg(target_os = "macos")]
use std::ffi::CStr;

use aura_common::{
    AuraResult, DiskStat, FixedString16, MountStat, StorageStats, MAX_DISKS, MAX_MOUNTS,
};

use super::parsing::{parse_u64, split_whitespace};
use super::DiskSectorSnapshot;

#[cfg(target_os = "macos")]
fn should_skip_mount(mountpoint: &[u8], fstype: &[u8]) -> bool {
    mountpoint.starts_with(b"/dev")
        || mountpoint.starts_with(b"/net")
        || fstype == b"devfs"
        || fstype == b"autofs"
        || fstype == b"procfs"
        || fstype == b"linprocfs"
}

#[cfg(target_os = "macos")]
fn disk_name_from_device(device: &[u8]) -> &[u8] {
    let last = device.rsplit(|b| *b == b'/').next().unwrap_or(device);
    if last.is_empty() {
        b"disk"
    } else {
        last
    }
}

#[cfg(target_os = "macos")]
pub fn collect_macos(
    out: &mut StorageStats,
    prev: &mut DiskSectorSnapshot,
    _delta_secs: f32,
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

pub fn parse_diskstats(
    buf: &[u8],
    disks_out: &mut [DiskStat; MAX_DISKS],
    count_out: &mut u8,
) -> AuraResult<()> {
    let mut count = 0usize;
    let mut line_start = 0usize;

    for i in 0..buf.len() {
        if buf[i] != b'\n' {
            continue;
        }
        let line = &buf[line_start..i];
        line_start = i + 1;

        if line.is_empty() || count >= MAX_DISKS {
            continue;
        }

        let mut fields: [&[u8]; 16] = [&[]; 16];
        let mut field_count = 0usize;
        for tok in split_whitespace(line) {
            if field_count >= fields.len() {
                break;
            }
            fields[field_count] = tok;
            field_count += 1;
        }

        if field_count < 10 {
            continue;
        }

        let name = FixedString16::from_bytes(fields[2]);
        let rx_bytes = parse_u64(fields[5]).unwrap_or(0).saturating_mul(512);
        let wx_bytes = parse_u64(fields[9]).unwrap_or(0).saturating_mul(512);

        disks_out[count] = DiskStat {
            name,
            rx_bytes,
            wx_bytes,
            rx_per_sec: 0.0,
            wx_per_sec: 0.0,
        };
        count += 1;
    }

    *count_out = count as u8;
    Ok(())
}

pub fn parse_mounts(
    buf: &[u8],
    mounts_out: &mut [MountStat; MAX_MOUNTS],
    mount_count_out: &mut u16,
) -> AuraResult<()> {
    let mut count = 0usize;
    let mut line_start = 0usize;

    for i in 0..buf.len() {
        if buf[i] != b'\n' {
            continue;
        }
        let line = &buf[line_start..i];
        line_start = i + 1;

        if line.is_empty() || count >= MAX_MOUNTS {
            continue;
        }

        let mut fields: [&[u8]; 4] = [&[]; 4];
        let mut n = 0usize;
        for tok in split_whitespace(line) {
            if n >= fields.len() {
                break;
            }
            fields[n] = tok;
            n += 1;
        }
        if n < 3 {
            continue;
        }

        let mountpoint = fields[1];
        let fstype = fields[2];

        let mut mp = [0u8; 256];
        let mp_len = mountpoint.len().min(255);
        mp[..mp_len].copy_from_slice(&mountpoint[..mp_len]);

        let fs = FixedString16::from_bytes(fstype);
        let (total, available, used, percent) = get_fs_stats(mountpoint);

        mounts_out[count] = MountStat {
            mountpoint: mp,
            fstype: fs,
            total,
            available,
            used,
            percent,
        };
        count += 1;
    }

    *mount_count_out = count as u16;
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
    // Cast to u64 for cross-platform compatibility: macOS has u32 fields
    #[allow(clippy::unnecessary_cast)]
    let frsize = s.f_frsize as u64;
    #[allow(clippy::unnecessary_cast)]
    let total = (s.f_blocks as u64).saturating_mul(frsize);
    #[allow(clippy::unnecessary_cast)]
    let available = (s.f_bavail as u64).saturating_mul(frsize);
    let used = total.saturating_sub(available);
    let percent = if total > 0 {
        (used as f32 / total as f32) * 100.0
    } else {
        0.0
    };
    (total, available, used, percent)
}

pub fn collect(
    diskstats_buf: &mut [u8; 4096],
    mounts_buf: &mut [u8; 4096],
    out: &mut StorageStats,
    prev: &mut DiskSectorSnapshot,
    delta_secs: f32,
) -> AuraResult<()> {
    let mut f = File::open("/proc/diskstats")?;
    let n = f.read(diskstats_buf)?;
    parse_diskstats(&diskstats_buf[..n], &mut out.disks, &mut out.disk_count)?;

    let mut f2 = File::open("/proc/mounts")?;
    let n2 = f2.read(mounts_buf)?;
    parse_mounts(&mounts_buf[..n2], &mut out.mounts, &mut out.mount_count)?;

    let count = out.disk_count as usize;
    let mut i = 0usize;
    while i < count && i < MAX_DISKS {
        let rx = out.disks[i].rx_bytes;
        let wx = out.disks[i].wx_bytes;
        let (prx, pwx) = prev.devices[i];
        out.disks[i].rx_per_sec = if delta_secs > 0.0 {
            rx.saturating_sub(prx) as f32 / delta_secs
        } else {
            0.0
        };
        out.disks[i].wx_per_sec = if delta_secs > 0.0 {
            wx.saturating_sub(pwx) as f32 / delta_secs
        } else {
            0.0
        };
        prev.devices[i] = (rx, wx);
        i += 1;
    }
    prev.count = count;

    Ok(())
}

#[cfg(test)]
mod tests {
    use aura_common::{DiskStat, MountStat, MAX_DISKS, MAX_MOUNTS};

    use super::{parse_diskstats, parse_mounts};

    #[test]
    fn parse_diskstats_sample() {
        let fixture = include_bytes!("../../tests/fixtures/proc_diskstats_sample.txt");
        let mut disks = [DiskStat {
            name: aura_common::FixedString16::new(),
            rx_bytes: 0,
            wx_bytes: 0,
            rx_per_sec: 0.0,
            wx_per_sec: 0.0,
        }; MAX_DISKS];
        let mut count = 0u8;
        parse_diskstats(fixture, &mut disks, &mut count).expect("parse");
        assert_eq!(count, 2);
        assert_eq!(disks[0].name.as_str(), "vda");
        assert_eq!(disks[0].rx_bytes, 9012 * 512);
    }

    #[test]
    fn parse_mounts_sample() {
        let fixture = include_bytes!("../../tests/fixtures/proc_mounts_sample.txt");
        let mut mounts = [MountStat {
            mountpoint: [0; 256],
            fstype: aura_common::FixedString16::new(),
            total: 0,
            available: 0,
            used: 0,
            percent: 0.0,
        }; MAX_MOUNTS];
        let mut count = 0u16;
        parse_mounts(fixture, &mut mounts, &mut count).expect("parse");
        assert!(count >= 2);
        assert_eq!(mounts[0].fstype.as_str(), "ext4");
    }
}
