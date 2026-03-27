use std::mem::MaybeUninit;
use std::sync::atomic::{fence, AtomicU64, Ordering};

use crate::{TelemetryArchive, BUFFER_0_OFFSET, BUFFER_1_OFFSET};

#[repr(C)]
pub struct DoubleBufferHeader {
    pub active_index: AtomicU64,
    /// Per-buffer sequence numbers. seq[0] protects BUFFER_0, seq[1] protects BUFFER_1.
    /// Odd = writer actively writing to that buffer, Even = buffer is consistent.
    pub seq: [AtomicU64; 2],
}

const ARCHIVE_SIZE: usize = std::mem::size_of::<TelemetryArchive>();
const _: () = assert!(
    ARCHIVE_SIZE.is_multiple_of(8),
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
/// Protocol: seq[inactive] is incremented to ODD before copy (signals "writing to THIS buffer"),
/// then incremented to EVEN after copy (signals "THIS buffer is consistent").
/// Finally, active_index is flipped to publish the new buffer.
///
/// # Safety
/// Caller must provide a valid writable shared-memory base pointer containing
/// a `DoubleBufferHeader` at offset 0 and two `TelemetryArchive` buffers.
#[inline]
pub unsafe fn write_double_buffer(base: *mut u8, archive: &TelemetryArchive) {
    let header = unsafe { &*(base as *const DoubleBufferHeader) };

    let active = header.active_index.load(Ordering::Relaxed) & 1;
    let inactive = 1 - active;

    // Mark THIS buffer as being written to (odd = writer active on inactive buffer)
    header.seq[inactive as usize].fetch_add(1, Ordering::Release);

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
    // Mark THIS buffer as consistent (even = ready to read)
    header.seq[inactive as usize].fetch_add(1, Ordering::Release);
    // Publish the newly written buffer
    header.active_index.store(inactive, Ordering::Release);
}

/// Read from active buffer. Returns `Err(())` if writer changed the SAME buffer
/// during read after bounded retries. Caller maps `Err(())` to `AuraError::SeqLockInvalid`.
///
/// Protocol:
/// 1. Load active (which buffer to read)
/// 2. Load seq[active] — if odd, writer is writing to THIS buffer, retry
/// 3. Copy from active buffer
/// 4. Load seq[active] again — if different, writer modified THIS buffer during copy, retry
/// 5. If seq1 == seq2, success
///
/// Key difference from old design: we check seq[active] (per-buffer), not write_seq (global).
/// This eliminates false contention where reader was blocked by writer writing to OTHER buffer.
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
        // Step 1: determine which buffer is active
        let active = header.active_index.load(Ordering::Acquire) & 1;

        // Step 2: check if writer is mid-write to THIS specific buffer
        let seq1 = header.seq[active as usize].load(Ordering::Acquire);
        if seq1 & 1 != 0 {
            core::hint::spin_loop();
            continue;
        }

        let offset = if active == 0 {
            BUFFER_0_OFFSET
        } else {
            BUFFER_1_OFFSET
        };

        let src = unsafe { base.add(offset) };
        let mut archive = MaybeUninit::<TelemetryArchive>::uninit();

        // Step 3: atomic copy from the active buffer
        unsafe {
            atomic_read_shm(src, archive.as_mut_ptr() as *mut u8, ARCHIVE_SIZE);
        }

        fence(Ordering::Acquire);
        // Step 4: verify writer didn't touch THIS buffer during our copy
        let seq2 = header.seq[active as usize].load(Ordering::Acquire);

        if seq1 == seq2 {
            return Ok(unsafe { archive.assume_init() });
        }
    }

    Err(())
}
