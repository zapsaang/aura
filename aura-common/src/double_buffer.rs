use std::mem::MaybeUninit;
use std::sync::atomic::{fence, AtomicU64, Ordering};

use crate::{TelemetryArchive, BUFFER_0_OFFSET, BUFFER_1_OFFSET};

#[repr(C)]
pub struct DoubleBufferHeader {
    pub active_index: AtomicU64,
    pub write_seq: AtomicU64,
}

const ARCHIVE_SIZE: usize = std::mem::size_of::<TelemetryArchive>();
const _: () = assert!(
    ARCHIVE_SIZE % 8 == 0,
    "TelemetryArchive must be 8-byte aligned for atomic copy"
);

/// Atomically read `len` bytes from shared memory using Relaxed u64 loads.
/// Each u64 is read atomically, preventing torn reads under concurrent writes.
///
/// # Safety
/// - `src` must be 8-byte aligned and point to valid memory
/// - `dst` must be 8-byte aligned and point to valid writable memory
/// - `len` must be a multiple of 8
/// - `src` must not be modified by non-atomic operations while this runs
#[inline]
unsafe fn atomic_read_shm(src: *mut u8, dst: *mut u8, len: usize) {
    debug_assert_eq!(len % 8, 0);
    let chunks = len / 8;
    let src_u64 = src as *mut u64;
    let dst_u64 = dst as *mut u64;
    for i in 0..chunks {
        let val = AtomicU64::from_ptr(src_u64.add(i)).load(Ordering::Relaxed);
        dst_u64.add(i).write(val);
    }
}

/// Atomically write `len` bytes to shared memory using Relaxed u64 stores.
/// Each u64 is stored atomically, preventing torn writes under concurrent reads.
///
/// # Safety
/// - `src` must be 8-byte aligned and point to valid memory
/// - `dst` must be 8-byte aligned and point to valid writable shared memory
/// - `len` must be a multiple of 8
/// - `dst` must not be accessed by non-atomic operations while this runs
#[inline]
unsafe fn atomic_write_shm(src: *const u8, dst: *mut u8, len: usize) {
    debug_assert_eq!(len % 8, 0);
    let chunks = len / 8;
    let src_u64 = src as *const u64;
    let dst_u64 = dst as *mut u64;
    for i in 0..chunks {
        let val = src_u64.add(i).read();
        AtomicU64::from_ptr(dst_u64.add(i)).store(val, Ordering::Relaxed);
    }
}

/// Write to inactive buffer, then atomically flip active index.
///
/// Protocol: seq is incremented to ODD before copy (signals "writing in progress"),
/// then incremented to EVEN after flip (signals "consistent"). This lets readers
/// skip the expensive 64KB copy when a write is active.
///
/// # Safety
/// Caller must provide a valid writable shared-memory base pointer containing
/// a `DoubleBufferHeader` at offset 0 and two `TelemetryArchive` buffers.
#[inline]
pub unsafe fn write_double_buffer(base: *mut u8, archive: &TelemetryArchive) {
    let header = unsafe { &*(base as *const DoubleBufferHeader) };

    // Mark write as in-progress (odd seq = writer active)
    header.write_seq.fetch_add(1, Ordering::Release);

    let active = header.active_index.load(Ordering::Relaxed) & 1;
    let inactive = 1 - active;
    let offset = if inactive == 0 {
        BUFFER_0_OFFSET
    } else {
        BUFFER_1_OFFSET
    };
    let dst = unsafe { base.add(offset) };

    // Atomic copy: prevents UB if reader is slow; Relaxed is sufficient since
    // seq fences provide ordering and individual u64 ops are hardware-atomic.
    unsafe {
        atomic_write_shm(
            archive as *const TelemetryArchive as *const u8,
            dst,
            ARCHIVE_SIZE,
        );
    }

    fence(Ordering::Release);
    header.active_index.store(inactive, Ordering::Release);
    // Mark write complete (even seq = consistent)
    header.write_seq.fetch_add(1, Ordering::Release);
}

/// Read from active buffer. Returns `Err(())` if writer changed snapshots
/// during read after bounded retries. Caller maps `Err(())` to `AuraError::SeqLockInvalid`.
///
/// Protocol: If seq1 is odd, writer is active — skip copy and retry immediately.
/// If seq1 is even, copy the active buffer, then verify seq unchanged (seq2 == seq1).
/// This eliminates UB from non-atomic memcpy while preserving zero-copy semantics.
///
/// # Safety
/// Caller must provide a valid readable shared-memory base pointer containing
/// a `DoubleBufferHeader` at offset 0 and two initialized `TelemetryArchive`
/// buffers. The base pointer must be `*mut u8` (not `*const u8`) because
/// `AtomicU64::from_ptr` requires `*mut u64`.
#[inline]
#[allow(clippy::result_unit_err)]
pub unsafe fn read_double_buffer(base: *mut u8) -> Result<TelemetryArchive, ()> {
    let header = unsafe { &*(base as *const DoubleBufferHeader) };

    for _ in 0..3 {
        let seq1 = header.write_seq.load(Ordering::Acquire);

        // If seq is odd, writer is mid-write — skip expensive copy
        if seq1 & 1 != 0 {
            core::hint::spin_loop();
            continue;
        }

        let active = header.active_index.load(Ordering::Acquire) & 1;
        let offset = if active == 0 {
            BUFFER_0_OFFSET
        } else {
            BUFFER_1_OFFSET
        };

        let src = unsafe { base.add(offset) };
        let mut archive = MaybeUninit::<TelemetryArchive>::uninit();

        // Atomic copy: prevents UB from data race; Relaxed is sufficient since
        // seq fences provide ordering and individual u64 ops are hardware-atomic.
        unsafe {
            atomic_read_shm(src, archive.as_mut_ptr() as *mut u8, ARCHIVE_SIZE);
        }

        fence(Ordering::Acquire);
        let seq2 = header.write_seq.load(Ordering::Acquire);

        if seq1 == seq2 {
            return Ok(unsafe { archive.assume_init() });
        }
    }

    Err(())
}
