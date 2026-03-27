use std::sync::OnceLock;

use aura_common::{
    system_page_size, AuraError, AuraResult, CpuGlobalStat, MemoryStats, MetaStats, OsFingerprint,
    ProcessStats,
};

#[cfg(target_os = "macos")]
use aura_common::{CpuCoreStat, FixedString16, ProcessStat, MAX_CORES, MAX_TOP_N};

#[cfg(target_os = "macos")]
use std::{collections::HashMap, sync::Mutex};

#[cfg(target_os = "macos")]
use crate::collectors::heap::{zero_process, HeapEntry, MinHeap5};

pub trait PlatformStatsProvider: Send + Sync {
    fn name(&self) -> &'static str;
    fn cpu_stats(&self) -> AuraResult<CpuGlobalStat>;
    fn memory_stats(&self) -> AuraResult<MemoryStats>;
    fn process_stats(&self) -> AuraResult<ProcessStats>;
}

static PROVIDER: OnceLock<Box<dyn PlatformStatsProvider>> = OnceLock::new();

pub fn init() -> AuraResult<&'static dyn PlatformStatsProvider> {
    if PROVIDER.get().is_none() {
        let provider: Box<dyn PlatformStatsProvider> = Box::new(MacosPlatform::new()?);
        let _ = PROVIDER.set(provider);
    }
    provider()
}

pub fn provider() -> AuraResult<&'static dyn PlatformStatsProvider> {
    PROVIDER.get().map(|p| p.as_ref()).ok_or_else(|| {
        AuraError::PlatformNotSupported("platform provider not initialized".to_string())
    })
}

#[cfg(target_os = "macos")]
pub fn boot_time() -> AuraResult<u64> {
    use std::mem::MaybeUninit;

    let mut mib = [libc::CTL_KERN, libc::KERN_BOOTTIME];
    let mut boot_time_val = MaybeUninit::<libc::timeval>::uninit();
    let mut size = std::mem::size_of::<libc::timeval>();

    let ret = unsafe {
        libc::sysctl(
            mib.as_mut_ptr(),
            mib.len() as libc::c_uint,
            boot_time_val.as_mut_ptr() as *mut _,
            &mut size,
            std::ptr::null(),
            0,
        )
    };

    if ret != 0 {
        return Err(AuraError::PlatformNotSupported(
            "sysctl kern.boottime failed".into(),
        ));
    }

    let bt = unsafe { boot_time_val.assume_init() };
    let now = unsafe { libc::time(std::ptr::null_mut()) };

    let uptime = now.saturating_sub(bt.tv_sec as i64) as u64;
    Ok(uptime)
}

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
const PROC_PIDTASKINFO: libc::c_int = 4;
#[cfg(target_os = "macos")]
const PROC_PIDTBSDINFO: libc::c_int = 3;

#[cfg(target_os = "macos")]
const SIDL: u32 = 1;
#[cfg(target_os = "macos")]
const SRUN: u32 = 2;
#[cfg(target_os = "macos")]
const SSLEEP: u32 = 3;
#[cfg(target_os = "macos")]
const SSTOP: u32 = 4;

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
    fn proc_pidinfo(
        pid: libc::c_int,
        flavor: libc::c_int,
        arg: u64,
        buffer: *mut libc::c_void,
        buffersize: libc::c_int,
    ) -> libc::c_int;
    fn proc_name(
        pid: libc::c_int,
        buffer: *mut libc::c_void,
        buffersize: libc::c_uint,
    ) -> libc::c_int;
    fn mach_absolute_time() -> u64;
    fn mach_timebase_info(info: *mut libc::mach_timebase_info_data_t) -> KernReturn;
}

#[cfg(target_os = "macos")]
#[derive(Debug, Default)]
struct ProcessSnapshot {
    prev_proc_ticks: HashMap<u32, u64>,
    prev_total_ticks: u64,
}

#[cfg(target_os = "macos")]
#[derive(Debug, Default)]
struct CpuSnapshot {
    prev_user_ticks: u64,
    prev_system_ticks: u64,
    prev_idle_ticks: u64,
    prev_total_ticks: u64,
    prev_timestamp_ns: u64,
    initialized: bool,
}

#[cfg(target_os = "macos")]
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct ProcTaskInfo {
    pti_virtual_size: u64,
    pti_resident_size: u64,
    pti_total_user: u64,
    pti_total_system: u64,
    pti_threads_user: u64,
    pti_threads_system: u64,
    pti_policy: i32,
    pti_faults: i32,
    pti_pageins: i32,
    pti_cow_faults: i32,
    pti_messages_sent: i32,
    pti_messages_received: i32,
    pti_syscalls_mach: i32,
    pti_syscalls_unix: i32,
    pti_csw: i32,
    pti_threadnum: i32,
    pti_numrunning: i32,
    pti_priority: i32,
}

#[cfg(target_os = "macos")]
#[repr(C)]
#[derive(Clone, Copy)]
struct ProcBsdInfo {
    pbi_flags: u32,
    pbi_status: u32,
    pbi_xstatus: u32,
    pbi_pid: u32,
    pbi_ppid: u32,
    pbi_uid: libc::uid_t,
    pbi_gid: libc::gid_t,
    pbi_ruid: libc::uid_t,
    pbi_rgid: libc::gid_t,
    pbi_svuid: libc::uid_t,
    pbi_svgid: libc::gid_t,
    rfu_1: u32,
    pbi_comm: [libc::c_char; 17],
    pbi_name: [libc::c_char; 2 * 17],
    pbi_nfiles: u32,
    pbi_pgid: u32,
    pbi_pjobc: u32,
    e_tdev: u32,
    e_tpgid: u32,
    pbi_nice: i32,
    pbi_start_tvsec: u64,
    pbi_start_tvusec: u64,
}

#[cfg(target_os = "macos")]
impl Default for ProcBsdInfo {
    fn default() -> Self {
        Self {
            pbi_flags: 0,
            pbi_status: 0,
            pbi_xstatus: 0,
            pbi_pid: 0,
            pbi_ppid: 0,
            pbi_uid: 0,
            pbi_gid: 0,
            pbi_ruid: 0,
            pbi_rgid: 0,
            pbi_svuid: 0,
            pbi_svgid: 0,
            rfu_1: 0,
            pbi_comm: [0; 17],
            pbi_name: [0; 34],
            pbi_nfiles: 0,
            pbi_pgid: 0,
            pbi_pjobc: 0,
            e_tdev: 0,
            e_tpgid: 0,
            pbi_nice: 0,
            pbi_start_tvsec: 0,
            pbi_start_tvusec: 0,
        }
    }
}

#[cfg(target_os = "macos")]
#[derive(Clone, Copy)]
struct ProcSample {
    pid: u32,
    comm: FixedString16,
    memory_bytes: u64,
    delta_ticks: u64,
}

#[derive(Debug)]
pub struct MacosPlatform {
    #[cfg(target_os = "macos")]
    host_port: MachPort,
    #[cfg(target_os = "macos")]
    process_snapshot: Mutex<ProcessSnapshot>,
    #[cfg(target_os = "macos")]
    cpu_snapshot: Mutex<CpuSnapshot>,
}

impl MacosPlatform {
    pub fn new() -> AuraResult<Self> {
        #[cfg(target_os = "macos")]
        {
            let host_port = unsafe { mach_host_self() };
            return Ok(Self {
                host_port,
                process_snapshot: Mutex::new(ProcessSnapshot::default()),
                cpu_snapshot: Mutex::new(CpuSnapshot::default()),
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
            let now_timestamp_ns = mach_absolute_to_ns(unsafe { mach_absolute_time() });

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

                let _ = unsafe {
                    vm_deallocate(
                        mach_task_self(),
                        cpu_info as usize,
                        cpu_info_count as usize * std::mem::size_of::<libc::c_int>(),
                    )
                };
            }

            let mut usage_percent = 0.0;
            let mut snapshot = match self.cpu_snapshot.lock() {
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

            return Ok(CpuGlobalStat {
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

            let page_size = system_page_size() as u64;
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
                _pad0: [0; 4],
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
            let mut out = ProcessStats {
                total: 0,
                running: 0,
                blocked: 0,
                sleeping: 0,
                top_cpu: [zero_process(); MAX_TOP_N],
                top_mem: [zero_process(); MAX_TOP_N],
            };

            let pid_cap = 4096usize;
            let mut pids = vec![0u32; pid_cap];
            let listed = unsafe {
                proc_listallpids(
                    pids.as_mut_ptr() as *mut libc::c_void,
                    (pid_cap * std::mem::size_of::<u32>()) as libc::c_int,
                )
            };

            if listed <= 0 {
                return Ok(out);
            }

            let pid_count = (listed as usize).min(pid_cap);
            let mut snapshot = match self.process_snapshot.lock() {
                Ok(g) => g,
                Err(_) => return Ok(out),
            };

            let mut current_ticks = HashMap::with_capacity(pid_count);
            let mut samples = Vec::with_capacity(pid_count);
            let mut total_ticks = 0u64;

            let taskinfo_size = std::mem::size_of::<ProcTaskInfo>() as libc::c_int;
            let bsdinfo_size = std::mem::size_of::<ProcBsdInfo>() as libc::c_int;

            for pid in &pids[..pid_count] {
                if *pid == 0 {
                    continue;
                }

                let mut taskinfo = ProcTaskInfo::default();
                let task_ret = unsafe {
                    proc_pidinfo(
                        *pid as libc::c_int,
                        PROC_PIDTASKINFO,
                        0,
                        &mut taskinfo as *mut ProcTaskInfo as *mut libc::c_void,
                        taskinfo_size,
                    )
                };
                if task_ret != taskinfo_size {
                    continue;
                }

                let mut bsdinfo = ProcBsdInfo::default();
                let bsd_ret = unsafe {
                    proc_pidinfo(
                        *pid as libc::c_int,
                        PROC_PIDTBSDINFO,
                        0,
                        &mut bsdinfo as *mut ProcBsdInfo as *mut libc::c_void,
                        bsdinfo_size,
                    )
                };
                if bsd_ret != bsdinfo_size {
                    continue;
                }

                out.total = out.total.saturating_add(1);
                match bsdinfo.pbi_status {
                    SRUN => out.running = out.running.saturating_add(1),
                    SSLEEP | SSTOP => out.sleeping = out.sleeping.saturating_add(1),
                    SIDL => out.blocked = out.blocked.saturating_add(1),
                    _ => {}
                }

                let proc_ticks = taskinfo
                    .pti_total_user
                    .saturating_add(taskinfo.pti_total_system);
                total_ticks = total_ticks.saturating_add(proc_ticks);
                current_ticks.insert(*pid, proc_ticks);

                let prev_ticks = snapshot.prev_proc_ticks.get(pid).copied().unwrap_or(0);
                let delta_ticks = proc_ticks.saturating_sub(prev_ticks);

                let mut name_buf = [0u8; 64];
                let name_len = unsafe {
                    proc_name(
                        *pid as libc::c_int,
                        name_buf.as_mut_ptr() as *mut libc::c_void,
                        name_buf.len() as libc::c_uint,
                    )
                };

                let comm = if name_len > 0 {
                    FixedString16::from_bytes(&name_buf[..(name_len as usize).min(name_buf.len())])
                } else {
                    FixedString16::from_bytes(c_char_bytes(&bsdinfo.pbi_comm))
                };

                samples.push(ProcSample {
                    pid: *pid,
                    comm,
                    memory_bytes: taskinfo.pti_resident_size,
                    delta_ticks,
                });
            }

            let global_delta = total_ticks.saturating_sub(snapshot.prev_total_ticks);
            snapshot.prev_total_ticks = total_ticks;
            snapshot.prev_proc_ticks = current_ticks;

            let mut cpu_heap = MinHeap5::new();
            let mut mem_heap = MinHeap5::new();

            for sample in samples {
                let cpu_usage = if global_delta > 0 {
                    (sample.delta_ticks as f32 / global_delta as f32) * 100.0
                } else {
                    0.0
                };

                let stat = ProcessStat {
                    pid: sample.pid,
                    cpu_usage,
                    memory_bytes: sample.memory_bytes,
                    comm: sample.comm,
                };

                cpu_heap.push(HeapEntry::new(sample.delta_ticks, stat));
                mem_heap.push(HeapEntry::new(sample.memory_bytes, stat));
            }

            out.top_cpu = cpu_heap.as_desc_array();
            out.top_mem = mem_heap.as_desc_array();

            return Ok(out);
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
fn c_char_bytes(buf: &[libc::c_char]) -> &[u8] {
    let bytes = unsafe { std::slice::from_raw_parts(buf.as_ptr() as *const u8, buf.len()) };
    let len = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    &bytes[..len]
}

#[cfg(target_os = "macos")]
fn mach_timebase_ratio() -> (u64, u64) {
    static TIMEBASE: OnceLock<(u64, u64)> = OnceLock::new();

    *TIMEBASE.get_or_init(|| {
        let mut info = libc::mach_timebase_info_data_t { numer: 0, denom: 0 };
        let ret = unsafe { mach_timebase_info(&mut info) };
        if ret == KERN_SUCCESS && info.numer > 0 && info.denom > 0 {
            (u64::from(info.numer), u64::from(info.denom))
        } else {
            (1, 1)
        }
    })
}

#[cfg(target_os = "macos")]
fn mach_absolute_to_ns(ticks: u64) -> u64 {
    let (numer, denom) = mach_timebase_ratio();
    ticks.saturating_mul(numer) / denom
}

#[cfg(target_os = "macos")]
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
pub fn cache_os_fingerprint(meta: &mut MetaStats) -> AuraResult<()> {
    use std::process::Command;

    let mut os = OsFingerprint {
        os_type: FixedString16::from_bytes(b"darwin"),
        os_id: FixedString16::new(),
        os_version_id: FixedString16::new(),
        os_pretty_name: [0; 128],
    };

    if let Ok(output) = Command::new("sw_vers").output() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        for line in stdout.lines() {
            if let Some((key, value)) = line.split_once(':') {
                let key = key.trim();
                let value = value.trim();
                match key {
                    "ProductVersion" => {
                        os.os_version_id = FixedString16::from_bytes(value.as_bytes());
                    }
                    "ProductName" => {
                        let n = value.len().min(128);
                        os.os_pretty_name[..n].copy_from_slice(&value.as_bytes()[..n]);
                    }
                    "BuildVersion" => {
                        os.os_id = FixedString16::from_bytes(value.as_bytes());
                    }
                    _ => {}
                }
            }
        }
    }

    meta.os = os;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{MacosPlatform, PlatformStatsProvider};

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

    #[test]
    #[cfg(target_os = "macos")]
    fn init_then_provider_returns_same_instance() {
        let p1 = crate::platform::macos::init().expect("init should succeed");
        let p2 = crate::platform::macos::provider().expect("provider should return Ok after init");

        assert!(
            std::ptr::eq(p1 as *const _, p2 as *const _),
            "provider() should return the same instance initialized by init()"
        );
    }
}
