use std::collections::HashMap;

use aura_common::{AuraResult, FixedString16, ProcessStat, ProcessStats, MAX_TOP_N};

use crate::collectors::heap::{zero_process, HeapEntry, MinHeap5};

use super::ffi::{proc_listallpids, proc_name, proc_pidinfo, PROC_PIDTASKINFO, PROC_PIDTBSDINFO};
use super::MacosPlatform;

const SIDL: u32 = 1;
const SRUN: u32 = 2;
const SSLEEP: u32 = 3;
const SSTOP: u32 = 4;

#[derive(Debug, Default)]
pub(super) struct ProcessSnapshot {
    prev_proc_ticks: HashMap<u32, u64>,
    prev_total_ticks: u64,
}

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
    pbi_name: [libc::c_char; 34],
    pbi_nfiles: u32,
    pbi_pgid: u32,
    pbi_pjobc: u32,
    e_tdev: u32,
    e_tpgid: u32,
    pbi_nice: i32,
    pbi_start_tvsec: u64,
    pbi_start_tvusec: u64,
}

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

#[derive(Clone, Copy)]
struct ProcSample {
    pid: u32,
    comm: FixedString16,
    memory_bytes: u64,
    delta_ticks: u64,
}

pub(super) fn collect(platform: &MacosPlatform) -> AuraResult<ProcessStats> {
    let mut out = ProcessStats {
        total: 0,
        running: 0,
        blocked: 0,
        sleeping: 0,
        top_cpu: [zero_process(); MAX_TOP_N],
        top_mem: [zero_process(); MAX_TOP_N],
        top_cpu_count: 0,
        top_mem_count: 0,
        flags: 0,
        _pad0: [0; 5],
    };
    let pid_cap = 4096usize;
    let mut pids = vec![0u32; pid_cap];
    // SAFETY: `pids` is writable for `pid_cap * size_of::<u32>()` bytes, matching the supplied buffer size.
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
    let mut snapshot = match platform.process_snapshot.lock() {
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
        // SAFETY: `taskinfo` is writable storage and `taskinfo_size` exactly matches `ProcTaskInfo`; return size is checked.
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
        // SAFETY: `bsdinfo` is writable storage and `bsdinfo_size` exactly matches `ProcBsdInfo`; return size is checked.
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
        // SAFETY: `name_buf` is writable for its full length and the returned byte count is validated before slicing.
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
    Ok(out)
}

fn c_char_bytes(buf: &[libc::c_char]) -> &[u8] {
    // SAFETY: `u8` has alignment 1 and the byte slice covers the same initialized `c_char` buffer length.
    let bytes = unsafe { std::slice::from_raw_parts(buf.as_ptr() as *const u8, buf.len()) };
    let len = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    &bytes[..len]
}
