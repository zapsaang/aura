#[cfg(target_os = "linux")]
pub mod linux;

#[cfg(target_os = "macos")]
pub mod macos;

use aura_common::{AuraResult, StorageStats};

use super::DiskSectorSnapshot;

pub trait DiskCollector: Send + Sync {
    fn collect(
        &self,
        diskstats_buf: &mut [u8; 4096],
        mounts_buf: &mut [u8; 4096],
        out: &mut StorageStats,
        prev: &mut DiskSectorSnapshot,
        delta_secs: f32,
    ) -> AuraResult<()>;
}

pub type Collector = dyn DiskCollector;
