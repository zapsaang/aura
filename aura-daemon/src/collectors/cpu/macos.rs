use aura_common::{AuraResult, CpuGlobalStat};

use super::CpuAvailability;

pub fn collect(_buf: &mut Vec<u8>, out: &mut CpuGlobalStat) -> AuraResult<CpuAvailability> {
    let provider = crate::platform::macos::provider()?;
    let cpu = provider.cpu_stats()?;
    *out = cpu;
    Ok(CpuAvailability {
        context_switches: false,
        over_capacity: false,
    })
}
