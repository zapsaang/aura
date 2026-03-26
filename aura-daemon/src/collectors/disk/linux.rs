use std::ffi::CString;
use std::fs::File;
use std::io::Read;

use aura_common::{
    AuraResult, DiskStat, FixedString16, MountStat, StorageStats, MAX_DISKS, MAX_MOUNTS,
};

use crate::collectors::parsing::{parse_u64, split_whitespace};
use crate::collectors::DiskSectorSnapshot;

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
            _pad0: [0; 4],
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

pub fn collect(
    diskstats_buf: &mut Vec<u8>,
    mounts_buf: &mut Vec<u8>,
    out: &mut StorageStats,
    prev: &mut DiskSectorSnapshot,
    delta_secs: f64,
) -> AuraResult<()> {
    diskstats_buf.clear();
    let mut f = File::open("/proc/diskstats")?;
    f.read_to_end(diskstats_buf)?;
    parse_diskstats(&diskstats_buf[..], &mut out.disks, &mut out.disk_count)?;

    mounts_buf.clear();
    let mut f2 = File::open("/proc/mounts")?;
    f2.read_to_end(mounts_buf)?;
    parse_mounts(&mounts_buf[..], &mut out.mounts, &mut out.mount_count)?;

    let count = out.disk_count as usize;
    let mut i = 0usize;
    while i < count && i < MAX_DISKS {
        let rx = out.disks[i].rx_bytes;
        let wx = out.disks[i].wx_bytes;
        let (prx, pwx) = prev.devices[i];
        out.disks[i].rx_per_sec = if delta_secs > 0.0 {
            (rx.saturating_sub(prx) as f64 / delta_secs) as f32
        } else {
            0.0
        };
        out.disks[i].wx_per_sec = if delta_secs > 0.0 {
            (wx.saturating_sub(pwx) as f64 / delta_secs) as f32
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
        let fixture = include_bytes!("../../../tests/fixtures/proc_diskstats_sample.txt");
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
        let fixture = include_bytes!("../../../tests/fixtures/proc_mounts_sample.txt");
        let mut mounts = [MountStat {
            mountpoint: [0; 256],
            fstype: aura_common::FixedString16::new(),
            total: 0,
            available: 0,
            used: 0,
            percent: 0.0,
            _pad0: [0; 4],
        }; MAX_MOUNTS];
        let mut count = 0u16;
        parse_mounts(fixture, &mut mounts, &mut count).expect("parse");
        assert!(count >= 2);
        assert_eq!(mounts[0].fstype.as_str(), "ext4");
    }
}
