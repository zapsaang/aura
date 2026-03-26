use std::hint::spin_loop;
use std::sync::atomic::{compiler_fence, AtomicUsize, Ordering};

use rkyv::{
    ser::{serializers::BufferSerializer, Serializer},
    Archive, Deserialize,
};

use crate::{AuraError, AuraResult};

/// Reads a seqlock-protected value with spin-wait retry logic.
///
/// # Safety
/// The `data_ptr` must be non-null, properly aligned, and point to valid initialized memory
/// for the lifetime of the operation. The `version_ptr` must be synchronized with `data_ptr`.
#[inline]
pub unsafe fn read_seqlock<T: Archive>(
    version_ptr: &AtomicUsize,
    data_ptr: *const T::Archived,
) -> AuraResult<T>
where
    T::Archived: Deserialize<T, rkyv::Infallible>,
{
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

/// Writes a seqlock-protected value with version increment protocol.
///
/// # Safety
/// The `data_ptr` must be non-null, properly aligned, and point to valid memory
/// capable of holding a serialized `T`. The `version_ptr` must be synchronized with `data_ptr`.
#[inline]
pub unsafe fn write_seqlock<T>(
    version_ptr: &mut AtomicUsize,
    data_ptr: *mut T::Archived,
    value: &T,
) -> AuraResult<()>
where
    T: Archive,
    for<'a> T: rkyv::Serialize<BufferSerializer<&'a mut [u8]>>,
{
    version_ptr.fetch_add(1, Ordering::SeqCst);

    compiler_fence(Ordering::SeqCst);

    let result = (|| {
        let buf_len = std::mem::size_of::<T::Archived>();
        let buffer = unsafe { std::slice::from_raw_parts_mut(data_ptr.cast::<u8>(), buf_len) };
        let mut serializer = BufferSerializer::new(buffer);

        let root_pos = serializer
            .serialize_value(value)
            .map_err(|e| AuraError::MmapFailed(format!("Serialization failed: {e}")))?;

        if root_pos != 0 {
            return Err(AuraError::MmapFailed(format!(
                "Serialization root offset {root_pos} is unsupported for seqlock writes"
            )));
        }
        Ok(())
    })();

    compiler_fence(Ordering::SeqCst);

    version_ptr.fetch_add(1, Ordering::SeqCst);

    result
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
