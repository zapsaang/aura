use aura_common::{AuraError, AuraResult, CpuCoreStat, CpuGlobalStat, MAX_CORES};

use super::CpuAvailability;

const CPU_STATE_MAX: usize = 4;

/// Raw host_processor_info(PROCESSOR_CPU_LOAD_INFO) query. The trait exposes
/// only the stable byte/query descriptor; parsing and capability decisions
/// live in this module.
pub trait MacosCpuProbe {
    /// Returns `(processor_count, raw integer_t lanes)` borrowed from the
    /// probe, or the raw `kern_return_t` on failure.
    fn processor_load_info(&mut self) -> Result<(u32, &[i32]), i32>;
}

fn fatal(message: String) -> AuraError {
    AuraError::Fatal(message)
}

fn lane(value: i32) -> AuraResult<u64> {
    u64::try_from(value).map_err(|_| fatal("host_processor_info negative tick".to_string()))
}

/// Maps raw PROCESSOR_CPU_LOAD_INFO lanes into the archive CPU section.
/// Per-core order is CPU_STATE_USER/SYSTEM/IDLE/NICE with NICE folded into
/// user; the aggregate is the exact sum over every online core. More than
/// `MAX_CORES` online cores keeps the aggregate but drops per-core
/// representation via `over_capacity`.
pub fn collect_cpu_from_probe<P: MacosCpuProbe + ?Sized>(
    probe: &mut P,
    out: &mut CpuGlobalStat,
) -> AuraResult<CpuAvailability> {
    let (processor_count, info) = probe
        .processor_load_info()
        .map_err(|code| fatal(format!("host_processor_info failed: kern_return_t {code}")))?;
    if processor_count == 0 {
        return Err(fatal(
            "host_processor_info reported zero processors".to_string(),
        ));
    }
    let expected = processor_count as usize * CPU_STATE_MAX;
    if info.len() != expected {
        return Err(fatal(format!(
            "host_processor_info count mismatch: {} lanes for {processor_count} processors",
            info.len()
        )));
    }

    let mut user = 0u64;
    let mut system = 0u64;
    let mut idle = 0u64;
    let mut total = 0u64;
    let online = processor_count as usize;
    let over_capacity = online > MAX_CORES;
    let represented = if over_capacity { 0 } else { online };

    for (index, lanes) in info.chunks_exact(CPU_STATE_MAX).enumerate() {
        let c_user = lane(lanes[0])?.saturating_add(lane(lanes[3])?);
        let c_system = lane(lanes[1])?;
        let c_idle = lane(lanes[2])?;
        let c_total = c_user.saturating_add(c_system).saturating_add(c_idle);
        user = user.saturating_add(c_user);
        system = system.saturating_add(c_system);
        idle = idle.saturating_add(c_idle);
        total = total.saturating_add(c_total);
        if index < represented {
            out.cores[index] = CpuCoreStat {
                core_index: index as u8,
                _pad0: [0; 7],
                user_ticks: c_user,
                system_ticks: c_system,
                idle_ticks: c_idle,
                total_ticks: c_total,
                usage_percent: 0.0,
                _pad1: [0; 4],
            };
        }
    }
    for core in &mut out.cores[represented..] {
        *core = zero_core();
    }

    out.user_ticks = user;
    out.system_ticks = system;
    out.idle_ticks = idle;
    out.total_ticks = total;
    out.context_switches = 0;
    out.context_switches_per_sec = 0.0;
    out.usage_percent = 0.0;
    out.core_count = represented as u8;

    Ok(CpuAvailability {
        context_switches: false,
        over_capacity,
    })
}

const fn zero_core() -> CpuCoreStat {
    CpuCoreStat {
        core_index: 0,
        _pad0: [0; 7],
        user_ticks: 0,
        system_ticks: 0,
        idle_ticks: 0,
        total_ticks: 0,
        usage_percent: 0.0,
        _pad1: [0; 4],
    }
}

#[cfg(target_os = "macos")]
pub fn collect(_buf: &mut Vec<u8>, out: &mut CpuGlobalStat) -> AuraResult<CpuAvailability> {
    let mut host = crate::platform::macos::host()?;
    collect_cpu_from_probe(&mut host, out)
}
