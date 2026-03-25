use aura_common::{AuraError, AuraResult, CpuGlobalStat, MemoryStats, ProcessStats};

#[cfg(target_os = "macos")]
use aura_common::{CpuCoreStat, ProcessStat, MAX_CORES, MAX_TOP_N};

use super::PlatformStatsProvider;

#[cfg(target_os = "macos")]
type MachPort = libc::c_uint;

#[cfg(target_os = "macos")]
type KernReturn = libc::c_int;

#[cfg(target_os = "macos")]
type ProcessorInfoArray = *mut libc::c_int;

#[cfg(target_os = "macos")]
type MachMsgTypeNumber = libc::c_uint;

#[cfg(target_os = "macos")]
const KERN_SUCCESS: KernReturn = 0;
#[cfg(target_os = "macos")]
const PROCESSOR_CPU_LOAD_INFO: libc::c_int = 2;
#[cfg(target_os = "macos")]
const CPU_STATE_MAX: usize = 4;
#[cfg(target_os = "macos")]
const HOST_VM_INFO64: libc::c_int = 4;

#[cfg(target_os = "macos")]
unsafe extern "C" {
    fn mach_host_self() -> MachPort;
    fn mach_task_self() -> MachPort;
    fn host_processor_info(
        host: MachPort,
        flavor: libc::c_int,
        out_processor_count: *mut libc::c_uint,
        out_processor_info: *mut ProcessorInfoArray,
        out_processor_info_count: *mut MachMsgTypeNumber,
    ) -> KernReturn;
    fn host_statistics64(
        host: MachPort,
        flavor: libc::c_int,
        host_info: *mut libc::c_int,
        host_info_count: *mut MachMsgTypeNumber,
    ) -> KernReturn;
    fn vm_deallocate(target_task: MachPort, address: usize, size: usize) -> KernReturn;
    fn proc_listallpids(buffer: *mut libc::c_void, buffersize: libc::c_int) -> libc::c_int;
}

#[derive(Clone, Copy, Debug)]
pub struct MacosPlatform {
    #[cfg(target_os = "macos")]
    host_port: MachPort,
}

impl MacosPlatform {
    pub fn new() -> AuraResult<Self> {
        #[cfg(target_os = "macos")]
        {
            let host_port = unsafe { mach_host_self() };
            return Ok(Self { host_port });
        }

        #[cfg(not(target_os = "macos"))]
        {
            Err(AuraError::PlatformNotSupported(
                "macOS platform is only available on macOS targets".to_string(),
            ))
        }
    }
}

impl PlatformStatsProvider for MacosPlatform {
    fn name(&self) -> &'static str {
        "macos"
    }

    fn cpu_stats(&self) -> AuraResult<CpuGlobalStat> {
        #[cfg(target_os = "macos")]
        {
            let mut processor_count: libc::c_uint = 0;
            let mut cpu_info: ProcessorInfoArray = std::ptr::null_mut();
            let mut cpu_info_count: MachMsgTypeNumber = 0;

            let ret = unsafe {
                host_processor_info(
                    self.host_port,
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
                        user_ticks: c_user.saturating_add(c_nice),
                        system_ticks: c_system,
                        idle_ticks: c_idle,
                        total_ticks: c_total,
                        usage_percent: if c_total > 0 {
                            ((c_total.saturating_sub(c_idle)) as f32 / c_total as f32) * 100.0
                        } else {
                            0.0
                        },
                    };

                    core_idx += 1;
                    idx += CPU_STATE_MAX;
                }
                core_count = core_idx as u8;

                let _ = unsafe {
                    vm_deallocate(
                        mach_task_self(),
                        cpu_info as usize,
                        cpu_info_count as usize * std::mem::size_of::<libc::c_int>(),
                    )
                };
            }

            return Ok(CpuGlobalStat {
                user_ticks: user,
                system_ticks: system,
                idle_ticks: idle,
                total_ticks: total,
                context_switches: 0,
                context_switches_per_sec: 0.0,
                usage_percent: if total > 0 {
                    ((total.saturating_sub(idle)) as f32 / total as f32) * 100.0
                } else {
                    0.0
                },
                cores,
                core_count,
            });
        }

        #[cfg(not(target_os = "macos"))]
        {
            Err(AuraError::PlatformNotSupported(
                "macOS platform is only available on macOS targets".to_string(),
            ))
        }
    }

    fn memory_stats(&self) -> AuraResult<MemoryStats> {
        #[cfg(target_os = "macos")]
        {
            let mut stats_buf = [0i32; 128];
            let mut count = stats_buf.len() as MachMsgTypeNumber;
            let ret = unsafe {
                host_statistics64(
                    self.host_port,
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

            let page_size = 4096u64;
            let free =
                (stats_buf.get(0).copied().unwrap_or_default() as u64).saturating_mul(page_size);
            let active =
                (stats_buf.get(1).copied().unwrap_or_default() as u64).saturating_mul(page_size);
            let inactive =
                (stats_buf.get(2).copied().unwrap_or_default() as u64).saturating_mul(page_size);
            let wired =
                (stats_buf.get(6).copied().unwrap_or_default() as u64).saturating_mul(page_size);

            let total = free
                .saturating_add(active)
                .saturating_add(inactive)
                .saturating_add(wired);

            return Ok(MemoryStats {
                ram_total: total,
                ram_free: free,
                ram_used: total.saturating_sub(free),
                buffers: 0,
                cached: inactive,
                swap_total: 0,
                swap_free: 0,
                swap_used: 0,
                page_faults: (stats_buf.get(7).copied().unwrap_or_default() as u64),
                page_faults_per_sec: 0.0,
            });
        }

        #[cfg(not(target_os = "macos"))]
        {
            Err(AuraError::PlatformNotSupported(
                "macOS platform is only available on macOS targets".to_string(),
            ))
        }
    }

    fn process_stats(&self) -> AuraResult<ProcessStats> {
        #[cfg(target_os = "macos")]
        {
            let pid_cap = 4096usize;
            let mut pids = vec![0u32; pid_cap];
            let bytes = unsafe {
                proc_listallpids(
                    pids.as_mut_ptr() as *mut libc::c_void,
                    (pid_cap * std::mem::size_of::<u32>()) as libc::c_int,
                )
            };

            if bytes < 0 {
                return Err(AuraError::PlatformNotSupported(
                    "proc_listallpids failed".to_string(),
                ));
            }

            let count = (bytes as usize) / std::mem::size_of::<u32>();

            return Ok(ProcessStats {
                total: count as u32,
                running: 0,
                blocked: 0,
                sleeping: 0,
                top_cpu: [zero_process(); MAX_TOP_N],
                top_mem: [zero_process(); MAX_TOP_N],
            });
        }

        #[cfg(not(target_os = "macos"))]
        {
            Err(AuraError::PlatformNotSupported(
                "macOS platform is only available on macOS targets".to_string(),
            ))
        }
    }
}

#[cfg(target_os = "macos")]
const fn zero_core() -> CpuCoreStat {
    CpuCoreStat {
        core_index: 0,
        user_ticks: 0,
        system_ticks: 0,
        idle_ticks: 0,
        total_ticks: 0,
        usage_percent: 0.0,
    }
}

#[cfg(target_os = "macos")]
const fn zero_process() -> ProcessStat {
    ProcessStat {
        pid: 0,
        cpu_usage: 0.0,
        memory_bytes: 0,
        comm: aura_common::FixedString16 { bytes: [0; 16] },
    }
}

#[cfg(test)]
mod tests {
    use super::MacosPlatform;

    #[cfg(target_os = "macos")]
    use crate::platform::PlatformStatsProvider;

    #[test]
    #[cfg(not(target_os = "macos"))]
    fn constructor_is_unsupported_off_macos() {
        let err = MacosPlatform::new().expect_err("expected unsupported platform error");
        let msg = err.to_string();
        assert!(msg.contains("macOS platform is only available"));
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn stub_collectors_return_structs() {
        let provider = MacosPlatform::new().expect("macos provider");
        let cpu = provider.cpu_stats().expect("cpu");
        let mem = provider.memory_stats().expect("memory");
        let proc = provider.process_stats().expect("process");

        assert!(cpu.total_ticks >= cpu.idle_ticks);
        assert!(mem.ram_total >= mem.ram_free);
        assert!(proc.total <= u32::MAX);
    }
}
