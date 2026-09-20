//! Process collection: Linux `/proc` scanning over fixed baseline tables;
//! macOS is intentionally unsupported (Unavailable + zeroed stats).

#[cfg(target_os = "linux")]
use aura_common::{AuraError, AuraResult};

pub mod state;

#[cfg(target_os = "linux")]
pub mod linux;

mod macos;

pub use state::{
    ProcessBaseSnapshot, ProcessBaseStat, ProcessBaseline, ProcessProcStat,
    PROCESS_BASELINE_CAPACITY,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProcessAvailability {
    pub running: bool,
    pub total: bool,
}

impl ProcessAvailability {
    pub const fn unavailable() -> Self {
        Self {
            running: false,
            total: false,
        }
    }

    pub const fn capability_mask(self) -> u64 {
        use aura_common::{
            CAP_PROCESS_BLOCKED, CAP_PROCESS_RUNNING, CAP_PROCESS_SLEEPING, CAP_PROCESS_TOP_CPU,
            CAP_PROCESS_TOP_MEMORY, CAP_PROCESS_TOTAL,
        };
        let mut mask = 0;
        if self.total {
            mask |= CAP_PROCESS_TOTAL;
        }
        if self.running {
            mask |= CAP_PROCESS_RUNNING
                | CAP_PROCESS_BLOCKED
                | CAP_PROCESS_SLEEPING
                | CAP_PROCESS_TOP_CPU
                | CAP_PROCESS_TOP_MEMORY;
        }
        mask
    }
}

/// Validates a raw `sysconf(_SC_PAGESIZE)` result; `-1`, zero and conversion
/// failures are Fatal.
#[cfg(target_os = "linux")]
pub fn validate_page_size(raw: i64) -> AuraResult<u64> {
    match u64::try_from(raw) {
        Ok(value) if value > 0 => Ok(value),
        _ => Err(AuraError::Fatal(format!(
            "sysconf(_SC_PAGESIZE) returned {raw}"
        ))),
    }
}

/// Reads and validates the page size once at collector init.
#[cfg(target_os = "linux")]
pub fn cache_page_size() -> AuraResult<u64> {
    // SAFETY: sysconf is thread-safe with no side effects; -1 signals failure.
    let raw = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
    validate_page_size(raw)
}

pub(crate) fn zero_stats() -> aura_common::ProcessStats {
    macos::collect()
}

/// Unsupported process telemetry is capability-clear with a zeroed shape.
pub fn mark_unavailable(out: &mut aura_common::ProcessStats) {
    *out = zero_stats();
}
