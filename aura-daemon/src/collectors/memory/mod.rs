#[cfg(target_os = "linux")]
pub mod linux;

#[cfg(target_os = "macos")]
pub mod macos;

use aura_common::{AuraResult, MemoryStats};

pub trait MemoryCollector: Send + Sync {
    fn collect(
        &self,
        meminfo_buf: &mut [u8; 4096],
        vmstat_buf: &mut [u8; 4096],
        out: &mut MemoryStats,
        prev_page_faults: &mut u64,
        delta_secs: f32,
    ) -> AuraResult<()>;
}

pub type Collector = dyn MemoryCollector;
