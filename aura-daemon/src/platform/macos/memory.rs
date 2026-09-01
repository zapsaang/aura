use aura_common::{system_page_size, AuraError, AuraResult, MemoryStats};

use super::ffi::{
    host_statistics64, MachMsgTypeNumber, HOST_VM_INFO64, KERN_SUCCESS, VM_STAT_ACTIVE_COUNT,
    VM_STAT_FAULTS, VM_STAT_FREE_COUNT, VM_STAT_INACTIVE_COUNT, VM_STAT_WIRE_COUNT,
};
use super::MacosPlatform;

pub(super) fn collect(platform: &MacosPlatform) -> AuraResult<MemoryStats> {
    let mut stats_buf = [0i32; 128];
    let mut count = stats_buf.len() as MachMsgTypeNumber;
    // SAFETY: `stats_buf` is writable for `count` integer lanes and `platform.host_port` is a valid host port.
    let ret = unsafe {
        host_statistics64(
            platform.host_port,
            HOST_VM_INFO64,
            stats_buf.as_mut_ptr(),
            &mut count,
        )
    };

    if ret != KERN_SUCCESS {
        return Err(AuraError::PlatformNotSupported(format!(
            "host_statistics64 failed: {ret}",
        )));
    }

    let page_size = system_page_size() as u64;
    let free = (stats_buf
        .get(VM_STAT_FREE_COUNT)
        .copied()
        .unwrap_or_default() as u64)
        .saturating_mul(page_size);
    let active = (stats_buf
        .get(VM_STAT_ACTIVE_COUNT)
        .copied()
        .unwrap_or_default() as u64)
        .saturating_mul(page_size);
    let inactive = (stats_buf
        .get(VM_STAT_INACTIVE_COUNT)
        .copied()
        .unwrap_or_default() as u64)
        .saturating_mul(page_size);
    let wired = (stats_buf
        .get(VM_STAT_WIRE_COUNT)
        .copied()
        .unwrap_or_default() as u64)
        .saturating_mul(page_size);
    let total = free
        .saturating_add(active)
        .saturating_add(inactive)
        .saturating_add(wired);

    Ok(MemoryStats {
        ram_total: total,
        ram_free: free,
        ram_used: total.saturating_sub(free),
        buffers: 0,
        cached: inactive,
        swap_total: 0,
        swap_free: 0,
        swap_used: 0,
        page_faults: (stats_buf.get(VM_STAT_FAULTS).copied().unwrap_or_default() as u64),
        page_faults_per_sec: 0.0,
        _pad0: [0; 4],
    })
}
