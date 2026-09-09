//! Linux storage collection: `/proc/diskstats` plus `/proc/self/mountinfo`
//! read into one reused scratch buffer (sequentially), with `statvfs`
//! capacity sampling per published mount.

mod diskstats;
mod mountinfo;

pub use diskstats::parse_diskstats;
pub use mountinfo::parse_mountinfo;

use std::ffi::OsStr;
use std::fs::File;
use std::os::unix::ffi::OsStrExt;

use aura_common::{AuraResult, StorageStats, MAX_DISKS};

use super::state::DiskRawSnapshot;
use super::FsCapacity;
use crate::collectors::parsing::read_reused;

pub fn collect(
    buf: &mut Vec<u8>,
    out: &mut StorageStats,
    raw: &mut [DiskRawSnapshot; MAX_DISKS],
) -> AuraResult<()> {
    collect_from_paths(b"/proc/diskstats", b"/proc/self/mountinfo", buf, out, raw)
}

pub fn collect_from_paths(
    diskstats_path: &[u8],
    mountinfo_path: &[u8],
    buf: &mut Vec<u8>,
    out: &mut StorageStats,
    raw: &mut [DiskRawSnapshot; MAX_DISKS],
) -> AuraResult<()> {
    buf.clear();
    read_reused(&mut open_proc(diskstats_path)?, buf)?;
    parse_diskstats(&buf[..], out, raw)?;

    buf.clear();
    read_reused(&mut open_proc(mountinfo_path)?, buf)?;
    parse_mountinfo(&buf[..], out, &mut statvfs_capacity)?;
    Ok(())
}

fn open_proc(path: &[u8]) -> AuraResult<File> {
    Ok(File::open(OsStr::from_bytes(path))?)
}

/// `statvfs` capacity probe using `f_frsize` as the block unit (POSIX:
/// `f_blocks` counts `f_frsize` units). Any failure is record-local.
fn statvfs_capacity(path: &[u8]) -> Option<FsCapacity> {
    if path.is_empty() || path.len() > 255 {
        return None;
    }
    let mut c_path = [0u8; 256];
    c_path[..path.len()].copy_from_slice(path);
    let mut stats = std::mem::MaybeUninit::<libc::statvfs>::uninit();
    // SAFETY: `c_path` is a NUL-terminated path buffer and `stats` is valid
    // writable storage for one statvfs record.
    let rc = unsafe { libc::statvfs(c_path.as_ptr() as *const libc::c_char, stats.as_mut_ptr()) };
    if rc != 0 {
        return None;
    }
    // SAFETY: statvfs returned success and initialized `stats`.
    let stats = unsafe { stats.assume_init() };
    #[allow(clippy::unnecessary_cast)]
    Some(FsCapacity {
        blocks: stats.f_blocks as u64,
        bfree: stats.f_bfree as u64,
        bavail: stats.f_bavail as u64,
        unit: stats.f_frsize as u64,
    })
}
