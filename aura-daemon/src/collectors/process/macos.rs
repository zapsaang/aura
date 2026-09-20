use aura_common::{ProcessStats, MAX_TOP_N};

use crate::collectors::heap::zero_process;

pub fn collect() -> ProcessStats {
    ProcessStats {
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
    }
}
