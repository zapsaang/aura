//! Allocation-stable process enumeration and aggregation.
//!
//! Per-record failures set the truncation flag and skip the record, while
//! enumeration failures are Fatal and commit nothing.

use aura_common::{AuraError, AuraResult, ProcessStat, ProcessStats, MAX_TOP_N, PROCESS_TRUNCATED};

use crate::collectors::heap::{HeapEntry, MinHeap5};

use super::procfs::{
    parse_dirent, parse_pid, read_stat, ProcessDirectory, ProcessScan, RawDir, DIRENT_BUF_LEN,
};
use super::rank;
use crate::collectors::process::state::{
    ProcessBaseSnapshot, ProcessBaseline, ProcessProcStat, PROCESS_BASELINE_CAPACITY,
};

#[derive(Default)]
struct ScanAccum {
    total: u32,
    running: u32,
    blocked: u32,
    sleeping: u32,
    cpu_heap: MinHeap5,
    mem_heap: MinHeap5,
    cpu_candidates: usize,
    mem_candidates: usize,
    truncated: bool,
}

pub fn collect(
    scan: &mut ProcessScan<'_>,
    baseline: &mut ProcessBaseline,
    out: &mut ProcessStats,
) -> AuraResult<()> {
    if scan.page_size == 0 {
        return Err(AuraError::Fatal(
            "process page size not initialized".to_string(),
        ));
    }
    let mut dir = RawDir::open(scan.proc_root, scan.path_buf)?;
    collect_with_directory(scan, baseline, out, &mut dir)
}

pub fn collect_with_directory<D: ProcessDirectory>(
    scan: &mut ProcessScan<'_>,
    baseline: &mut ProcessBaseline,
    out: &mut ProcessStats,
    dir: &mut D,
) -> AuraResult<()> {
    if scan.page_size == 0 {
        return Err(AuraError::Fatal(
            "process page size not initialized".to_string(),
        ));
    }
    *out = crate::collectors::process::zero_stats();
    let cycle_start = baseline.generation();
    let mut accum = ScanAccum::default();
    let mut raw = [0u8; DIRENT_BUF_LEN];

    loop {
        let nread = dir.read(&mut raw)?;
        if nread == 0 {
            break;
        }
        let mut pos = 0usize;
        while pos < nread {
            let (name, reclen) = parse_dirent(&raw, pos, nread)?;
            pos += reclen;
            let Some(pid) = parse_pid(name) else {
                continue;
            };
            handle_pid(scan, baseline, pid, name, &mut accum);
        }
    }

    baseline.sweep_unmarked(cycle_start);
    baseline.rehash_after_cycle();
    rank::publish_cpu(&accum.cpu_heap, out);
    rank::publish_mem(&accum.mem_heap, out);
    out.total = accum.total;
    out.running = accum.running;
    out.blocked = accum.blocked;
    out.sleeping = accum.sleeping;
    if accum.truncated || accum.cpu_candidates > MAX_TOP_N || accum.mem_candidates > MAX_TOP_N {
        out.flags |= PROCESS_TRUNCATED;
    }
    Ok(())
}

fn handle_pid(
    scan: &mut ProcessScan<'_>,
    baseline: &mut ProcessBaseline,
    pid: u64,
    name: &[u8],
    accum: &mut ScanAccum,
) {
    let Some(parsed) = read_stat(scan, name, &mut accum.truncated) else {
        return;
    };
    if u64::from(parsed.pid) != pid || parsed.rss_pages < 0 {
        accum.truncated = true;
        return;
    }
    let Some(memory_bytes) = (parsed.rss_pages as u64).checked_mul(scan.page_size) else {
        accum.truncated = true;
        return;
    };
    accum.total = accum.total.saturating_add(1);
    match parsed.state {
        b'R' => accum.running = accum.running.saturating_add(1),
        b'D' => accum.blocked = accum.blocked.saturating_add(1),
        b'S' | b'I' | b'T' | b't' => accum.sleeping = accum.sleeping.saturating_add(1),
        _ => {}
    }
    track_baseline(scan, baseline, &parsed, memory_bytes, accum);
}

fn track_baseline(
    scan: &ProcessScan<'_>,
    baseline: &mut ProcessBaseline,
    parsed: &ProcessProcStat,
    memory_bytes: u64,
    accum: &mut ScanAccum,
) {
    let key = (parsed.pid, parsed.starttime);
    let (slot, found) = baseline.probe(&key);
    let previous = if found {
        Some(baseline.slots[slot].stat)
    } else {
        None
    };
    if !found && slot >= PROCESS_BASELINE_CAPACITY {
        // Baseline overflow never evicts an active identity; the record still
        // counts and ranks by memory, but no interval state is tracked.
        accum.truncated = true;
    } else {
        baseline.insert(
            key,
            ProcessBaseSnapshot {
                utime: parsed.utime,
                stime: parsed.stime,
            },
        );
    }
    push_cpu_candidate(scan, parsed, memory_bytes, previous, accum);
    accum.mem_heap.push(HeapEntry::new(
        memory_bytes,
        ProcessStat {
            pid: parsed.pid,
            cpu_usage: 0.0,
            memory_bytes,
            comm: parsed.comm,
        },
    ));
    accum.mem_candidates += 1;
}

fn push_cpu_candidate(
    scan: &ProcessScan<'_>,
    parsed: &ProcessProcStat,
    memory_bytes: u64,
    previous: Option<ProcessBaseSnapshot>,
    accum: &mut ScanAccum,
) {
    let Some(prev) = previous else {
        return;
    };
    let prev_total = prev.utime.saturating_add(prev.stime);
    let curr_total = parsed.utime.saturating_add(parsed.stime);
    if curr_total < prev_total {
        return;
    }
    let delta = curr_total - prev_total;
    if delta == 0 || scan.delta_global_ticks == 0 || scan.online_cores == 0 {
        return;
    }
    let usage =
        (100.0 * delta as f64 * scan.online_cores as f64 / scan.delta_global_ticks as f64) as f32;
    accum.cpu_heap.push(HeapEntry::new(
        delta,
        ProcessStat {
            pid: parsed.pid,
            cpu_usage: usage,
            memory_bytes,
            comm: parsed.comm,
        },
    ));
    accum.cpu_candidates += 1;
}
