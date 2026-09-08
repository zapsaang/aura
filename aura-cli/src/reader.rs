use std::path::Path;
use std::time::Duration;

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
        })
    }

    pub fn read(&self) -> AuraResult<TelemetryArchive> {
        // SAFETY: `self.mmap` covers the full SHM layout and `read_double_buffer` only performs atomic reads from it.
        let mut snapshot = match unsafe { read_double_buffer(self.mmap.as_ptr()) } {
            Ok(snapshot) => snapshot,
            Err(AuraError::NotPublished) => {
                return Err(AuraError::Offline(
                    "telemetry has not been published".to_string(),
                ));
            }
            Err(AuraError::SeqLockTimeout) => {
                return Err(AuraError::Offline(
                    "seqlock read retry-admission deadline exhausted".to_string(),
                ));
            }
            Err(error) => return Err(error),
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

        if snapshot.meta.timestamp_ns == 0 {
            return Err(AuraError::Offline(
                "telemetry producer timestamp is zero".to_string(),
            ));
        }

        validate_archive(&snapshot)?;

        Ok(snapshot)
    }

    pub fn is_fresh(&self, telemetry: &TelemetryArchive, threshold: Duration) -> bool {
        timestamp_is_fresh(telemetry.meta.timestamp_ns, monotonic_ns(), threshold)
    }
}

/// Compare a producer monotonic timestamp with the consumer's current clock.
#[doc(hidden)]
pub fn timestamp_is_fresh(timestamp_ns: u64, now_ns: u64, threshold: Duration) -> bool {
    if timestamp_ns == 0 || timestamp_ns > now_ns {
        return false;
    }
    let threshold_ns = u64::try_from(threshold.as_nanos()).unwrap_or(u64::MAX);
    now_ns - timestamp_ns <= threshold_ns
}

#[cfg(test)]
mod tests;
