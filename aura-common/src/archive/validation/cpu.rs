use crate::error::AuraResult;
use crate::TelemetryArchive;

use super::{
    check_percent, check_rate, expect_zero, expect_zero_u8, fault, owned_f32, owned_u64, rate_path,
};
use crate::archive::capabilities::{
    CAP_CPU_CONTEXT_SWITCHES, CAP_CPU_GLOBAL, CAP_CPU_PER_CORE, CAP_PROCESS_TOP_CPU,
};
use crate::archive::MAX_CORES;

fn validate_core(core: &crate::CpuCoreStat, index: usize, active: bool) -> AuraResult<()> {
    let base = format!("cpu.cores[{index}]");
    if !active {
        return expect_zero(bytemuck::bytes_of(core), &base);
    }
    if core.core_index != index as u8 {
        return Err(fault(
            format!("{base}.core_index"),
            "inconsistent with cpu.core_count".to_string(),
        ));
    }
    expect_zero(&core._pad0, &format!("{base}.leading_padding"))?;
    let sum = core.user_ticks as u128 + core.system_ticks as u128 + core.idle_ticks as u128;
    if (core.total_ticks as u128) < sum {
        return Err(fault(
            format!("{base}.total_ticks"),
            format!("inconsistent with {base}.user_ticks"),
        ));
    }
    check_percent(core.usage_percent, &format!("{base}.usage_percent"))?;
    expect_zero(&core._pad1, &format!("{base}.trailing_padding"))?;
    Ok(())
}

pub(super) fn validate(a: &TelemetryArchive) -> AuraResult<()> {
    let caps = a.capabilities;
    let global = caps & CAP_CPU_GLOBAL != 0;
    let per_core = caps & CAP_CPU_PER_CORE != 0;
    let context = caps & CAP_CPU_CONTEXT_SWITCHES != 0;
    let cpu = &a.cpu;

    owned_u64(cpu.user_ticks, global, "cpu.user_ticks")?;
    owned_u64(cpu.system_ticks, global, "cpu.system_ticks")?;
    owned_u64(cpu.idle_ticks, global, "cpu.idle_ticks")?;
    if !global {
        owned_u64(cpu.total_ticks, false, "cpu.total_ticks")?;
    } else {
        let sum = cpu.user_ticks as u128 + cpu.system_ticks as u128 + cpu.idle_ticks as u128;
        if (cpu.total_ticks as u128) < sum {
            return Err(fault(
                "cpu.total_ticks".to_string(),
                "inconsistent with cpu.user_ticks".to_string(),
            ));
        }
    }
    owned_u64(cpu.context_switches, context, "cpu.context_switches")?;
    owned_f32(
        cpu.context_switches_per_sec,
        context,
        &rate_path("cpu", "context_switches"),
        check_rate,
    )?;
    owned_f32(
        cpu.usage_percent,
        global,
        "cpu.usage_percent",
        check_percent,
    )?;

    let represented = (cpu.core_count as usize).min(MAX_CORES);
    for (index, core) in cpu.cores.iter().enumerate() {
        validate_core(core, index, per_core && index < represented)?;
    }

    if !global {
        expect_zero_u8(cpu.core_count, "cpu.core_count")?;
    } else if cpu.core_count as usize > MAX_CORES {
        return Err(fault(
            "cpu.core_count".to_string(),
            format!("{} exceeds {MAX_CORES}", cpu.core_count),
        ));
    } else if caps & CAP_PROCESS_TOP_CPU != 0 && cpu.core_count == 0 {
        return Err(fault(
            "cpu.core_count".to_string(),
            format!("outside 1..={MAX_CORES}"),
        ));
    }
    expect_zero(&cpu._pad0, "cpu.padding")?;
    Ok(())
}
