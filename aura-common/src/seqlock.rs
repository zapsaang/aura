use std::hint::spin_loop;
use std::sync::atomic::{compiler_fence, AtomicUsize, Ordering};

use rkyv::{to_bytes, Archive, Deserialize};

use crate::{AuraError, AuraResult};

#[inline]
pub fn read_seqlock<T: Archive + Deserialize<T, rkyv::Infallible>>(
    version_ptr: &AtomicUsize,
    data_ptr: *const T,
) -> AuraResult<T> {
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

        compiler_fence(Ordering::SeqCst);

        let archived = unsafe { &*data_ptr };
        let result = archived
            .deserialize(&mut rkyv::Infallible)
            .map_err(|_| AuraError::SeqLockInvalid)?;

        compiler_fence(Ordering::SeqCst);
        let v2 = version_ptr.load(Ordering::SeqCst);

        if v1 == v2 {
            return Ok(result);
        }

        spin_count = 0;
    }
}

#[inline]
pub fn write_seqlock<T: rkyv::Serialize<rkyv::ser::serializers::AllocSerializer<1024>>>(
    version_ptr: &mut AtomicUsize,
    data_ptr: *mut T,
    value: &T,
) -> AuraResult<()> {
    version_ptr.fetch_add(1, Ordering::SeqCst);

    compiler_fence(Ordering::SeqCst);

    let bytes = to_bytes::<T, 1024>(value)
        .map_err(|e| AuraError::MmapFailed(format!("Serialization failed: {:?}", e)))?;

    let archived_ptr = data_ptr as *mut u8;
    unsafe {
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), archived_ptr, bytes.len());
    }

    compiler_fence(Ordering::SeqCst);

    version_ptr.fetch_add(1, Ordering::SeqCst);

    Ok(())
}

#[inline]
pub fn validate_freshness(timestamp_ns: u64, threshold_ns: u64) -> AuraResult<()> {
    let now = std::time::Instant::now().elapsed().as_nanos() as u64;
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
