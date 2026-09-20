//! Per-buffer SeqLock publication over one coherent shared mapping.
//!
//! Supported x86_64/aarch64 Linux/macOS mappings are page-aligned, coherent
//! `MAP_SHARED` regions. Header words and archive lanes are naturally aligned
//! `AtomicU64` locations and are never concurrently accessed non-atomically.
//! Rust thread order is active Acquire, selected sequence Acquire, payload
//! Relaxed lanes, Acquire fence, selected sequence Acquire for readers; writers
//! publish odd Relaxed, Release fence, payload Relaxed, even Release, active
//! Release. A read deadline controls retry admission, not total copy duration.

#[cfg(not(all(
    any(target_os = "linux", target_os = "macos"),
    any(target_arch = "x86_64", target_arch = "aarch64")
)))]
compile_error!("AURA SeqLock supports x86_64/aarch64 Linux/macOS only");

use std::mem::{align_of, size_of, MaybeUninit};
use std::sync::atomic::{fence, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use crate::{
    AuraError, AuraResult, TelemetryArchive, BUFFER_0_OFFSET, BUFFER_1_OFFSET, HEADER_SIZE,
    SEQLOCK_RETRY_ADMISSION_MS,
};

const SEQUENCE_OFFSET: usize = size_of::<u64>();
const ARCHIVE_SIZE: usize = size_of::<TelemetryArchive>();

#[repr(C)]
pub struct DoubleBufferHeader {
    pub active_index: AtomicU64,
    pub seq: [AtomicU64; 2],
}

#[allow(clippy::assertions_on_constants)]
const _: () = assert!(size_of::<DoubleBufferHeader>() == HEADER_SIZE);
#[allow(clippy::assertions_on_constants)]
const _: () = assert!(align_of::<DoubleBufferHeader>() == align_of::<AtomicU64>());
#[allow(clippy::assertions_on_constants)]
const _: () = assert!(align_of::<TelemetryArchive>() == align_of::<AtomicU64>());
#[allow(clippy::assertions_on_constants)]
const _: () = assert!(ARCHIVE_SIZE % size_of::<u64>() == 0);
#[allow(clippy::assertions_on_constants)]
const _: () = assert!(BUFFER_0_OFFSET % align_of::<AtomicU64>() == 0);
#[allow(clippy::assertions_on_constants)]
const _: () = assert!(BUFFER_1_OFFSET % align_of::<AtomicU64>() == 0);

#[inline]
unsafe fn load_aligned_atomic_u64(src: *const u64) -> u64 {
    debug_assert_eq!(src.align_offset(align_of::<AtomicU64>()), 0);
    let atomic = src.cast::<AtomicU64>();
    // SAFETY: [Categories 2, 6, 10 — races, alignment, bounds] the caller
    // proves this initialized aligned lane is accessed concurrently only as an atomic.
    unsafe { (*atomic).load(Ordering::Relaxed) }
}

#[inline]
unsafe fn load_aligned_atomic_u64_acquire(src: *const u64) -> u64 {
    debug_assert_eq!(src.align_offset(align_of::<AtomicU64>()), 0);
    let atomic = src.cast::<AtomicU64>();
    // SAFETY: [Categories 2, 6, 10 — races, alignment, bounds] the caller
    // proves this initialized aligned lane is accessed concurrently only as an atomic.
    unsafe { (*atomic).load(Ordering::Acquire) }
}

#[inline]
unsafe fn store_aligned_atomic_u64(dst: *mut u64, value: u64) {
    debug_assert_eq!(dst.align_offset(align_of::<AtomicU64>()), 0);
    let atomic = dst.cast::<AtomicU64>();
    // SAFETY: [Categories 2, 6, 10 — races, alignment, bounds] the caller
    // proves this writable aligned lane is accessed concurrently only as an atomic.
    unsafe { (*atomic).store(value, Ordering::Relaxed) };
}

#[inline]
unsafe fn store_aligned_atomic_u64_release(dst: *mut u64, value: u64) {
    debug_assert_eq!(dst.align_offset(align_of::<AtomicU64>()), 0);
    let atomic = dst.cast::<AtomicU64>();
    // SAFETY: [Categories 2, 6, 10 — races, alignment, bounds] the caller
    // proves this writable aligned lane is accessed concurrently only as an atomic.
    unsafe { (*atomic).store(value, Ordering::Release) };
}

#[inline]
unsafe fn load_active_acquire(base: *const u8) -> u64 {
    // SAFETY: [Categories 2, 6, 10 — races, alignment, bounds] the public read
    // contract guarantees an aligned live header word at offset zero.
    unsafe { load_aligned_atomic_u64_acquire(base.cast::<u64>()) }
}

#[inline]
unsafe fn load_sequence_acquire(base: *const u8, index: u64) -> u64 {
    let offset = SEQUENCE_OFFSET + index as usize * size_of::<u64>();
    let ptr = base.wrapping_add(offset).cast::<u64>();
    // SAFETY: [Categories 2, 6, 10 — races, alignment, bounds] callers first
    // prove `index <= 1`; both sequence words are aligned and inside the header.
    unsafe { load_aligned_atomic_u64_acquire(ptr) }
}

#[inline]
unsafe fn store_sequence_relaxed(base: *mut u8, index: u64, value: u64) {
    let offset = SEQUENCE_OFFSET + index as usize * size_of::<u64>();
    let ptr = base.wrapping_add(offset).cast::<u64>();
    // SAFETY: [Categories 2, 6, 10 — races, alignment, bounds] callers prove
    // `index <= 1` and provide the writable mapping, so this header lane is valid.
    unsafe { store_aligned_atomic_u64(ptr, value) };
}

#[inline]
unsafe fn store_sequence_release(base: *mut u8, index: u64, value: u64) {
    let offset = SEQUENCE_OFFSET + index as usize * size_of::<u64>();
    let ptr = base.wrapping_add(offset).cast::<u64>();
    // SAFETY: [Categories 2, 6, 10 — races, alignment, bounds] callers prove
    // `index <= 1` and provide the writable mapping, so this header lane is valid.
    unsafe { store_aligned_atomic_u64_release(ptr, value) };
}

#[inline]
unsafe fn store_active_release(base: *mut u8, value: u64) {
    // SAFETY: [Categories 2, 6, 10 — races, alignment, bounds] the public write
    // contract guarantees an aligned writable active word at offset zero.
    unsafe { store_aligned_atomic_u64_release(base.cast::<u64>(), value) };
}

#[inline]
unsafe fn atomic_read_shm(src: *const u8, dst: *mut u8) {
    debug_assert_eq!(src.align_offset(align_of::<AtomicU64>()), 0);
    debug_assert_eq!(dst.align_offset(align_of::<AtomicU64>()), 0);
    let src_u64 = src.cast::<u64>();
    let dst_u64 = dst.cast::<u64>();
    for index in 0..ARCHIVE_SIZE / size_of::<u64>() {
        // SAFETY: [Categories 2, 6, 10 — races, alignment, bounds] `index` is
        // within the complete aligned archive and shared lanes are atomic-only.
        let value = unsafe { load_aligned_atomic_u64(src_u64.add(index)) };
        // SAFETY: [Categories 4, 6, 10 — initialization, alignment, bounds]
        // the destination lane is unique uninitialized output within the archive.
        unsafe { dst_u64.add(index).write(value) };
    }
}

#[inline]
unsafe fn atomic_write_shm(src: *const u8, dst: *mut u8) {
    debug_assert_eq!(src.align_offset(align_of::<AtomicU64>()), 0);
    debug_assert_eq!(dst.align_offset(align_of::<AtomicU64>()), 0);
    let src_u64 = src.cast::<u64>();
    let dst_u64 = dst.cast::<u64>();
    for index in 0..ARCHIVE_SIZE / size_of::<u64>() {
        // SAFETY: [Categories 6 and 10 — alignment and bounds] the immutable
        // source archive is initialized, aligned, and valid for every lane.
        let value = unsafe { src_u64.add(index).read() };
        // SAFETY: [Categories 2, 6, 10 — races, alignment, bounds] the target
        // shared lane is aligned, writable, and concurrently atomic-only.
        unsafe { store_aligned_atomic_u64(dst_u64.add(index), value) };
    }
}

#[inline]
fn backoff_after_failure(failures: u64) {
    if failures <= 64 {
        core::hint::spin_loop();
    } else {
        std::thread::yield_now();
    }
}

/// Read one stable published archive with a 10 ms retry-admission deadline.
///
/// # Safety
/// `base` must address a live, naturally aligned, readable `SHM_SIZE` coherent
/// shared mapping initialized as AURA's header plus two archive buffers. Every
/// protocol-visible shared word must be accessed only through atomic operations.
#[inline]
pub unsafe fn read_double_buffer(base: *const u8) -> AuraResult<TelemetryArchive> {
    let started = Instant::now();
    // SAFETY: [Categories 2, 6, 10 — races, alignment, bounds] this wrapper
    // forwards its complete mapping contract and supplies a monotonic elapsed clock.
    unsafe { read_double_buffer_with_elapsed(base, || started.elapsed()) }
}

/// Execute the production read protocol with an injected monotonic elapsed clock.
///
/// # Safety
/// The mapping contract is identical to [`read_double_buffer`]. `elapsed` must
/// never decrease. This hook exists for deterministic deadline-admission tests.
#[doc(hidden)]
pub unsafe fn read_double_buffer_with_elapsed<F>(
    base: *const u8,
    mut elapsed: F,
) -> AuraResult<TelemetryArchive>
where
    F: FnMut() -> Duration,
{
    let deadline = Duration::from_millis(SEQLOCK_RETRY_ADMISSION_MS);
    let mut failures = 0u64;
    let mut saw_nonzero = false;
    loop {
        if failures != 0 && elapsed() >= deadline {
            return if saw_nonzero {
                Err(AuraError::SeqLockTimeout)
            } else {
                Err(AuraError::NotPublished)
            };
        }
        // SAFETY: [Categories 2, 6, 10 — races, alignment, bounds] `base`
        // satisfies this function's mapping contract for the active header word.
        let active = unsafe { load_active_acquire(base) };
        if active > 1 {
            return Err(AuraError::InvalidShmHeader { found: active });
        }
        // SAFETY: [Categories 2, 6, 10 — races, alignment, bounds] `active` was
        // range-checked and selects one initialized sequence word.
        let seq1 = unsafe { load_sequence_acquire(base, active) };
        saw_nonzero |= seq1 != 0;
        if seq1 == 0 || seq1 & 1 != 0 {
            failures = failures.saturating_add(1);
            backoff_after_failure(failures);
            continue;
        }
        let offset = if active == 0 {
            BUFFER_0_OFFSET
        } else {
            BUFFER_1_OFFSET
        };
        let src = base.wrapping_add(offset);
        let mut archive = MaybeUninit::<TelemetryArchive>::uninit();
        // SAFETY: [Categories 2, 4, 6, 10 — races, initialization, alignment,
        // bounds] the selected buffer and complete destination satisfy lane contracts.
        unsafe { atomic_read_shm(src, archive.as_mut_ptr().cast::<u8>()) };
        fence(Ordering::Acquire);
        // SAFETY: [Categories 2, 6, 10 — races, alignment, bounds] `active`
        // still selects the same in-bounds sequence word for final validation.
        let seq2 = unsafe { load_sequence_acquire(base, active) };
        saw_nonzero |= seq2 != 0;
        if seq1 == seq2 && seq2 & 1 == 0 {
            // SAFETY: [Category 4 — uninitialized memory] `atomic_read_shm`
            // initialized every byte of the exact `TelemetryArchive` destination.
            return Ok(unsafe { archive.assume_init() });
        }
        failures = failures.saturating_add(1);
        backoff_after_failure(failures);
    }
}

/// Write the inactive archive, advance its generation, and publish it.
///
/// # Safety
/// `base` must address a live, naturally aligned, writable `SHM_SIZE` coherent
/// shared mapping initialized as AURA's header plus two archive buffers. There
/// must be exactly one writer, and all shared words use this atomic protocol.
#[inline]
pub unsafe fn write_double_buffer(base: *mut u8, archive: &TelemetryArchive) -> AuraResult<()> {
    // SAFETY: [Categories 2, 6, 10 — races, alignment, bounds] the mapping
    // contract guarantees an initialized aligned active header word.
    let active = unsafe { load_active_acquire(base) };
    if active > 1 {
        return Err(AuraError::InvalidShmHeader { found: active });
    }
    let inactive = 1 - active;
    // SAFETY: [Categories 2, 6, 10 — races, alignment, bounds] `inactive` is
    // exactly zero or one and selects its initialized sequence word.
    let current = unsafe { load_sequence_acquire(base, inactive) };
    let (odd, even) = next_sequence(current)?;
    // SAFETY: [Categories 2, 6, 10 — races, alignment, bounds] the selected
    // sequence word is writable and atomic-only under the single-writer contract.
    unsafe { store_sequence_relaxed(base, inactive, odd) };
    fence(Ordering::Release);
    let offset = if inactive == 0 {
        BUFFER_0_OFFSET
    } else {
        BUFFER_1_OFFSET
    };
    let src = std::ptr::addr_of!(*archive).cast::<u8>();
    let dst = base.wrapping_add(offset);
    // SAFETY: [Categories 2, 6, 10 — races, alignment, bounds] source and
    // destination are complete aligned archives and shared lanes are atomic-only.
    unsafe { atomic_write_shm(src, dst) };
    // SAFETY: [Categories 2, 6, 10 — races, alignment, bounds] the even release
    // publishes all payload lanes through the selected valid sequence word.
    unsafe { store_sequence_release(base, inactive, even) };
    // SAFETY: [Categories 2, 6, 10 — races, alignment, bounds] the active
    // release publishes the now-complete buffer index, which is zero or one.
    unsafe { store_active_release(base, inactive) };
    Ok(())
}

fn next_sequence(current: u64) -> AuraResult<(u64, u64)> {
    let (odd_increment, even_increment) = if current & 1 == 0 { (1, 2) } else { (2, 3) };
    let odd = current
        .checked_add(odd_increment)
        .ok_or(AuraError::SequenceExhausted { sequence: current })?;
    let even = current
        .checked_add(even_increment)
        .ok_or(AuraError::SequenceExhausted { sequence: current })?;
    if odd == 0 || even == 0 {
        return Err(AuraError::SequenceExhausted { sequence: current });
    }
    Ok((odd, even))
}
