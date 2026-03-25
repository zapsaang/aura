use std::sync::OnceLock;

use aura_common::{AuraError, AuraResult, CpuGlobalStat, MemoryStats, ProcessStats};

#[cfg(target_os = "linux")]
pub mod linux;
pub mod macos;

pub trait PlatformStatsProvider: Send + Sync {
    fn name(&self) -> &'static str;
    fn cpu_stats(&self) -> AuraResult<CpuGlobalStat>;
    fn memory_stats(&self) -> AuraResult<MemoryStats>;
    fn process_stats(&self) -> AuraResult<ProcessStats>;
}

static PROVIDER: OnceLock<Box<dyn PlatformStatsProvider>> = OnceLock::new();

pub fn init() -> AuraResult<&'static dyn PlatformStatsProvider> {
    if PROVIDER.get().is_none() {
        let provider: Box<dyn PlatformStatsProvider> = {
            #[cfg(target_os = "linux")]
            {
                linux::init();
                Box::new(linux::LinuxPlatform)
            }

            #[cfg(target_os = "macos")]
            {
                Box::new(macos::MacosPlatform::new()?)
            }

            #[cfg(not(any(target_os = "linux", target_os = "macos")))]
            {
                return Err(AuraError::PlatformNotSupported(
                    std::env::consts::OS.to_string(),
                ));
            }
        };

        let _ = PROVIDER.set(provider);
    }

    provider()
}

pub fn provider() -> AuraResult<&'static dyn PlatformStatsProvider> {
    PROVIDER.get().map(|p| p.as_ref()).ok_or_else(|| {
        AuraError::PlatformNotSupported("platform provider not initialized".to_string())
    })
}
