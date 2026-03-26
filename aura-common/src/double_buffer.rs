use std::mem::MaybeUninit;
use std::sync::atomic::{fence, AtomicU64, Ordering};

use crate::{TelemetryArchive, BUFFER_0_OFFSET, BUFFER_1_OFFSET};

#[repr(C)]
pub struct DoubleBufferHeader {
    pub active_index: AtomicU64,
    pub write_seq: AtomicU64,
}

/// Write to inactive buffer, then atomically flip active index.
///
/// # Safety
/// Caller must provide a valid writable shared-memory base pointer containing
/// a `DoubleBufferHeader` at offset 0 and two `TelemetryArchive` buffers.
#[inline]
pub unsafe fn write_double_buffer(base: *mut u8, archive: &TelemetryArchive) {
    let header = unsafe { &*(base as *const DoubleBufferHeader) };
    let active = header.active_index.load(Ordering::Relaxed) & 1;
    let inactive = 1 - active;

    let offset = if inactive == 0 {
        BUFFER_0_OFFSET
    } else {
        BUFFER_1_OFFSET
    };
    let dst = unsafe { base.add(offset) };

    unsafe {
        std::ptr::copy_nonoverlapping(
            archive as *const TelemetryArchive as *const u8,
            dst,
            std::mem::size_of::<TelemetryArchive>(),
        );
    }

    fence(Ordering::Release);
    header.active_index.store(inactive, Ordering::Release);
    header.write_seq.fetch_add(1, Ordering::Release);
}

/// Read from active buffer. Returns `Err(())` if writer changed snapshots
/// during read after bounded retries. Caller maps `Err(())` to `AuraError::SeqLockInvalid`.
///
/// # Safety
/// Caller must provide a valid readable shared-memory base pointer containing
/// a `DoubleBufferHeader` at offset 0 and two initialized `TelemetryArchive`
/// buffers.
#[inline]
#[allow(clippy::result_unit_err)]
pub unsafe fn read_double_buffer(base: *const u8) -> Result<TelemetryArchive, ()> {
    let header = unsafe { &*(base as *const DoubleBufferHeader) };

    for _ in 0..3 {
        let seq1 = header.write_seq.load(Ordering::Acquire);
        let active = header.active_index.load(Ordering::Acquire) & 1;
        let offset = if active == 0 {
            BUFFER_0_OFFSET
        } else {
            BUFFER_1_OFFSET
        };

        let src = unsafe { base.add(offset) };
        let mut archive = MaybeUninit::<TelemetryArchive>::uninit();
        unsafe {
            std::ptr::copy_nonoverlapping(
                src,
                archive.as_mut_ptr() as *mut u8,
                std::mem::size_of::<TelemetryArchive>(),
            );
        }

        fence(Ordering::Acquire);
        let seq2 = header.write_seq.load(Ordering::Acquire);

        if seq1 == seq2 {
            return Ok(unsafe { archive.assume_init() });
        }
    }

    Err(())
}
