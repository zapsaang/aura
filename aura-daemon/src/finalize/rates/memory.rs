use aura_common::{
    CAP_MEMORY_PAGE_FAULTS, CAP_MEMORY_RAM_TOTAL, CAP_MEMORY_RAM_USED, CAP_MEMORY_SWAP,
};

use crate::collectors::FixedCollectorState;

pub(super) fn finalize(state: &mut FixedCollectorState, elapsed: f64, warmed: bool) {
    let caps = state.archive.capabilities;
    let derived = &mut state.archive.derived;

    let ram_prereq = caps & CAP_MEMORY_RAM_TOTAL != 0 && caps & CAP_MEMORY_RAM_USED != 0;
    if ram_prereq {
        let total = state.archive.memory.ram_total;
        let used = state.archive.memory.ram_used;
        if total > 0 && used <= total {
            derived.ram_used_percent = 100.0 * used as f32 / total as f32;
        } else {
            derived.ram_used_percent = 0.0;
        }
    } else {
        derived.ram_used_percent = 0.0;
    }

    let swap_prereq = caps & CAP_MEMORY_SWAP != 0;
    if swap_prereq {
        let total = state.archive.memory.swap_total;
        let used = state.archive.memory.swap_used;
        if total > 0 && used <= total {
            derived.swap_used_percent = 100.0 * used as f32 / total as f32;
        } else {
            derived.swap_used_percent = 0.0;
        }
    } else {
        derived.swap_used_percent = 0.0;
    }

    if caps & CAP_MEMORY_PAGE_FAULTS == 0 {
        state.baselines.prev_page_faults = 0;
        state.archive.memory.page_faults_per_sec = 0.0;
    } else if warmed {
        let current = state.archive.memory.page_faults;
        let previous = state.baselines.prev_page_faults;
        if current >= previous {
            state.archive.memory.page_faults_per_sec =
                current.saturating_sub(previous) as f32 / elapsed as f32;
        } else {
            state.archive.memory.page_faults_per_sec = 0.0;
        }
        state.baselines.prev_page_faults = current;
    } else {
        state.archive.memory.page_faults_per_sec = 0.0;
        state.baselines.prev_page_faults = state.archive.memory.page_faults;
    }
}
