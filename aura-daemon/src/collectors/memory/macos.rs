use aura_common::{AuraResult, MemoryStats};

pub fn collect(
    _meminfo_buf: &mut Vec<u8>,
    _vmstat_buf: &mut Vec<u8>,
    out: &mut MemoryStats,
    prev_page_faults: &mut u64,
    _delta_secs: f64,
) -> AuraResult<()> {
    let provider = crate::platform::macos::provider()?;
    let mem = provider.memory_stats()?;
    *prev_page_faults = mem.page_faults;
    *out = mem;
    Ok(())
}
