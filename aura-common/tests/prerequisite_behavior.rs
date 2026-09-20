use std::mem::{align_of, size_of, MaybeUninit};
use std::sync::atomic::Ordering;

use aura_common::{
    read_double_buffer, write_double_buffer, DoubleBufferHeader, TelemetryArchive, BUFFER_0_OFFSET,
    SHM_SIZE,
};

fn checksum_offset() -> usize {
    let uninit = MaybeUninit::<TelemetryArchive>::uninit();
    let base = uninit.as_ptr();
    // SAFETY: `addr_of!` forms a raw pointer without reading the uninitialized
    // archive; both pointers belong to the same `TelemetryArchive` allocation.
    let field = unsafe { std::ptr::addr_of!((*base).checksum) };
    field as usize - base as usize
}

fn aligned_shm() -> (Vec<u64>, *mut u8) {
    let mut storage = vec![0u64; SHM_SIZE / size_of::<u64>()];
    let base = storage.as_mut_ptr().cast::<u8>();
    assert_eq!(base.align_offset(align_of::<u64>()), 0);
    (storage, base)
}

#[test]
fn archive_layout_matches_current_shared_memory_contract() {
    assert_eq!(size_of::<TelemetryArchive>(), 65_536);
    assert_eq!(align_of::<TelemetryArchive>(), 8);
    assert_eq!(checksum_offset(), 18_968);
    assert_eq!(size_of::<TelemetryArchive>() % 8, 0);
}

#[test]
fn zeroed_archive_checksum_matches_current_crc_contract() {
    let mut archive = TelemetryArchive::zeroed();
    archive.version = aura_common::ARCHIVE_VERSION;
    assert_eq!(archive.calculate_checksum(), 0xe11d_ad37);
}

#[test]
fn atomic_copy_roundtrip_preserves_every_archive_byte() {
    let (_storage, base) = aligned_shm();
    let mut expected = TelemetryArchive::zeroed();
    for (index, byte) in bytemuck::bytes_of_mut(&mut expected).iter_mut().enumerate() {
        *byte = index.wrapping_mul(37).wrapping_add(11) as u8;
    }

    // SAFETY: `base` is an aligned zeroed allocation covering the full SHM
    // layout and `expected` is a fully initialized archive.
    unsafe { write_double_buffer(base, &expected) }.expect("publish archive");
    // SAFETY: the preceding write initialized the published archive buffer in
    // the same aligned full-size SHM allocation.
    let actual = unsafe { read_double_buffer(base) }.expect("published archive should be readable");

    assert_eq!(bytemuck::bytes_of(&actual), bytemuck::bytes_of(&expected));
}

#[test]
fn writer_publishes_expected_buffer_and_sequence() {
    let (_storage, base) = aligned_shm();
    let archive = TelemetryArchive::zeroed();

    // SAFETY: `base` is an aligned zeroed allocation covering the full SHM
    // layout and `archive` is fully initialized.
    unsafe { write_double_buffer(base, &archive) }.expect("publish archive");
    // SAFETY: offset zero of the aligned allocation contains the initialized
    // double-buffer header written atomically above.
    let header = unsafe { &*base.cast::<DoubleBufferHeader>() };

    assert_eq!(header.active_index.load(Ordering::Acquire), 1);
    assert_eq!(header.seq[0].load(Ordering::Acquire), 0);
    assert_eq!(header.seq[1].load(Ordering::Acquire), 2);
}

#[test]
fn writer_places_archive_at_the_published_offset() {
    let (storage, base) = aligned_shm();
    let mut archive = TelemetryArchive::zeroed();
    archive.version = 0x1122_3344_5566_7788;

    // SAFETY: `base` is an aligned zeroed allocation covering the full SHM
    // layout and `archive` is fully initialized.
    unsafe { write_double_buffer(base, &archive) }.expect("publish archive");

    let bytes = bytemuck::cast_slice::<u64, u8>(&storage);
    assert_eq!(
        &bytes[BUFFER_0_OFFSET + 65_536..BUFFER_0_OFFSET + 65_544],
        &archive.version.to_ne_bytes()
    );
}
