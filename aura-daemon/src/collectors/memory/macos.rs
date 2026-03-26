use aura_common::{AuraResult, MemoryStats};

pub struct MacosMemoryCollector;

impl super::MemoryCollector for MacosMemoryCollector {
    fn collect(
        &self,
        _meminfo_buf: &mut [u8; 4096],
        _vmstat_buf: &mut [u8; 4096],
        out: &mut MemoryStats,
        prev_page_faults: &mut u64,
        _delta_secs: f32,
    ) -> AuraResult<()> {
        let provider = crate::platform::macos::provider()?;
        let mem = provider.memory_stats()?;
        *prev_page_faults = mem.page_faults;
        *out = mem;
        Ok(())
    }
}
