use aura_common::{CAP_CPU_GLOBAL, CAP_CPU_PER_CORE, MAX_CORES};

use crate::collectors::FixedCollectorState;

use super::cpu;

pub(super) fn finalize(state: &mut FixedCollectorState) {
    let caps = state.archive.capabilities;
    if caps & CAP_CPU_GLOBAL == 0 {
        state.archive.cpu.core_count = 0;
        cpu::clear_when_no_per_core(&mut state.archive.cpu);
        return;
    }
    if state.archive.cpu.core_count as usize > MAX_CORES {
        cpu::clear_over_cap(state);
    } else if caps & CAP_CPU_PER_CORE == 0 {
        cpu::clear_when_no_per_core(&mut state.archive.cpu);
    }
}
