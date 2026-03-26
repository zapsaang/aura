#[cfg(target_os = "linux")]
pub mod linux;

#[cfg(target_os = "macos")]
pub mod macos;

use aura_common::{AuraResult, NetworkStats};

use super::NetByteSnapshot;

pub trait NetworkCollector: Send + Sync {
    fn collect(
        &self,
        buf: &mut [u8; 4096],
        out: &mut NetworkStats,
        prev: &mut NetByteSnapshot,
        delta_secs: f32,
    ) -> AuraResult<()>;
}

pub type Collector = dyn NetworkCollector;
