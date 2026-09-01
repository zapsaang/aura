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
    debug_assert_eq!(src.align_offset(8), 0, "src must be 8-byte aligned");
    debug_assert_eq!(dst.align_offset(8), 0, "dst must be 8-byte aligned");
    debug_assert_eq!(len % 8, 0);
    let chunks = len / 8;
    let src_u64 = src as *mut u64;
    let dst_u64 = dst as *mut u64;
    for i in 0..chunks {
        // SAFETY: `src` is 8-byte aligned, valid for `len` bytes, and `i < chunks`, so this u64 lane is in-bounds for atomic loading.
        let val = unsafe { AtomicU64::from_ptr(src_u64.add(i)).load(Ordering::Relaxed) };
        // SAFETY: `dst` is 8-byte aligned, valid writable for `len` bytes, and `i < chunks`, so this u64 lane is in-bounds to initialize.
        unsafe { dst_u64.add(i).write(val) };
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
    debug_assert_eq!(src.align_offset(8), 0, "src must be 8-byte aligned");
    debug_assert_eq!(dst.align_offset(8), 0, "dst must be 8-byte aligned");
    debug_assert_eq!(len % 8, 0);
    let chunks = len / 8;
    let src_u64 = src as *const u64;
    let dst_u64 = dst as *mut u64;
    for i in 0..chunks {
        // SAFETY: `src` is 8-byte aligned, valid for `len` bytes, and `i < chunks`, so this u64 lane is in-bounds for reading.
        let val = unsafe { src_u64.add(i).read() };
        // SAFETY: `dst` is 8-byte aligned shared memory, valid for `len` bytes, and `i < chunks`, so this lane can be atomically stored.
        unsafe { AtomicU64::from_ptr(dst_u64.add(i)).store(val, Ordering::Relaxed) };
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
    // SAFETY: `base` points to a writable SHM mapping whose offset 0 contains an 8-byte aligned `DoubleBufferHeader`.
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
    // SAFETY: `offset` is one of the two fixed buffer offsets inside the SHM mapping, both within `SHM_SIZE` and 8-byte aligned.
    let dst = unsafe { base.add(offset) };

    // Atomic copy: prevents UB if reader is slow; Relaxed is sufficient since
    // seq fences provide ordering and individual u64 ops are hardware-atomic.
    // SAFETY: `archive` is initialized and 8-byte aligned; `dst` points to a full archive buffer and `ARCHIVE_SIZE` is a multiple of 8.
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
    // SAFETY: `base` points to a readable SHM mapping whose offset 0 contains an 8-byte aligned `DoubleBufferHeader`.
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

        // SAFETY: `offset` selects the active archive buffer inside the SHM mapping, within bounds and aligned for atomic u64 reads.
        let src = unsafe { base.add(offset) };
        let mut archive = MaybeUninit::<TelemetryArchive>::uninit();

        // Step 3: atomic copy from the active buffer
        // SAFETY: `src` points to a full archive buffer; `archive` is writable uninit storage and `ARCHIVE_SIZE` is a multiple of 8.
        unsafe {
            atomic_read_shm(src, archive.as_mut_ptr() as *mut u8, ARCHIVE_SIZE);
        }

        fence(Ordering::Acquire);
        // Step 4: verify writer didn't touch THIS buffer during our copy
        let seq2 = header.seq[active as usize].load(Ordering::Acquire);

        if seq1 == seq2 {
            // SAFETY: `atomic_read_shm` initialized exactly `ARCHIVE_SIZE` bytes, which is the complete `TelemetryArchive`.
            return Ok(unsafe { archive.assume_init() });
        }
    }

    Err(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{TelemetryArchive, SHM_SIZE};

    fn aligned_shm_buffer() -> (Vec<u64>, *mut u8) {
        let mut buf = vec![0u64; SHM_SIZE / 8];
        let ptr = buf.as_mut_ptr() as *mut u8;
        assert_eq!(ptr.align_offset(8), 0, "Vec<u64> must be 8-byte aligned");
        assert_eq!(buf.len() * 8, SHM_SIZE, "Buffer must cover full SHM size");
        (buf, ptr)
    }

    #[test]
    fn write_then_read_returns_same_data() {
        let (_buf, base) = aligned_shm_buffer();

        let mut archive = TelemetryArchive::zeroed();
        archive.version = 42;
        archive.cpu.user_ticks = 1234;
        archive.memory.ram_total = 16_777_216;

        // SAFETY: `base` comes from `aligned_shm_buffer`, a zeroed `Vec<u64>` covering `SHM_SIZE` with 8-byte alignment.
        unsafe {
            write_double_buffer(base, &archive);
        }

        // SAFETY: `base` still points to the same aligned, initialized test SHM buffer after the clean write.
        let result = unsafe { read_double_buffer(base) };
        let read_back = result.expect("read should succeed after a clean write");

        assert_eq!(read_back.version, 42);
        assert_eq!(read_back.cpu.user_ticks, 1234);
        assert_eq!(read_back.memory.ram_total, 16_777_216);
    }

    #[test]
    fn read_returns_err_when_seq_is_odd() {
        let (_buf, base) = aligned_shm_buffer();

        // SAFETY: `base` is an 8-byte aligned test SHM buffer whose first bytes are the zeroed header.
        let header = unsafe { &*(base as *const DoubleBufferHeader) };
        header.active_index.store(0, Ordering::Relaxed);
        header.seq[0].store(1, Ordering::Relaxed);

        // SAFETY: `base` is a valid aligned test SHM buffer; the intentionally odd seq exercises the error path.
        let result = unsafe { read_double_buffer(base) };
        assert!(result.is_err(), "read should fail when seq[active] is odd");
    }

    #[test]
    fn write_flips_active_index() {
        let (_buf, base) = aligned_shm_buffer();

        // SAFETY: `base` is an 8-byte aligned test SHM buffer whose first bytes are the zeroed header.
        let header = unsafe { &*(base as *const DoubleBufferHeader) };
        assert_eq!(
            header.active_index.load(Ordering::Relaxed),
            0,
            "initial active_index should be 0 (zeroed memory)"
        );

        let archive = TelemetryArchive::zeroed();
        // SAFETY: `base` is a valid aligned test SHM buffer covering the full double-buffer layout.
        unsafe {
            write_double_buffer(base, &archive);
        }

        assert_eq!(
            header.active_index.load(Ordering::Relaxed),
            1,
            "active_index should flip to 1 after first write"
        );
    }

    #[test]
    fn multiple_writes_increment_seq_by_2() {
        let (_buf, base) = aligned_shm_buffer();

        // SAFETY: `base` is an 8-byte aligned test SHM buffer whose first bytes are the zeroed header.
        let header = unsafe { &*(base as *const DoubleBufferHeader) };
        let archive = TelemetryArchive::zeroed();

        // SAFETY: `base` is a valid aligned test SHM buffer covering the full double-buffer layout.
        unsafe {
            write_double_buffer(base, &archive);
        }
        let seq_after_1 = header.seq[1].load(Ordering::Relaxed);
        assert_eq!(
            seq_after_1, 2,
            "seq[inactive] should be 2 after first write (0→1→2)"
        );

        // SAFETY: `base` remains valid for the second write into the alternate archive buffer.
        unsafe {
            write_double_buffer(base, &archive);
        }
        let seq_after_2 = header.seq[0].load(Ordering::Relaxed);
        assert_eq!(
            seq_after_2, 2,
            "seq[inactive=0] should be 2 after second write (0→1→2)"
        );

        // SAFETY: `base` remains valid for the third write into the original archive buffer.
        unsafe {
            write_double_buffer(base, &archive);
        }
        let seq_after_3 = header.seq[1].load(Ordering::Relaxed);
        assert_eq!(
            seq_after_3, 4,
            "seq[inactive=1] should be 4 after third write (2→3→4)"
        );
    }
}
