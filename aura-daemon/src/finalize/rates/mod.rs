use aura_common::MIN_DELTA_NS;

use crate::collectors::FixedCollectorState;

mod core_clamp;
mod cpu;
mod derived;
mod memory;
mod network;
mod storage;
mod tone;

pub(super) use storage::zero_unowned as zero_unowned_storage;

pub(super) fn apply(state: &mut FixedCollectorState, now: u64) {
    let previous_time = state.baselines.prev_timestamp_ns;
    let elapsed = elapsed_seconds(previous_time, now);
    let warmed = previous_time != 0;

    cpu::finalize(state, elapsed, warmed);
    memory::finalize(state, elapsed, warmed);
    network::finalize(state, elapsed, warmed);
    storage::finalize(state, elapsed, warmed);
    derived::finalize(state);
    tone::finalize(state);
    core_clamp::finalize(state);

    state.baselines.prev_timestamp_ns = now;
}

pub(super) fn elapsed_seconds(previous: u64, now: u64) -> f64 {
    let delta = now.saturating_sub(previous);
    delta.max(MIN_DELTA_NS) as f64 / 1_000_000_000.0
}
