use aura_common::{
    TelemetryArchive, CAPABILITY_COUNT, CAP_CPU_CONTEXT_SWITCHES, CAP_CPU_GLOBAL, CAP_CPU_PER_CORE,
    CAP_MEMORY_BUFFERS, CAP_MEMORY_CACHED, CAP_MEMORY_PAGE_FAULTS, CAP_MEMORY_RAM_FREE,
    CAP_MEMORY_RAM_TOTAL, CAP_MEMORY_RAM_USED, CAP_MEMORY_SWAP, CAP_PROCESS_BLOCKED,
    CAP_PROCESS_RUNNING, CAP_PROCESS_SLEEPING, CAP_PROCESS_TOP_CPU, CAP_PROCESS_TOP_MEMORY,
    CAP_PROCESS_TOTAL, CAP_STORAGE_DISK_BYTES, CAP_STORAGE_DISK_IOPS, CAP_STORAGE_DISK_LATENCY,
    CAP_STORAGE_DISK_QUEUE_DEPTH, CAP_STORAGE_DISK_RATES, PROCESS_TRUNCATED,
};

use super::schema::*;

pub(super) const DISK_CAPS: u64 = CAP_STORAGE_DISK_BYTES
    | CAP_STORAGE_DISK_RATES
    | CAP_STORAGE_DISK_IOPS
    | CAP_STORAGE_DISK_QUEUE_DEPTH
    | CAP_STORAGE_DISK_LATENCY;
const PROCESS_CAPS: u64 = CAP_PROCESS_TOTAL
    | CAP_PROCESS_RUNNING
    | CAP_PROCESS_BLOCKED
    | CAP_PROCESS_SLEEPING
    | CAP_PROCESS_TOP_CPU
    | CAP_PROCESS_TOP_MEMORY;

pub(super) fn opt<T>(owned: bool, value: T) -> Option<T> {
    owned.then_some(value)
}

pub(super) fn capabilities(caps: u64) -> CapabilitiesJson {
    let mut fields = [false; CAPABILITY_COUNT as usize];
    for (bit, slot) in fields.iter_mut().enumerate() {
        *slot = caps & (1u64 << bit) != 0;
    }
    let f = fields;
    CapabilitiesJson {
        cpu_global: f[0],
        cpu_per_core: f[1],
        cpu_context_switches: f[2],
        process_total: f[3],
        process_running: f[4],
        process_blocked: f[5],
        process_sleeping: f[6],
        process_top_cpu: f[7],
        process_top_memory: f[8],
        memory_ram_total: f[9],
        memory_ram_free: f[10],
        memory_ram_used: f[11],
        memory_buffers: f[12],
        memory_cached: f[13],
        memory_swap: f[14],
        memory_page_faults: f[15],
        storage_disk_bytes: f[16],
        storage_disk_rates: f[17],
        storage_disk_iops: f[18],
        storage_disk_queue_depth: f[19],
        storage_disk_latency: f[20],
        storage_mounts: f[21],
        network_bytes: f[22],
        network_rates: f[23],
        meta_uptime: f[24],
        meta_load_average: f[25],
        meta_timezone: f[26],
        meta_os_identity: f[27],
        meta_os_version: f[28],
        meta_os_version_id: f[29],
        meta_os_codename: f[30],
        meta_wallclock: f[31],
        gpu_enumeration: f[32],
    }
}

pub(super) fn cpu_json(t: &TelemetryArchive) -> CpuJson {
    let caps = t.capabilities;
    let c = &t.cpu;
    CpuJson {
        user_ticks: opt(caps & CAP_CPU_GLOBAL != 0, c.user_ticks),
        system_ticks: opt(caps & CAP_CPU_GLOBAL != 0, c.system_ticks),
        idle_ticks: opt(caps & CAP_CPU_GLOBAL != 0, c.idle_ticks),
        total_ticks: opt(caps & CAP_CPU_GLOBAL != 0, c.total_ticks),
        context_switches: opt(caps & CAP_CPU_CONTEXT_SWITCHES != 0, c.context_switches),
        context_switches_per_sec: opt(
            caps & CAP_CPU_CONTEXT_SWITCHES != 0,
            c.context_switches_per_sec,
        ),
        usage_percent: opt(caps & CAP_CPU_GLOBAL != 0, c.usage_percent),
        cores: opt(
            caps & CAP_CPU_PER_CORE != 0,
            (0..c.core_count as usize)
                .map(|i| {
                    let r = &c.cores[i];
                    CpuCoreJson {
                        core_index: r.core_index,
                        user_ticks: r.user_ticks,
                        system_ticks: r.system_ticks,
                        idle_ticks: r.idle_ticks,
                        total_ticks: r.total_ticks,
                        usage_percent: r.usage_percent,
                    }
                })
                .collect(),
        ),
    }
}

pub(super) fn process_json(t: &TelemetryArchive) -> ProcessJson {
    let caps = t.capabilities;
    let p = &t.process;
    ProcessJson {
        total: opt(caps & CAP_PROCESS_TOTAL != 0, p.total),
        running: opt(caps & CAP_PROCESS_RUNNING != 0, p.running),
        blocked: opt(caps & CAP_PROCESS_BLOCKED != 0, p.blocked),
        sleeping: opt(caps & CAP_PROCESS_SLEEPING != 0, p.sleeping),
        truncated: opt(caps & PROCESS_CAPS != 0, p.flags & PROCESS_TRUNCATED != 0),
        top_cpu: opt(
            caps & CAP_PROCESS_TOP_CPU != 0,
            (0..p.top_cpu_count as usize)
                .map(|i| {
                    let e = &p.top_cpu[i];
                    TopCpuJson {
                        pid: e.pid,
                        cpu_usage: e.cpu_usage,
                        memory_bytes: e.memory_bytes,
                        comm: e.comm.as_str().to_string(),
                    }
                })
                .collect(),
        ),
        top_memory: opt(
            caps & CAP_PROCESS_TOP_MEMORY != 0,
            (0..p.top_mem_count as usize)
                .map(|i| {
                    let e = &p.top_mem[i];
                    TopMemoryJson {
                        pid: e.pid,
                        memory_bytes: e.memory_bytes,
                        comm: e.comm.as_str().to_string(),
                    }
                })
                .collect(),
        ),
    }
}

pub(super) fn memory_json(t: &TelemetryArchive) -> MemoryJson {
    let caps = t.capabilities;
    let m = &t.memory;
    let ram_pct = caps & CAP_MEMORY_RAM_TOTAL != 0 && caps & CAP_MEMORY_RAM_USED != 0;
    let swap = caps & CAP_MEMORY_SWAP != 0;
    MemoryJson {
        ram_total: opt(caps & CAP_MEMORY_RAM_TOTAL != 0, m.ram_total),
        ram_free: opt(caps & CAP_MEMORY_RAM_FREE != 0, m.ram_free),
        ram_used: opt(caps & CAP_MEMORY_RAM_USED != 0, m.ram_used),
        ram_used_percent: opt(ram_pct, t.derived.ram_used_percent),
        buffers: opt(caps & CAP_MEMORY_BUFFERS != 0, m.buffers),
        cached: opt(caps & CAP_MEMORY_CACHED != 0, m.cached),
        swap_total: opt(swap, m.swap_total),
        swap_free: opt(swap, m.swap_free),
        swap_used: opt(swap, m.swap_used),
        swap_used_percent: opt(swap, t.derived.swap_used_percent),
        page_faults: opt(caps & CAP_MEMORY_PAGE_FAULTS != 0, m.page_faults),
        page_faults_per_sec: opt(caps & CAP_MEMORY_PAGE_FAULTS != 0, m.page_faults_per_sec),
    }
}
