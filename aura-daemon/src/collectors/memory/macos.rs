use aura_common::{AuraResult, MemoryStats};

use super::MemoryAvailability;

pub fn collect(
    _meminfo_buf: &mut Vec<u8>,
    _vmstat_buf: &mut Vec<u8>,
    out: &mut MemoryStats,
) -> AuraResult<MemoryAvailability> {
    let provider = crate::platform::macos::provider()?;
    let mem = provider.memory_stats()?;
    *out = mem;
    Ok(MemoryAvailability {
        buffers: false,
        cached: true,
        swap: false,
        page_faults: true,
    })
}
