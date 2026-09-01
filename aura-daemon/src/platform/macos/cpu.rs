use std::sync::OnceLock;

use aura_common::{AuraError, AuraResult, CpuCoreStat, CpuGlobalStat, MAX_CORES};

use super::ffi::{
    host_processor_info, mach_absolute_time, mach_task_self, mach_timebase_info, vm_deallocate,
    MachMsgTypeNumber, ProcessorInfoArray, CPU_STATE_MAX, KERN_SUCCESS, PROCESSOR_CPU_LOAD_INFO,
};
use super::MacosPlatform;

#[derive(Debug, Default)]
pub(super) struct CpuSnapshot {
    prev_user_ticks: u64,
    prev_system_ticks: u64,
    prev_idle_ticks: u64,
    prev_total_ticks: u64,
    prev_timestamp_ns: u64,
    initialized: bool,
}

pub(super) fn collect(platform: &MacosPlatform) -> AuraResult<CpuGlobalStat> {
    let mut processor_count: libc::c_uint = 0;
    let mut cpu_info: ProcessorInfoArray = std::ptr::null_mut();
    let mut cpu_info_count: MachMsgTypeNumber = 0;
    // SAFETY: `mach_absolute_time` takes no arguments and returns a monotonic tick count.
    let now_timestamp_ns = mach_absolute_to_ns(unsafe { mach_absolute_time() });

    // SAFETY: output pointers are valid for processor count/info/count, and `platform.host_port` came from `mach_host_self`.
    let ret = unsafe {
        host_processor_info(
            platform.host_port,
            PROCESSOR_CPU_LOAD_INFO,
            &mut processor_count,
            &mut cpu_info,
            &mut cpu_info_count,
        )
    };

    if ret != KERN_SUCCESS {
        return Err(AuraError::PlatformNotSupported(format!(
            "host_processor_info failed: {ret}",
        )));
    }

    let mut user = 0u64;
    let mut system = 0u64;
    let mut idle = 0u64;
    let mut total = 0u64;
    let mut cores = [zero_core(); MAX_CORES];
    let mut core_count = 0u8;

    if !cpu_info.is_null() {
        let len = cpu_info_count as usize;
        // SAFETY: `host_processor_info` returned a non-null allocation containing `cpu_info_count` c_int values.
        let values = unsafe { std::slice::from_raw_parts(cpu_info, len) };
        let mut idx = 0usize;
        let mut core_idx = 0usize;
        while idx + CPU_STATE_MAX <= values.len() && core_idx < MAX_CORES {
            let c_user = values[idx] as u64;
            let c_system = values[idx + 1] as u64;
            let c_idle = values[idx + 2] as u64;
            let c_nice = values[idx + 3] as u64;
            let c_total = c_user
                .saturating_add(c_system)
                .saturating_add(c_idle)
                .saturating_add(c_nice);

            user = user.saturating_add(c_user.saturating_add(c_nice));
            system = system.saturating_add(c_system);
            idle = idle.saturating_add(c_idle);
            total = total.saturating_add(c_total);
            cores[core_idx] = CpuCoreStat {
                core_index: core_idx as u8,
                _pad0: [0; 7],
                user_ticks: c_user.saturating_add(c_nice),
                system_ticks: c_system,
                idle_ticks: c_idle,
                total_ticks: c_total,
                usage_percent: if c_total > 0 {
                    ((c_total.saturating_sub(c_idle)) as f32 / c_total as f32) * 100.0
                } else {
                    0.0
                },
                _pad1: [0; 4],
            };
            core_idx += 1;
            idx += CPU_STATE_MAX;
        }
        core_count = core_idx as u8;

        // SAFETY: `cpu_info` and byte size are exactly the allocation returned by `host_processor_info`.
        let _ = unsafe {
            vm_deallocate(
                mach_task_self(),
                cpu_info as usize,
                cpu_info_count as usize * std::mem::size_of::<libc::c_int>(),
            )
        };
    }

    let mut usage_percent = 0.0;
    let mut snapshot = match platform.cpu_snapshot.lock() {
        Ok(guard) => guard,
        Err(err) => err.into_inner(),
    };
    if snapshot.initialized {
        let delta_user = user.saturating_sub(snapshot.prev_user_ticks);
        let delta_system = system.saturating_sub(snapshot.prev_system_ticks);
        let delta_idle = idle.saturating_sub(snapshot.prev_idle_ticks);
        let delta_total = total.saturating_sub(snapshot.prev_total_ticks);
        let delta_ns = now_timestamp_ns.saturating_sub(snapshot.prev_timestamp_ns);
        let delta_secs = delta_ns as f64 / 1_000_000_000.0;

        if delta_total > 0 && delta_secs > 0.0 {
            let busy_ticks = delta_user
                .saturating_add(delta_system)
                .min(delta_total.saturating_sub(delta_idle));
            let bounded_busy_ticks = busy_ticks.min(delta_total);
            let busy_rate = bounded_busy_ticks as f64 / delta_secs;
            let total_rate = delta_total as f64 / delta_secs;
            usage_percent = if total_rate > 0.0 {
                ((busy_rate / total_rate) * 100.0) as f32
            } else {
                0.0
            };
        }
    }

    snapshot.prev_user_ticks = user;
    snapshot.prev_system_ticks = system;
    snapshot.prev_idle_ticks = idle;
    snapshot.prev_total_ticks = total;
    snapshot.prev_timestamp_ns = now_timestamp_ns;
    snapshot.initialized = true;

    Ok(CpuGlobalStat {
        user_ticks: user,
        system_ticks: system,
        idle_ticks: idle,
        total_ticks: total,
        context_switches: 0,
        context_switches_per_sec: 0.0,
        usage_percent,
        cores,
        core_count,
        _pad0: [0; 7],
    })
}

fn mach_timebase_ratio() -> (u64, u64) {
    static TIMEBASE: OnceLock<(u64, u64)> = OnceLock::new();
    *TIMEBASE.get_or_init(|| {
        let mut info = libc::mach_timebase_info_data_t { numer: 0, denom: 0 };
        // SAFETY: `info` is valid writable storage for `mach_timebase_info_data_t`; return and denominator are checked.
        let ret = unsafe { mach_timebase_info(&mut info) };
        if ret == KERN_SUCCESS && info.numer > 0 && info.denom > 0 {
            (u64::from(info.numer), u64::from(info.denom))
        } else {
            (1, 1)
        }
    })
}

fn mach_absolute_to_ns(ticks: u64) -> u64 {
    let (numer, denom) = mach_timebase_ratio();
    ticks.saturating_mul(numer) / denom
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
