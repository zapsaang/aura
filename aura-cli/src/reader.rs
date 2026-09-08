use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use aura_common::runtime::RuntimeLocation;
use aura_common::{
    monotonic_ns, read_double_buffer, validate_archive, AuraError, AuraResult, TelemetryArchive,
    ARCHIVE_VERSION, SHM_SIZE,
};
use memmap2::{Mmap, MmapOptions};

/// Consumer handle over the shared-memory state.
///
/// The runtime parent directory descriptor is retained for the lifetime
/// of the reader; the state leaf is opened no-follow relative to it and
/// validated (regular type, size, owner, mode) before mapping. The
/// reader never creates any AURA leaf: a missing default runtime dir or
/// state maps to the offline class.
#[derive(Debug)]
pub struct TelemetryReader {
    mmap: Mmap,
    _location: RuntimeLocation,
    path: PathBuf,
}

impl TelemetryReader {
    /// Open state at an operator-supplied absolute path whose parent
    /// already exists as an euid-owned 0700 directory.
    pub fn new(path: &Path) -> AuraResult<Self> {
        Self::open(RuntimeLocation::resolve_override(path.as_os_str())?)
    }

    /// Open state at the private per-user platform default.
    pub fn new_default() -> AuraResult<Self> {
        Self::open(RuntimeLocation::resolve_cli_default()?)
    }

    #[doc(hidden)]
    pub fn new_default_under(parent: &Path, child: &str) -> AuraResult<Self> {
        Self::open(RuntimeLocation::resolve_default_under(
            parent, child, false,
        )?)
    }

    fn open(location: RuntimeLocation) -> AuraResult<Self> {
        let path = location.state_path();
        let file = location.open_state_read()?;
        // SAFETY: the validated state fd is exactly `SHM_SIZE`; the read-only
        // mapping length matches the SHM layout.
        let mmap = unsafe {
            MmapOptions::new()
                .len(SHM_SIZE)
                .map(&file)
                .map_err(|e| AuraError::MmapFailed(e.to_string()))?
        };
        Ok(Self {
            mmap,
            _location: location,
            path,
        })
    }

    pub fn read(&self) -> AuraResult<TelemetryArchive> {
        // SAFETY: `self.mmap` covers the full SHM layout and `read_double_buffer` only performs atomic reads from it.
        let mut snapshot = unsafe {
            read_double_buffer(self.mmap.as_ptr() as *mut u8)
                .map_err(|()| AuraError::SeqLockInvalid)?
        };

        if snapshot.version != ARCHIVE_VERSION {
            return Err(AuraError::UnsupportedVersion {
                found: snapshot.version,
            });
        }

        let expected = snapshot.checksum;
        snapshot.checksum = 0;
        let actual = snapshot.calculate_checksum();
        snapshot.checksum = expected;

        if expected != actual {
            return Err(AuraError::ChecksumMismatch { expected, actual });
        }

        validate_archive(&snapshot)?;

        Ok(snapshot)
    }

    pub fn is_fresh(&self, telemetry: &TelemetryArchive, threshold: Duration) -> bool {
        if telemetry.meta.timestamp_ns > 0 {
            let now = monotonic_ns();
            let age_ns = now.saturating_sub(telemetry.meta.timestamp_ns);
            let threshold_ns = threshold.as_nanos() as u64;
            if age_ns <= threshold_ns {
                return true;
            }
        }

        self.file_is_fresh(threshold)
    }

    fn file_is_fresh(&self, threshold: Duration) -> bool {
        let Ok(metadata) = std::fs::metadata(&self.path) else {
            return false;
        };
        let Ok(modified) = metadata.modified() else {
            return false;
        };
        let Ok(elapsed) = SystemTime::now().duration_since(modified) else {
            return true;
        };
        elapsed <= threshold
    }
}

#[cfg(test)]
mod tests;
