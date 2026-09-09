//! Publishes fixed heaps into the archive with deterministic ordering:
//! metric-descending, PID-ascending on ties.

use aura_common::{ProcessStats, MAX_TOP_N};

use crate::collectors::heap::MinHeap5;

pub(super) fn publish_cpu(heap: &MinHeap5, out: &mut ProcessStats) {
    let mut entries = heap.as_desc_array();
    let count = heap.len().min(MAX_TOP_N);
    entries[..count].sort_unstable_by(|a, b| {
        b.cpu_usage
            .total_cmp(&a.cpu_usage)
            .then_with(|| a.pid.cmp(&b.pid))
    });
    out.top_cpu = entries;
    out.top_cpu_count = count as u8;
}

pub(super) fn publish_mem(heap: &MinHeap5, out: &mut ProcessStats) {
    let mut entries = heap.as_desc_array();
    let count = heap.len().min(MAX_TOP_N);
    entries[..count].sort_unstable_by(|a, b| {
        b.memory_bytes
            .cmp(&a.memory_bytes)
            .then_with(|| a.pid.cmp(&b.pid))
    });
    out.top_mem = entries;
    out.top_mem_count = count as u8;
}
