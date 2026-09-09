use aura_common::{AuraError, AuraResult, CpuCoreStat, CpuGlobalStat, MAX_CORES};

use super::ffi::{
    host_processor_info, mach_task_self, vm_deallocate, MachMsgTypeNumber, ProcessorInfoArray,
    CPU_STATE_MAX, KERN_SUCCESS, PROCESSOR_CPU_LOAD_INFO,
};
use super::MacosPlatform;

pub(super) fn collect(platform: &MacosPlatform) -> AuraResult<CpuGlobalStat> {
    let mut processor_count: libc::c_uint = 0;
    let mut cpu_info: ProcessorInfoArray = std::ptr::null_mut();
    let mut cpu_info_count: MachMsgTypeNumber = 0;
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
                usage_percent: 0.0,
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

    Ok(CpuGlobalStat {
        user_ticks: user,
        system_ticks: system,
        idle_ticks: idle,
        total_ticks: total,
        context_switches: 0,
        context_switches_per_sec: 0.0,
        usage_percent: 0.0,
        cores,
        core_count,
        _pad0: [0; 7],
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
