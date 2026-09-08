//! Hardened shared-memory state handle.
//!
//! The lock leaf is created/opened and validated relative to the retained
//! runtime directory fd, and `flock(LOCK_EX|LOCK_NB)` is acquired before
//! any state/temp operation, so a second daemon exits without mutating
//! any runtime leaf. State creation goes through an invocation-owned
//! temporary file that is atomically published and always unlinked by its
//! owner on failure; existing leaves with wrong owner/type/mode/size are
//! rejected without mutation.

pub mod lock;
pub mod publish;

use std::path::Path;

use memmap2::{MmapMut, MmapOptions};

use aura_common::runtime::RuntimeLocation;
use aura_common::{write_double_buffer, AuraResult, TelemetryArchive, SHM_SIZE};

#[derive(Debug)]
pub struct ShmHandle {
    mmap: MmapMut,
    _lock: lock::StateLock,
    _location: RuntimeLocation,
}

impl ShmHandle {
    /// Open or create state at an operator-supplied absolute path whose
    /// parent already exists as an euid-owned 0700 directory.
    pub fn new(path: &Path) -> AuraResult<Self> {
        Self::open(RuntimeLocation::resolve_override(path.as_os_str())?)
    }

    /// Open or create state at the private per-user platform default.
    pub fn new_default() -> AuraResult<Self> {
        Self::open(RuntimeLocation::resolve_daemon_default()?)
    }

    fn open(location: RuntimeLocation) -> AuraResult<Self> {
        let state_lock = lock::acquire(&location)?;
        let file = publish::open_state(&location)?;
        // SAFETY: the validated/newly-created state fd is exactly `SHM_SIZE`,
        // matching the writable mapping length.
        let mmap = unsafe { MmapOptions::new().len(SHM_SIZE).map_mut(&file)? };
        Ok(Self {
            mmap,
            _lock: state_lock,
            _location: location,
        })
    }

    pub fn write(&mut self, telemetry: &mut TelemetryArchive) -> AuraResult<()> {
        telemetry.version = aura_common::ARCHIVE_VERSION;
        telemetry.checksum = 0;
        telemetry.checksum = telemetry.calculate_checksum();
        // SAFETY: `self.mmap` is a writable `SHM_SIZE` mapping with the expected
        // header and archive buffers; `telemetry` is initialized.
        unsafe {
            write_double_buffer(self.mmap.as_mut_ptr(), telemetry);
        }
        Ok(())
    }
}
