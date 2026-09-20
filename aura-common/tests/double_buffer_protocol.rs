use std::cell::Cell;
use std::mem::{align_of, size_of};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use aura_common::{
    read_double_buffer, read_double_buffer_with_elapsed, write_double_buffer, AuraError,
    DoubleBufferHeader, TelemetryArchive, BUFFER_0_OFFSET, BUFFER_1_OFFSET, HEADER_SIZE,
    SEQLOCK_RETRY_ADMISSION_MS, SHM_SIZE,
};

const ACTIVE_OFFSET: usize = 0;
const SEQ_0_OFFSET: usize = 8;
const SEQ_1_OFFSET: usize = 16;

fn aligned_shm() -> (Vec<u64>, *mut u8) {
    let mut storage = vec![0u64; SHM_SIZE / size_of::<u64>()];
    let base = storage.as_mut_ptr().cast::<u8>();
    assert_eq!(base.align_offset(align_of::<u64>()), 0);
    (storage, base)
}

fn archive(marker: u64) -> TelemetryArchive {
    let mut archive = TelemetryArchive::zeroed();
    archive.version = marker;
    archive.meta.timestamp_ns = marker;
    archive
}

fn load_word(base: *const u8, offset: usize, ordering: Ordering) -> u64 {
    // SAFETY: [Categories 2, 6, 10 — races, alignment, bounds] callers use
    // header offsets 0, 8, or 16 in an aligned live `SHM_SIZE` allocation.
    unsafe { (*base.add(offset).cast::<AtomicU64>()).load(ordering) }
}

fn store_word(base: *mut u8, offset: usize, value: u64, ordering: Ordering) {
    // SAFETY: [Categories 2, 6, 10 — races, alignment, bounds] callers use
    // header offsets 0, 8, or 16 in an aligned writable `SHM_SIZE` allocation.
    unsafe { (*base.add(offset).cast::<AtomicU64>()).store(value, ordering) };
}

fn store_archive(base: *mut u8, offset: usize, archive: &TelemetryArchive) {
    let lanes = bytemuck::cast_slice::<u8, u64>(bytemuck::bytes_of(archive));
    for (index, lane) in lanes.iter().copied().enumerate() {
        // SAFETY: [Categories 2, 6, 10 — races, alignment, bounds] both archive
        // offsets and every u64 lane are aligned and within `SHM_SIZE`.
        unsafe {
            (*base.add(offset).cast::<AtomicU64>().add(index)).store(lane, Ordering::Relaxed)
        };
    }
}

fn source_function<'a>(source: &'a str, start: &str, end: &str) -> &'a str {
    let start = source.find(start).expect("function start must exist");
    let tail = &source[start..];
    let end = tail.find(end).expect("function end must exist");
    &tail[..end]
}

#[test]
fn stable_snapshot_roundtrips_every_byte() {
    // Given
    let (_storage, base) = aligned_shm();
    let expected = archive(41);

    // When
    // SAFETY: [Categories 2, 6, 10 — races, alignment, bounds] `aligned_shm`
    // owns an aligned, initialized `SHM_SIZE` region for both protocol calls.
    unsafe { write_double_buffer(base, &expected) }.expect("write must publish");
    // SAFETY: [Categories 2, 6, 10 — races, alignment, bounds] the preceding
    // write published one complete archive in the same live region.
    let actual = unsafe { read_double_buffer(base) }.expect("stable snapshot must read");

    // Then
    assert_eq!(bytemuck::bytes_of(&actual), bytemuck::bytes_of(&expected));
}

#[test]
fn first_publication_uses_buffer_one_and_sequence_two() {
    // Given
    let (_storage, base) = aligned_shm();

    // When
    // SAFETY: [Categories 2, 6, 10 — races, alignment, bounds] `base` covers
    // one exclusively owned, aligned shared-memory layout.
    unsafe { write_double_buffer(base, &archive(1)) }.expect("write must publish");

    // Then
    // SAFETY: [Categories 5, 6, 10 — validity, alignment, bounds] the zeroed
    // header has been accessed only atomically and fits at offset zero.
    let header = unsafe { &*base.cast::<DoubleBufferHeader>() };
    assert_eq!(header.active_index.load(Ordering::Acquire), 1);
    assert_eq!(header.seq[0].load(Ordering::Acquire), 0);
    assert_eq!(header.seq[1].load(Ordering::Acquire), 2);
}

#[test]
fn odd_sequence_on_inactive_buffer_does_not_block_reader() {
    // Given
    let (_storage, base) = aligned_shm();
    // SAFETY: [Categories 2, 6, 10 — races, alignment, bounds] `base` is an
    // exclusively owned aligned layout for this test.
    unsafe { write_double_buffer(base, &archive(9)) }.expect("write must publish");
    // SAFETY: [Categories 5, 6, 10 — validity, alignment, bounds] the header
    // occupies the first 24 initialized bytes of the aligned allocation.
    let header = unsafe { &*base.cast::<DoubleBufferHeader>() };
    header.seq[0].store(1, Ordering::Release);

    // When
    // SAFETY: [Categories 2, 6, 10 — races, alignment, bounds] all concurrent
    // protocol-visible locations are accessed atomically within the live map.
    let actual = unsafe { read_double_buffer(base) }.expect("inactive odd seq must not block");

    // Then
    assert_eq!(actual.version, 9);
}

#[test]
fn repeated_publications_alternate_and_advance_by_two() {
    // Given
    let (_storage, base) = aligned_shm();

    // When
    // SAFETY: [Categories 2, 6, 10 — races, alignment, bounds] every write uses
    // the same exclusively owned aligned region and initialized archive.
    unsafe {
        write_double_buffer(base, &archive(1)).expect("first write");
        write_double_buffer(base, &archive(2)).expect("second write");
        write_double_buffer(base, &archive(3)).expect("third write");
    }

    // Then
    // SAFETY: [Categories 5, 6, 10 — validity, alignment, bounds] the header
    // remains initialized and naturally aligned at offset zero.
    let header = unsafe { &*base.cast::<DoubleBufferHeader>() };
    assert_eq!(header.active_index.load(Ordering::Acquire), 1);
    assert_eq!(header.seq[0].load(Ordering::Acquire), 2);
    assert_eq!(header.seq[1].load(Ordering::Acquire), 4);
}

#[test]
fn mapped_layout_meets_atomic_header_and_lane_assumptions() {
    // Given
    let (_storage, base) = aligned_shm();

    // When
    let header_alignment = base.align_offset(align_of::<AtomicU64>());

    // Then
    assert_eq!(size_of::<DoubleBufferHeader>(), HEADER_SIZE);
    assert_eq!(align_of::<DoubleBufferHeader>(), align_of::<AtomicU64>());
    assert_eq!(align_of::<TelemetryArchive>(), align_of::<AtomicU64>());
    assert_eq!(header_alignment, 0);
    assert_eq!(BUFFER_0_OFFSET % align_of::<AtomicU64>(), 0);
    assert_eq!(BUFFER_1_OFFSET % align_of::<AtomicU64>(), 0);
}

#[test]
fn read_only_mapping_supports_atomic_header_and_payload_loads() {
    // Given
    let mut writable = memmap2::MmapOptions::new()
        .len(SHM_SIZE)
        .map_anon()
        .expect("anonymous writable map");
    let expected = archive(51);
    // SAFETY: [Categories 2, 6, 10 — races, alignment, bounds] the anonymous
    // mapping is page-aligned, writable, and exactly `SHM_SIZE` bytes.
    unsafe { write_double_buffer(writable.as_mut_ptr(), &expected) }.expect("seed mapping");
    let read_only = writable.make_read_only().expect("make mapping read-only");

    // When
    // SAFETY: [Categories 2, 6, 10 — races, alignment, bounds] the read-only
    // mapping remains page-aligned and live for all atomic loads.
    let actual = unsafe { read_double_buffer(read_only.as_ptr()) }.expect("read-only load");

    // Then
    assert_eq!(actual.version, expected.version);
}

#[test]
fn fresh_zero_sequence_reaches_not_published_deadline() {
    // Given
    let (_storage, base) = aligned_shm();

    // When
    // SAFETY: [Categories 2, 6, 10 — races, alignment, bounds] `base` is a
    // live aligned zeroed layout and the injected elapsed time is monotonic.
    let error = unsafe {
        read_double_buffer_with_elapsed(base, || Duration::from_millis(SEQLOCK_RETRY_ADMISSION_MS))
    }
    .expect_err("zero sequence must not be accepted");

    // Then
    assert!(matches!(error, AuraError::NotPublished));
}

#[test]
fn nonzero_odd_sequence_reaches_retry_timeout() {
    // Given
    let (_storage, base) = aligned_shm();
    store_word(base, SEQ_0_OFFSET, 1, Ordering::Release);

    // When
    // SAFETY: [Categories 2, 6, 10 — races, alignment, bounds] `base` is a
    // live aligned layout and the injected elapsed time is monotonic.
    let error = unsafe {
        read_double_buffer_with_elapsed(base, || Duration::from_millis(SEQLOCK_RETRY_ADMISSION_MS))
    }
    .expect_err("odd sequence must time out");

    // Then
    assert!(matches!(error, AuraError::SeqLockTimeout));
}

#[test]
fn retry_at_exact_deadline_is_not_admitted() {
    // Given
    let (_storage, base) = aligned_shm();
    store_archive(base, BUFFER_0_OFFSET, &archive(61));
    let clock_calls = Cell::new(0u32);

    // When
    // SAFETY: [Categories 2, 6, 10 — races, alignment, bounds] the clock hook
    // mutates only the selected atomic sequence in the live aligned layout.
    let error = unsafe {
        read_double_buffer_with_elapsed(base, || {
            clock_calls.set(clock_calls.get() + 1);
            store_word(base, SEQ_0_OFFSET, 2, Ordering::Release);
            Duration::from_millis(SEQLOCK_RETRY_ADMISSION_MS)
        })
    }
    .expect_err("deadline equality must reject a new copy");

    // Then
    assert!(matches!(error, AuraError::NotPublished));
    assert_eq!(clock_calls.get(), 1);
}

#[test]
fn retry_admitted_before_deadline_may_complete() {
    // Given
    let (_storage, base) = aligned_shm();
    store_archive(base, BUFFER_0_OFFSET, &archive(62));
    let clock_calls = Cell::new(0u32);

    // When
    // SAFETY: [Categories 2, 6, 10 — races, alignment, bounds] the clock hook
    // publishes only the selected sequence before admitting the retry.
    let actual = unsafe {
        read_double_buffer_with_elapsed(base, || {
            clock_calls.set(clock_calls.get() + 1);
            store_word(base, SEQ_0_OFFSET, 2, Ordering::Release);
            Duration::from_millis(SEQLOCK_RETRY_ADMISSION_MS - 1)
        })
    }
    .expect("admitted copy may finish");

    // Then
    assert_eq!(actual.version, 62);
    assert_eq!(clock_calls.get(), 1);
}

#[test]
fn corrupt_active_index_rejects_without_mutation() {
    // Given
    let (storage, base) = aligned_shm();
    store_word(base, ACTIVE_OFFSET, 2, Ordering::Release);
    let before = storage.clone();

    // When
    // SAFETY: [Categories 2, 6, 10 — races, alignment, bounds] `base` is an
    // aligned writable layout containing the intentionally corrupt header.
    let error = unsafe { write_double_buffer(base, &archive(70)) }
        .expect_err("corrupt active index must reject");

    // Then
    assert!(matches!(error, AuraError::InvalidShmHeader { found: 2 }));
    assert_eq!(storage, before);
}

#[test]
fn exhausted_even_sequence_rejects_without_mutation() {
    // Given
    let (storage, base) = aligned_shm();
    store_word(base, SEQ_1_OFFSET, u64::MAX - 1, Ordering::Release);
    let before = storage.clone();

    // When
    // SAFETY: [Categories 2, 6, 10 — races, alignment, bounds] `base` is an
    // aligned writable layout containing the boundary sequence.
    let error = unsafe { write_double_buffer(base, &archive(71)) }
        .expect_err("even sequence overflow must reject");

    // Then
    assert!(matches!(
        error,
        AuraError::SequenceExhausted { sequence } if sequence == u64::MAX - 1
    ));
    assert_eq!(storage, before);
}

#[test]
fn exhausted_odd_sequence_rejects_without_mutation() {
    // Given
    let (storage, base) = aligned_shm();
    store_word(base, SEQ_1_OFFSET, u64::MAX, Ordering::Release);
    let before = storage.clone();

    // When
    // SAFETY: [Categories 2, 6, 10 — races, alignment, bounds] `base` is an
    // aligned writable layout containing the boundary sequence.
    let error = unsafe { write_double_buffer(base, &archive(72)) }
        .expect_err("odd sequence overflow must reject");

    // Then
    assert!(matches!(
        error,
        AuraError::SequenceExhausted { sequence: u64::MAX }
    ));
    assert_eq!(storage, before);
}

#[test]
fn abandoned_odd_sequence_recovers_to_new_generation() {
    // Given
    let (_storage, base) = aligned_shm();
    store_word(base, SEQ_1_OFFSET, 1, Ordering::Release);

    // When
    // SAFETY: [Categories 2, 6, 10 — races, alignment, bounds] the abandoned
    // sequence and target archive both reside in the live aligned layout.
    unsafe { write_double_buffer(base, &archive(80)) }.expect("recover odd generation");

    // Then
    assert_eq!(load_word(base, SEQ_1_OFFSET, Ordering::Acquire), 4);
    assert_eq!(load_word(base, ACTIVE_OFFSET, Ordering::Acquire), 1);
}

#[test]
fn existing_even_sequence_advances_without_reset() {
    // Given
    let (_storage, base) = aligned_shm();
    store_word(base, SEQ_1_OFFSET, 40, Ordering::Release);

    // When
    // SAFETY: [Categories 2, 6, 10 — races, alignment, bounds] the existing
    // generation and target archive both reside in the live aligned layout.
    unsafe { write_double_buffer(base, &archive(81)) }.expect("advance even generation");

    // Then
    assert_eq!(load_word(base, SEQ_1_OFFSET, Ordering::Acquire), 42);
}

#[test]
fn repeated_restart_style_writes_never_reuse_a_generation() {
    // Given
    let (_storage, base) = aligned_shm();
    store_word(base, SEQ_1_OFFSET, 17, Ordering::Release);

    // When
    // SAFETY: [Categories 2, 6, 10 — races, alignment, bounds] all writes use
    // one live aligned layout and fully initialized archives.
    unsafe {
        write_double_buffer(base, &archive(91)).expect("recover buffer one");
        write_double_buffer(base, &archive(92)).expect("publish buffer zero");
        write_double_buffer(base, &archive(93)).expect("reuse buffer one");
    }

    // Then
    assert_eq!(load_word(base, SEQ_1_OFFSET, Ordering::Acquire), 22);
    assert_eq!(load_word(base, SEQ_0_OFFSET, Ordering::Acquire), 2);
}

#[test]
fn crash_before_odd_store_preserves_old_snapshot() {
    // Given
    let (_storage, base) = aligned_shm();
    // SAFETY: [Categories 2, 6, 10 — races, alignment, bounds] `base` is one
    // live aligned layout and `archive(100)` is initialized.
    unsafe { write_double_buffer(base, &archive(100)) }.expect("seed old snapshot");

    // When
    // SAFETY: [Categories 2, 6, 10 — races, alignment, bounds] no write began,
    // so the selected published buffer remains stable.
    let actual = unsafe { read_double_buffer(base) }.expect("old snapshot remains readable");

    // Then
    assert_eq!(actual.version, 100);
}

#[test]
fn crash_after_odd_store_preserves_old_snapshot() {
    // Given
    let (_storage, base) = aligned_shm();
    // SAFETY: [Categories 2, 6, 10 — races, alignment, bounds] `base` is one
    // live aligned layout and the seed archive is initialized.
    unsafe { write_double_buffer(base, &archive(101)) }.expect("seed old snapshot");
    store_word(base, SEQ_0_OFFSET, 1, Ordering::Relaxed);

    // When
    // SAFETY: [Categories 2, 6, 10 — races, alignment, bounds] only the
    // inactive buffer sequence is odd; the selected buffer is unchanged.
    let actual = unsafe { read_double_buffer(base) }.expect("old snapshot remains readable");

    // Then
    assert_eq!(actual.version, 101);
}

#[test]
fn crash_during_payload_copy_preserves_old_snapshot() {
    // Given
    let (_storage, base) = aligned_shm();
    // SAFETY: [Categories 2, 6, 10 — races, alignment, bounds] `base` is one
    // live aligned layout and the seed archive is initialized.
    unsafe { write_double_buffer(base, &archive(102)) }.expect("seed old snapshot");
    store_word(base, SEQ_0_OFFSET, 1, Ordering::Relaxed);
    store_archive(base, BUFFER_0_OFFSET, &archive(202));

    // When
    // SAFETY: [Categories 2, 6, 10 — races, alignment, bounds] the partially
    // published payload is inactive and the selected old buffer is stable.
    let actual = unsafe { read_double_buffer(base) }.expect("old snapshot remains readable");

    // Then
    assert_eq!(actual.version, 102);
}

#[test]
fn crash_after_even_store_before_active_preserves_old_snapshot() {
    // Given
    let (_storage, base) = aligned_shm();
    // SAFETY: [Categories 2, 6, 10 — races, alignment, bounds] `base` is one
    // live aligned layout and the seed archive is initialized.
    unsafe { write_double_buffer(base, &archive(103)) }.expect("seed old snapshot");
    store_word(base, SEQ_0_OFFSET, 1, Ordering::Relaxed);
    store_archive(base, BUFFER_0_OFFSET, &archive(203));
    store_word(base, SEQ_0_OFFSET, 2, Ordering::Release);

    // When
    // SAFETY: [Categories 2, 6, 10 — races, alignment, bounds] active still
    // selects the old complete buffer even though the new buffer is complete.
    let actual = unsafe { read_double_buffer(base) }.expect("old snapshot remains selected");

    // Then
    assert_eq!(actual.version, 103);
}

#[test]
fn active_release_after_complete_payload_publishes_new_snapshot() {
    // Given
    let (_storage, base) = aligned_shm();
    // SAFETY: [Categories 2, 6, 10 — races, alignment, bounds] `base` is one
    // live aligned layout and the seed archive is initialized.
    unsafe { write_double_buffer(base, &archive(104)) }.expect("seed old snapshot");
    store_word(base, SEQ_0_OFFSET, 1, Ordering::Relaxed);
    store_archive(base, BUFFER_0_OFFSET, &archive(204));
    store_word(base, SEQ_0_OFFSET, 2, Ordering::Release);
    store_word(base, ACTIVE_OFFSET, 0, Ordering::Release);

    // When
    // SAFETY: [Categories 2, 6, 10 — races, alignment, bounds] active selects
    // the complete even-sequence buffer in the live aligned layout.
    let actual = unsafe { read_double_buffer(base) }.expect("new snapshot must read");

    // Then
    assert_eq!(actual.version, 204);
}

#[test]
fn reader_source_uses_exact_atomic_order_and_no_header_reference() {
    // Given
    let source = include_str!("../src/double_buffer.rs");
    let body = source_function(
        source,
        "pub unsafe fn read_double_buffer_with_elapsed",
        "pub unsafe fn write_double_buffer",
    );

    // When
    let active = body.find("load_active_acquire(base)").expect("active load");
    let seq1 = body
        .find("load_sequence_acquire(base, active)")
        .expect("seq1 load");
    let payload = body.find("atomic_read_shm").expect("payload loads");
    let fence = body
        .find("fence(Ordering::Acquire)")
        .expect("acquire fence");
    let seq2 = body[seq1 + 1..]
        .find("load_sequence_acquire(base, active)")
        .map(|offset| seq1 + 1 + offset)
        .expect("seq2 load");

    // Then
    assert!(active < seq1 && seq1 < payload && payload < fence && fence < seq2);
    assert!(!body.contains("DoubleBufferHeader"));
    assert!(!source.contains("AtomicU64::from_ptr"));
}

#[test]
fn writer_source_uses_exact_publish_order() {
    // Given
    let source = include_str!("../src/double_buffer.rs");
    let body = source_function(
        source,
        "pub unsafe fn write_double_buffer",
        "fn next_sequence",
    );

    // When
    let odd = body
        .find("store_sequence_relaxed(base, inactive, odd)")
        .expect("odd relaxed store");
    let fence = body
        .find("fence(Ordering::Release)")
        .expect("release fence");
    let payload = body.find("atomic_write_shm").expect("payload stores");
    let even = body
        .find("store_sequence_release(base, inactive, even)")
        .expect("even release store");
    let active = body
        .find("store_active_release(base, inactive)")
        .expect("active release store");

    // Then
    assert!(odd < fence && fence < payload && payload < even && even < active);
    assert!(body.contains("load_active_acquire(base)"));
    assert!(body.contains("load_sequence_acquire(base, inactive)"));
}

#[test]
fn retry_backoff_source_spins_sixty_four_then_yields() {
    // Given
    let source = include_str!("../src/double_buffer.rs");
    let body = source_function(source, "fn backoff_after_failure", "/// Read one stable");

    // When
    let boundary = body.find("failures <= 64").expect("spin boundary");
    let spin = body.find("spin_loop()").expect("spin operation");
    let yield_now = body.find("yield_now()").expect("yield operation");

    // Then
    assert!(boundary < spin && spin < yield_now);
}
