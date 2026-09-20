//! macOS storage collection. Mounts come from a single fixed-buffer
//! `getfsstat` call with `MNT_NOWAIT`; the 33rd record slot is the overflow
//! sentinel. Disk counters are compile-time unavailable (no documented
//! public API) and always zeroed.

use aura_common::{AuraError, AuraResult, StorageStats, MAX_MOUNTS};

use super::{publish_statfs_mounts, MountSample, EMPTY_DISK};

const GETFSSTAT_CAPACITY: usize = MAX_MOUNTS + 1;

pub fn collect(out: &mut StorageStats) -> AuraResult<()> {
    for slot in out.disks.iter_mut() {
        *slot = EMPTY_DISK;
    }
    out.disk_count = 0;
    out.disk_truncated = 0;

    let mut raw: [std::mem::MaybeUninit<libc::statfs>; GETFSSTAT_CAPACITY] =
        std::array::from_fn(|_| std::mem::MaybeUninit::uninit());
    let bytes = (std::mem::size_of::<libc::statfs>() * GETFSSTAT_CAPACITY) as libc::c_int;
    // SAFETY: `raw` is writable storage for exactly GETFSSTAT_CAPACITY statfs
    // records and `bytes` is that same extent; MNT_NOWAIT forbids blocking.
    let returned = unsafe {
        libc::getfsstat(
            raw.as_mut_ptr() as *mut libc::statfs,
            bytes,
            libc::MNT_NOWAIT,
        )
    };
    if returned < 0 {
        return Err(AuraError::Fatal(format!(
            "getfsstat failed: {}",
            std::io::Error::last_os_error()
        )));
    }
    let returned = (returned as usize).min(GETFSSTAT_CAPACITY);

    let mut samples = [MountSample::zero(); GETFSSTAT_CAPACITY];
    let mut valid = 0usize;
    let mut record_failure = false;
    for record in raw.iter().take(returned) {
        // SAFETY: getfsstat initialized the first `returned` records.
        let statfs = unsafe { record.assume_init_ref() };
        match sample_from_statfs(statfs) {
            Some(sample) => {
                samples[valid] = sample;
                valid += 1;
            }
            None => record_failure = true,
        }
    }
    publish_statfs_mounts(&samples[..valid], record_failure, out);
    Ok(())
}

fn sample_from_statfs(statfs: &libc::statfs) -> Option<MountSample> {
    MountSample::new(
        cstr_bytes(&statfs.f_mntonname),
        cstr_bytes(&statfs.f_fstypename),
        statfs.f_blocks,
        statfs.f_bfree,
        statfs.f_bavail,
        u64::from(statfs.f_bsize),
    )
}

fn cstr_bytes(field: &[libc::c_char]) -> &[u8] {
    let len = field.iter().position(|&c| c == 0).unwrap_or(field.len());
    // SAFETY: c_char and u8 share size and alignment; the returned slice is a
    // reinterpretation of the field prefix up to (excluding) the first NUL.
    unsafe { std::slice::from_raw_parts(field.as_ptr() as *const u8, len) }
}
