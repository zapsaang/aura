use aura_common::{
    CAP_CPU_CONTEXT_SWITCHES, CAP_CPU_GLOBAL, CAP_MEMORY_PAGE_FAULTS, CAP_NETWORK_BYTES,
    CAP_NETWORK_RATES, MIN_DELTA_NS,
};

use crate::collectors::FixedCollectorState;

pub(super) fn apply(state: &mut FixedCollectorState, now: u64) {
    let previous_time = state.baselines.prev_timestamp_ns;
    let elapsed = elapsed_seconds(previous_time, now);
    finalize_cpu(state, elapsed, previous_time != 0);
    finalize_memory(state, elapsed, previous_time != 0);
    finalize_network(state, elapsed, previous_time != 0);
    state.baselines.prev_timestamp_ns = now;
}

fn finalize_cpu(state: &mut FixedCollectorState, elapsed: f64, warmed: bool) {
    let caps = state.archive.capabilities;
    if caps & CAP_CPU_GLOBAL == 0 {
        state.baselines.cpu_ticks = Default::default();
        return;
    }
    let cpu = &mut state.archive.cpu;
    let previous = state.baselines.cpu_ticks;
    let delta_total = cpu.total_ticks.saturating_sub(previous.total);
    let delta_idle = cpu.idle_ticks.saturating_sub(previous.idle);
    cpu.usage_percent = if warmed && delta_total != 0 {
        100.0 * delta_total.saturating_sub(delta_idle) as f32 / delta_total as f32
    } else {
        0.0
    };
    cpu.context_switches_per_sec = if warmed && caps & CAP_CPU_CONTEXT_SWITCHES != 0 {
        cpu.context_switches
            .saturating_sub(previous.context_switches) as f32
            / elapsed as f32
    } else {
        0.0
    };
    state.baselines.cpu_ticks.user = cpu.user_ticks;
    state.baselines.cpu_ticks.system = cpu.system_ticks;
    state.baselines.cpu_ticks.idle = cpu.idle_ticks;
    state.baselines.cpu_ticks.total = cpu.total_ticks;
    state.baselines.cpu_ticks.context_switches = cpu.context_switches;
}

fn finalize_memory(state: &mut FixedCollectorState, elapsed: f64, warmed: bool) {
    if state.archive.capabilities & CAP_MEMORY_PAGE_FAULTS == 0 {
        state.baselines.prev_page_faults = 0;
        return;
    }
    let current = state.archive.memory.page_faults;
    let previous = state.baselines.prev_page_faults;
    state.archive.memory.page_faults_per_sec = if warmed {
        current.saturating_sub(previous) as f32 / elapsed as f32
    } else {
        0.0
    };
    state.baselines.prev_page_faults = current;
}

fn finalize_network(state: &mut FixedCollectorState, elapsed: f64, warmed: bool) {
    if state.archive.capabilities & CAP_NETWORK_BYTES == 0 {
        state.baselines.net_bytes = Default::default();
        return;
    }
    let count =
        (state.archive.network.if_count as usize).min(state.archive.network.interfaces.len());
    let previous_count = state.baselines.net_bytes.count;
    for index in 0..count {
        let interface = &mut state.archive.network.interfaces[index];
        let (previous_rx, previous_tx) = state.baselines.net_bytes.interfaces[index];
        if warmed && index < previous_count && state.archive.capabilities & CAP_NETWORK_RATES != 0 {
            interface.rx_bytes_per_sec =
                interface.rx_bytes.saturating_sub(previous_rx) as f32 / elapsed as f32;
            interface.tx_bytes_per_sec =
                interface.tx_bytes.saturating_sub(previous_tx) as f32 / elapsed as f32;
        } else {
            interface.rx_bytes_per_sec = 0.0;
            interface.tx_bytes_per_sec = 0.0;
        }
        state.baselines.net_bytes.interfaces[index] = (interface.rx_bytes, interface.tx_bytes);
    }
    for baseline in &mut state.baselines.net_bytes.interfaces[count..] {
        *baseline = (0, 0);
    }
    state.baselines.net_bytes.count = count;
}

fn elapsed_seconds(previous: u64, now: u64) -> f64 {
    now.saturating_sub(previous).max(MIN_DELTA_NS) as f64 / 1_000_000_000.0
}
