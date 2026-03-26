use aura_common::{AuraResult, CpuGlobalStat};

use crate::collectors::CpuTickSnapshot;

pub struct MacosCpuCollector;

impl super::CpuCollector for MacosCpuCollector {
    fn collect(
        &self,
        _buf: &mut [u8; 4096],
        out: &mut CpuGlobalStat,
        prev: &mut CpuTickSnapshot,
        _delta_secs: f32,
    ) -> AuraResult<()> {
        let provider = crate::platform::macos::provider()?;
        let cpu = provider.cpu_stats()?;
        prev.user = cpu.user_ticks;
        prev.system = cpu.system_ticks;
        prev.idle = cpu.idle_ticks;
        prev.total = cpu.total_ticks;
        prev.context_switches = cpu.context_switches;
        *out = cpu;
        Ok(())
    }
}
