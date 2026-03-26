use std::hint::spin_loop;
use std::sync::atomic::{fence, AtomicUsize, Ordering};

use bytemuck::Pod;

use crate::{AuraError, AuraResult};

pub struct SeqLockWriterGuard<'a> {
    version: &'a AtomicUsize,
    owned: bool,
}

impl<'a> SeqLockWriterGuard<'a> {
    #[inline]
    pub fn begin(version: &'a AtomicUsize) -> Self {
        version.fetch_add(1, Ordering::SeqCst);
        Self {
            version,
            owned: true,
        }
    }

    #[inline]
    pub fn complete(mut self) {
        self.version.fetch_add(1, Ordering::Release);
        self.owned = false;
    }
}

impl<'a> Drop for SeqLockWriterGuard<'a> {
    fn drop(&mut self) {
        if self.owned {
            self.version.fetch_add(1, Ordering::Release);
        }
    }
}

/// Reads a seqlock-protected value with spin-wait retry logic.
///
/// # Safety
/// The `data_ptr` must be non-null, properly aligned, and point to valid initialized memory
/// for the lifetime of the operation. The `version_ptr` must be synchronized with `data_ptr`.
#[inline]
pub unsafe fn read_seqlock<T: Pod>(version_ptr: &AtomicUsize, data_ptr: *const T) -> AuraResult<T> {
    let mut spin_count = 0;
    const MAX_SPINS: usize = 10_000;

    loop {
        let v = version_ptr.load(Ordering::SeqCst);

        if v & 1 == 1 {
            spin_loop();
            spin_count += 1;
            if spin_count > MAX_SPINS {
                return Err(AuraError::SeqLockInvalid);
            }
            continue;
        }

        let v1 = v;

        fence(Ordering::Acquire);

        let result = unsafe { std::ptr::read_volatile(data_ptr) };

        fence(Ordering::Acquire);
        let v2 = version_ptr.load(Ordering::SeqCst);

        if v1 == v2 {
            return Ok(result);
        }

        spin_count = 0;
    }
}

/// Writes a seqlock-protected value with version increment protocol.
///
/// # Safety
/// The `data_ptr` must be non-null, properly aligned, and point to valid memory
/// capable of holding a serialized `T`. The `version_ptr` must be synchronized with `data_ptr`.
#[inline]
pub unsafe fn write_seqlock<T>(
    version_ptr: &mut AtomicUsize,
    data_ptr: *mut T,
    value: &T,
) -> AuraResult<()>
where
    T: Pod,
{
    version_ptr.fetch_add(1, Ordering::SeqCst);

    fence(Ordering::Release);

    unsafe { std::ptr::write_volatile(data_ptr, *value) };

    fence(Ordering::Release);

    version_ptr.fetch_add(1, Ordering::SeqCst);

    Ok(())
}

#[inline]
pub fn validate_freshness(timestamp_ns: u64, threshold_ns: u64) -> AuraResult<()> {
    let now = crate::time::monotonic_ns();
    let age_ns = now.saturating_sub(timestamp_ns);

    if age_ns > threshold_ns {
        Err(AuraError::StaleData {
            age_ms: age_ns / 1_000_000,
            threshold_ms: threshold_ns / 1_000_000,
        })
    } else {
        Ok(())
    }
}
