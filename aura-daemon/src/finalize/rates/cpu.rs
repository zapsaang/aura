use aura_common::{
    CpuCoreStat, CAP_CPU_CONTEXT_SWITCHES, CAP_CPU_GLOBAL, CAP_CPU_PER_CORE, CAP_PROCESS_TOP_CPU,
    MAX_CORES,
};

use crate::collectors::{CpuCoreSnapshot, FixedCollectorState};

pub(super) fn finalize(state: &mut FixedCollectorState, elapsed: f64, warmed: bool) {
    let caps = state.archive.capabilities;
    if caps & CAP_CPU_GLOBAL == 0 {
        state.baselines.cpu_ticks = Default::default();
        for core in &mut state.baselines.cores {
            *core = CpuCoreSnapshot::zero();
        }
        state.baselines.core_count = 0;
        return;
    }

    let cpu = &mut state.archive.cpu;
    let previous = state.baselines.cpu_ticks;
    let represented_now = (cpu.core_count as usize).min(MAX_CORES);
    let represented_prev = (state.baselines.core_count as usize).min(MAX_CORES);
    let hotplug = represented_now != represented_prev;

    let delta_total = cpu.total_ticks.saturating_sub(previous.total);
    let delta_idle = cpu.idle_ticks.saturating_sub(previous.idle);
    let aggregate_decreased = cpu.total_ticks < previous.total;
    cpu.usage_percent = if warmed && !hotplug && !aggregate_decreased && delta_total != 0 {
        let busy = delta_total.saturating_sub(delta_idle);
        100.0 * busy as f32 / delta_total as f32
    } else {
        0.0
    };

    cpu.context_switches_per_sec =
        if warmed && caps & CAP_CPU_CONTEXT_SWITCHES != 0 && !hotplug && !aggregate_decreased {
            let delta = cpu
                .context_switches
                .saturating_sub(previous.context_switches);
            delta as f32 / elapsed as f32
        } else {
            0.0
        };

    for index in 0..represented_now {
        let core = &mut cpu.cores[index];
        let prev = &state.baselines.cores[index];
        let core_decreased = core.total_ticks < prev.total;
        core.usage_percent = if warmed && !hotplug && !core_decreased && prev.total != 0 {
            let delta_total_core = core.total_ticks.saturating_sub(prev.total);
            let delta_idle_core = core.idle_ticks.saturating_sub(prev.idle);
            if delta_total_core != 0 {
                let busy = delta_total_core.saturating_sub(delta_idle_core);
                100.0 * busy as f32 / delta_total_core as f32
            } else {
                0.0
            }
        } else {
            0.0
        };
    }

    state.baselines.cpu_ticks.user = cpu.user_ticks;
    state.baselines.cpu_ticks.system = cpu.system_ticks;
    state.baselines.cpu_ticks.idle = cpu.idle_ticks;
    state.baselines.cpu_ticks.total = cpu.total_ticks;
    state.baselines.cpu_ticks.context_switches = cpu.context_switches;
    for index in 0..represented_now {
        state.baselines.cores[index] = CpuCoreSnapshot {
            user: cpu.cores[index].user_ticks,
            system: cpu.cores[index].system_ticks,
            idle: cpu.cores[index].idle_ticks,
            total: cpu.cores[index].total_ticks,
        };
    }
    state.baselines.core_count = represented_now as u8;
}

pub(super) fn zero_core() -> CpuCoreStat {
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

pub(super) fn zero_process() -> aura_common::ProcessStat {
    aura_common::ProcessStat {
        pid: 0,
        cpu_usage: 0.0,
        memory_bytes: 0,
        comm: aura_common::FixedString16::new(),
    }
}

pub(super) fn clear_over_cap(state: &mut FixedCollectorState) {
    let cpu = &mut state.archive.cpu;
    let caps = state.archive.capabilities;
    cpu.core_count = 0;
    for core in &mut cpu.cores {
        *core = zero_core();
    }
    state.archive.capabilities &= !CAP_CPU_PER_CORE;
    if caps & CAP_PROCESS_TOP_CPU != 0 {
        state.archive.process.top_cpu_count = 0;
        for entry in &mut state.archive.process.top_cpu {
            *entry = zero_process();
        }
    }
}

pub(super) fn clear_when_no_per_core(cpu: &mut aura_common::CpuGlobalStat) {
    for core in &mut cpu.cores {
        *core = zero_core();
    }
}
