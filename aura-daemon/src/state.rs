use std::fs::OpenOptions;
use std::path::Path;
use std::sync::atomic::{compiler_fence, AtomicUsize, Ordering};

use memmap2::{MmapMut, MmapOptions};

use aura_common::{AuraResult, TelemetryArchive, DATA_OFFSET, SHM_SIZE};

pub struct ShmHandle {
    mmap: MmapMut,
}

impl ShmHandle {
    pub fn new(path: &Path) -> AuraResult<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(true)
            .open(path)?;

        file.set_len(SHM_SIZE as u64)?;

        let mmap = unsafe { MmapOptions::new().len(SHM_SIZE).map_mut(&file)? };

        Ok(Self { mmap })
    }

    pub fn write(&mut self, telemetry: &TelemetryArchive) -> AuraResult<()> {
        let base = self.mmap.as_mut_ptr();
        let version_ptr = base as *mut AtomicUsize;
        unsafe { (*version_ptr).fetch_add(1, Ordering::SeqCst) };
        compiler_fence(Ordering::SeqCst);

        let dst = unsafe { base.add(DATA_OFFSET) } as *mut u8;
        let src = telemetry as *const TelemetryArchive as *const u8;
        let len = std::mem::size_of::<TelemetryArchive>();
        unsafe {
            std::ptr::copy_nonoverlapping(src, dst, len);
        }

        compiler_fence(Ordering::SeqCst);
        unsafe { (*version_ptr).fetch_add(1, Ordering::SeqCst) };
        Ok(())
    }
}
