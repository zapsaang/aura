use crate::error::AuraResult;
use crate::TelemetryArchive;

use super::{check_rate, expect_zero, fault, owned_f32, owned_u64, rate_path};
use crate::archive::capabilities::{
    CAP_MEMORY_BUFFERS, CAP_MEMORY_CACHED, CAP_MEMORY_PAGE_FAULTS, CAP_MEMORY_RAM_FREE,
    CAP_MEMORY_RAM_TOTAL, CAP_MEMORY_RAM_USED, CAP_MEMORY_SWAP,
};

pub(super) fn validate(a: &TelemetryArchive) -> AuraResult<()> {
    let caps = a.capabilities;
    let m = &a.memory;
    let total_owned = caps & CAP_MEMORY_RAM_TOTAL != 0;
    let free_owned = caps & CAP_MEMORY_RAM_FREE != 0;
    let used_owned = caps & CAP_MEMORY_RAM_USED != 0;
    let swap_owned = caps & CAP_MEMORY_SWAP != 0;
    let faults_owned = caps & CAP_MEMORY_PAGE_FAULTS != 0;

    owned_u64(m.ram_total, total_owned, "memory.ram_total")?;
    if !free_owned {
        owned_u64(m.ram_free, false, "memory.ram_free")?;
    } else if m.ram_free > m.ram_total {
        return Err(fault(
            "memory.ram_free".to_string(),
            format!("{} exceeds {}", m.ram_free, m.ram_total),
        ));
    }
    if !used_owned {
        owned_u64(m.ram_used, false, "memory.ram_used")?;
    } else if m.ram_used > m.ram_total {
        return Err(fault(
            "memory.ram_used".to_string(),
            format!("{} exceeds {}", m.ram_used, m.ram_total),
        ));
    }
    owned_u64(m.buffers, caps & CAP_MEMORY_BUFFERS != 0, "memory.buffers")?;
    owned_u64(m.cached, caps & CAP_MEMORY_CACHED != 0, "memory.cached")?;
    owned_u64(m.swap_total, swap_owned, "memory.swap_total")?;
    owned_u64(m.swap_free, swap_owned, "memory.swap_free")?;
    if swap_owned && m.swap_free as u128 + m.swap_used as u128 != m.swap_total as u128 {
        return Err(fault(
            "memory.swap_free".to_string(),
            "inconsistent with memory.swap_total".to_string(),
        ));
    }
    owned_u64(m.swap_used, swap_owned, "memory.swap_used")?;
    owned_u64(m.page_faults, faults_owned, "memory.page_faults")?;
    owned_f32(
        m.page_faults_per_sec,
        faults_owned,
        &rate_path("memory", "page_faults"),
        check_rate,
    )?;
    expect_zero(&m._pad0, "memory.padding")?;
    Ok(())
}
