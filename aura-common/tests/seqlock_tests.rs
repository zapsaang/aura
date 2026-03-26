use std::mem::MaybeUninit;
use std::sync::atomic::{AtomicUsize, Ordering};

use aura_common::{read_seqlock, write_seqlock, TelemetryArchive};

#[test]
fn seqlock_handles_large_archive() {
    let mut version = AtomicUsize::new(0);

    let mut archive = TelemetryArchive::zeroed();
    archive.version = 42;
    archive.cpu.user_ticks = 12_345;
    archive.process.total = 77;
    archive.memory.ram_total = 64 * 1024 * 1024;
    archive.meta.uptime_secs = 9_999;

    let mut slot = MaybeUninit::<TelemetryArchive>::zeroed();

    unsafe {
        write_seqlock(&mut version, slot.as_mut_ptr(), &archive)
            .expect("write_seqlock should succeed");
    }

    let restored: TelemetryArchive = unsafe {
        read_seqlock(&version, slot.as_ptr())
            .expect("read_seqlock should succeed for large archive")
    };

    assert_eq!(version.load(Ordering::SeqCst), 2);
    assert_eq!(restored.version, archive.version);
    assert_eq!(restored.cpu.user_ticks, archive.cpu.user_ticks);
    assert_eq!(restored.process.total, archive.process.total);
    assert_eq!(restored.memory.ram_total, archive.memory.ram_total);
    assert_eq!(restored.meta.uptime_secs, archive.meta.uptime_secs);
}
