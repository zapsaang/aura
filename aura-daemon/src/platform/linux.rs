use log::info;

use aura_common::{AuraError, AuraResult, CpuGlobalStat, MemoryStats, ProcessStats};

use super::PlatformStatsProvider;

pub struct LinuxPlatform;

impl PlatformStatsProvider for LinuxPlatform {
    fn name(&self) -> &'static str {
        "linux"
    }

    fn cpu_stats(&self) -> AuraResult<CpuGlobalStat> {
        Err(AuraError::PlatformNotSupported(
            "Linux uses /proc collectors directly, not this trait".to_string(),
        ))
    }

    fn memory_stats(&self) -> AuraResult<MemoryStats> {
        Err(AuraError::PlatformNotSupported(
            "Linux uses /proc collectors directly, not this trait".to_string(),
        ))
    }

    fn process_stats(&self) -> AuraResult<ProcessStats> {
        Err(AuraError::PlatformNotSupported(
            "Linux uses /proc collectors directly, not this trait".to_string(),
        ))
    }
}

pub fn init() {
    info!("aura-daemon running on Linux (using /proc collectors)");
}

pub fn send_watchdog_heartbeat() {
    // Linux watchdog via /proc/sys/kernel/watchdog
}
