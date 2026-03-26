#[cfg(target_os = "linux")]
pub mod linux;

#[cfg(target_os = "macos")]
pub mod macos;

use aura_common::{AuraResult, CpuGlobalStat};

use super::CpuTickSnapshot;

pub trait CpuCollector: Send + Sync {
    fn collect(
        &self,
        buf: &mut [u8; 4096],
        out: &mut CpuGlobalStat,
        prev: &mut CpuTickSnapshot,
        delta_secs: f32,
    ) -> AuraResult<()>;
}

pub type Collector = dyn CpuCollector;
