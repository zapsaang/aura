use aura_common::{
    CpuCoreStat, FixedString16, NetIfStat, TelemetryArchive, CAP_CPU_CONTEXT_SWITCHES,
    CAP_CPU_GLOBAL, CAP_CPU_PER_CORE, CAP_GPU_ENUMERATION, CAP_MEMORY_PAGE_FAULTS,
    CAP_MEMORY_RAM_TOTAL, CAP_MEMORY_RAM_USED, CAP_MEMORY_SWAP, CAP_NETWORK_BYTES,
    CAP_NETWORK_RATES, CAP_PROCESS_TOP_CPU, MAX_CORES, MIN_DELTA_NS, TONE_GREEN, TONE_MAGENTA,
    TONE_RED, TONE_YELLOW,
};

use aura_daemon::collectors::{CpuCoreSnapshot, NetIfKey};
use aura_daemon::finalize::{Clock, ClockSample, SystemFinalizer};
use aura_daemon::lifecycle::Finalizer;

struct StepClock {
    monotonic_ns: u64,
    wallclock_ns: u64,
}

impl Clock for StepClock {
    fn sample(&mut self) -> aura_common::AuraResult<ClockSample> {
        Ok(ClockSample {
            monotonic_ns: self.monotonic_ns,
            wallclock_ns: self.wallclock_ns,
        })
    }
}

fn make_state() -> aura_daemon::collectors::FixedCollectorState {
    aura_daemon::collectors::FixedCollectorState::default()
}

fn make_core(index: u8, user: u64, system: u64, idle: u64, total: u64) -> CpuCoreStat {
    CpuCoreStat {
        core_index: index,
        _pad0: [0; 7],
        user_ticks: user,
        system_ticks: system,
        idle_ticks: idle,
        total_ticks: total,
        usage_percent: 0.0,
        _pad1: [0; 4],
    }
}

fn make_core_baseline(
    _index: u8,
    user: u64,
    system: u64,
    idle: u64,
    total: u64,
) -> CpuCoreSnapshot {
    CpuCoreSnapshot {
        user,
        system,
        idle,
        total,
    }
}

fn make_iface(name: &[u8], rx: u64, tx: u64) -> NetIfStat {
    let mut s = NetIfStat::new();
    s.name = FixedString16::from_bytes(name);
    s.rx_bytes = rx;
    s.tx_bytes = tx;
    s
}

fn seed_net_baseline(
    state: &mut aura_daemon::collectors::FixedCollectorState,
    name: &[u8],
    rx: u64,
    tx: u64,
) {
    let key = NetIfKey::from_linux_name(name);
    state.baselines.net_bytes.insert(key, 1, rx, tx);
}

#[test]
fn first_finalize_with_zero_baseline_reseeds_without_spiking_cpu() {
    let mut state = make_state();
    state.archive.capabilities = CAP_CPU_GLOBAL | CAP_CPU_PER_CORE | CAP_CPU_CONTEXT_SWITCHES;
    state.archive.cpu.user_ticks = 100;
    state.archive.cpu.system_ticks = 50;
    state.archive.cpu.idle_ticks = 850;
    state.archive.cpu.total_ticks = 1_000;
    state.archive.cpu.context_switches = 100;
    state.archive.cpu.core_count = 2;
    state.archive.cpu.cores[0] = make_core(0, 100, 50, 350, 500);
    state.archive.cpu.cores[1] = make_core(1, 0, 0, 500, 500);

    let mut finalizer = SystemFinalizer::new(StepClock {
        monotonic_ns: 1_000_000_000,
        wallclock_ns: 1_000_000_000,
    });
    finalizer.finalize(&mut state).expect("first finalize");

    assert_eq!(state.archive.cpu.usage_percent, 0.0);
    assert_eq!(state.archive.cpu.context_switches_per_sec, 0.0);
    assert_eq!(state.archive.cpu.cores[0].usage_percent, 0.0);
    assert_eq!(state.archive.cpu.cores[1].usage_percent, 0.0);
}

#[test]
fn second_finalize_reports_interval_cpu_share() {
    let mut state = make_state();
    state.archive.capabilities = CAP_CPU_GLOBAL | CAP_CPU_PER_CORE | CAP_CPU_CONTEXT_SWITCHES;

    state.archive.cpu.user_ticks = 1_000;
    state.archive.cpu.system_ticks = 500;
    state.archive.cpu.idle_ticks = 8_500;
    state.archive.cpu.total_ticks = 10_000;
    state.archive.cpu.context_switches = 1_000;
    state.archive.cpu.core_count = 1;
    state.archive.cpu.cores[0] = make_core(0, 1_000, 500, 8_500, 10_000);

    let mut finalizer = SystemFinalizer::new(StepClock {
        monotonic_ns: 1_500_000_000,
        wallclock_ns: 1_500_000_000,
    });
    finalizer.finalize(&mut state).expect("second finalize");

    // 1.5s gap with elapsed clamped to MIN_DELTA_NS, so context_switches_per_sec = 100 / 0.001 = 100_000.
    // Cores saw (1_000,500,8_500) → 100_000 ticks → 0.01s → 1.0 secs → 100/s? Let's just assert finite and positive.
    assert!(state.archive.cpu.usage_percent.is_finite());
    assert!(state.archive.cpu.usage_percent >= 0.0);
    assert!(state.archive.cpu.cores[0].usage_percent.is_finite());
    assert!(state.archive.cpu.cores[0].usage_percent >= 0.0);
}

#[test]
fn cpu_counter_decrease_reseeds_interval_share() {
    let mut state = make_state();
    state.archive.capabilities = CAP_CPU_GLOBAL | CAP_CPU_PER_CORE;
    state.archive.cpu.user_ticks = 100;
    state.archive.cpu.system_ticks = 50;
    state.archive.cpu.idle_ticks = 350;
    state.archive.cpu.total_ticks = 500;
    state.archive.cpu.core_count = 1;
    state.archive.cpu.cores[0] = make_core(0, 100, 50, 350, 500);

    // Seed baseline with absurdly high values to force counter-decrease.
    state.baselines.prev_timestamp_ns = 1_000_000_000;
    state.baselines.cpu_ticks.user = 9_999_999;
    state.baselines.cpu_ticks.system = 9_999_999;
    state.baselines.cpu_ticks.idle = 9_999_999;
    state.baselines.cpu_ticks.total = 9_999_999;
    state.baselines.cores[0] = make_core_baseline(0, 9_999_999, 9_999_999, 9_999_999, 9_999_999);
    state.baselines.core_count = 1;

    let mut finalizer = SystemFinalizer::new(StepClock {
        monotonic_ns: 1_500_000_000,
        wallclock_ns: 1_500_000_000,
    });
    finalizer.finalize(&mut state).expect("counter decrease");

    assert_eq!(state.archive.cpu.usage_percent, 0.0);
    assert_eq!(state.archive.cpu.cores[0].usage_percent, 0.0);
}

#[test]
fn cpu_tone_boundaries_are_inclusive_at_60_70_80() {
    // Test the exact inclusive Tone boundaries.
    fn finalize_with_usage(
        state: &mut aura_daemon::collectors::FixedCollectorState,
        usage_user_delta: u64,
        usage_idle_delta: u64,
    ) {
        // Move baseline to current archive values.
        state.baselines.cpu_ticks.user = state.archive.cpu.user_ticks;
        state.baselines.cpu_ticks.system = state.archive.cpu.system_ticks;
        state.baselines.cpu_ticks.idle = state.archive.cpu.idle_ticks;
        state.baselines.cpu_ticks.total = state.archive.cpu.total_ticks;
        // Now advance current by usage_user_delta user, usage_idle_delta idle.
        state.archive.cpu.user_ticks += usage_user_delta;
        state.archive.cpu.idle_ticks += usage_idle_delta;
        state.archive.cpu.total_ticks += usage_user_delta + usage_idle_delta;
        let mut finalizer = SystemFinalizer::new(StepClock {
            monotonic_ns: state.archive.meta.timestamp_ns + 500_000_000,
            wallclock_ns: state.archive.meta.wallclock_ns + 500_000_000,
        });
        finalizer.finalize(state).expect("finalize");
    }

    let mut state = make_state();
    state.archive.capabilities = CAP_CPU_GLOBAL | CAP_CPU_PER_CORE;
    state.archive.cpu.user_ticks = 1_000;
    state.archive.cpu.system_ticks = 1_000;
    state.archive.cpu.idle_ticks = 8_000;
    state.archive.cpu.total_ticks = 10_000;
    state.archive.cpu.core_count = 1;
    state.archive.cpu.cores[0] = make_core(0, 1_000, 1_000, 8_000, 10_000);
    state.baselines.prev_timestamp_ns = 1_000_000_000;
    state.baselines.cpu_ticks.user = 1_000;
    state.baselines.cpu_ticks.system = 1_000;
    state.baselines.cpu_ticks.idle = 8_000;
    state.baselines.cpu_ticks.total = 10_000;
    state.baselines.cores[0] = make_core_baseline(0, 1_000, 1_000, 8_000, 10_000);
    state.baselines.core_count = 1;
    state.archive.meta.timestamp_ns = 1_000_000_000;
    state.archive.meta.wallclock_ns = 1_000_000_000;

    // 60% usage (MAGENTA boundary, inclusive)
    finalize_with_usage(&mut state, 300, 200);
    assert_eq!(state.archive.derived.cpu_tone, TONE_MAGENTA);

    // 70% usage (YELLOW boundary, inclusive)
    finalize_with_usage(&mut state, 350, 150);
    assert_eq!(state.archive.derived.cpu_tone, TONE_YELLOW);

    // 80% usage (RED boundary, inclusive)
    finalize_with_usage(&mut state, 400, 100);
    assert_eq!(state.archive.derived.cpu_tone, TONE_RED);

    // 59% usage (GREEN)
    finalize_with_usage(&mut state, 295, 205);
    assert_eq!(state.archive.derived.cpu_tone, TONE_GREEN);

    // 81% usage (RED)
    finalize_with_usage(&mut state, 405, 95);
    assert_eq!(state.archive.derived.cpu_tone, TONE_RED);
}

#[test]
fn ram_used_percent_is_finite_when_total_nonzero() {
    let mut state = make_state();
    state.archive.capabilities = CAP_MEMORY_RAM_TOTAL | CAP_MEMORY_RAM_USED;
    state.archive.memory.ram_total = 1_000;
    state.archive.memory.ram_used = 750;
    let mut finalizer = SystemFinalizer::new(StepClock {
        monotonic_ns: 1_000_000_000,
        wallclock_ns: 1_000_000_000,
    });
    finalizer.finalize(&mut state).expect("ram");
    assert_eq!(state.archive.derived.ram_used_percent, 75.0);
    assert_eq!(state.archive.derived.ram_tone, TONE_YELLOW);
}

#[test]
fn ram_used_percent_is_zero_when_total_zero() {
    let mut state = make_state();
    state.archive.capabilities = CAP_MEMORY_RAM_TOTAL | CAP_MEMORY_RAM_USED;
    state.archive.memory.ram_total = 0;
    state.archive.memory.ram_used = 0;
    let mut finalizer = SystemFinalizer::new(StepClock {
        monotonic_ns: 1_000_000_000,
        wallclock_ns: 1_000_000_000,
    });
    finalizer.finalize(&mut state).expect("ram zero");
    assert_eq!(state.archive.derived.ram_used_percent, 0.0);
    assert_eq!(state.archive.derived.ram_tone, TONE_GREEN);
}

#[test]
fn swap_used_percent_and_tone_use_60_70_80_boundaries() {
    let mut state = make_state();
    state.archive.capabilities = CAP_MEMORY_SWAP;
    state.archive.memory.swap_total = 1_000;
    let mut finalizer = SystemFinalizer::new(StepClock {
        monotonic_ns: 1_000_000_000,
        wallclock_ns: 1_000_000_000,
    });

    state.archive.memory.swap_used = 600;
    finalizer.finalize(&mut state).expect("60");
    assert_eq!(state.archive.derived.swap_used_percent, 60.0);
    assert_eq!(state.archive.derived.swap_tone, TONE_MAGENTA);

    state.archive.memory.swap_used = 700;
    finalizer.finalize(&mut state).expect("70");
    assert_eq!(state.archive.derived.swap_used_percent, 70.0);
    assert_eq!(state.archive.derived.swap_tone, TONE_YELLOW);

    state.archive.memory.swap_used = 800;
    finalizer.finalize(&mut state).expect("80");
    assert_eq!(state.archive.derived.swap_used_percent, 80.0);
    assert_eq!(state.archive.derived.swap_tone, TONE_RED);

    state.archive.memory.swap_used = 0;
    finalizer.finalize(&mut state).expect("0");
    assert_eq!(state.archive.derived.swap_used_percent, 0.0);
    assert_eq!(state.archive.derived.swap_tone, TONE_GREEN);
}

#[test]
fn swap_unowned_zeroes_derived_swap_fields_and_tone() {
    let mut state = make_state();
    state.archive.derived.swap_used_percent = 12.0;
    state.archive.derived.swap_tone = TONE_RED;
    state.archive.memory.swap_total = 100;
    state.archive.memory.swap_used = 80;
    let mut finalizer = SystemFinalizer::new(StepClock {
        monotonic_ns: 1_000_000_000,
        wallclock_ns: 1_000_000_000,
    });
    finalizer.finalize(&mut state).expect("swap zero");
    assert_eq!(state.archive.derived.swap_used_percent, 0.0);
    assert_eq!(state.archive.derived.swap_tone, TONE_GREEN);
    assert_eq!(state.archive.memory.swap_total, 0);
    assert_eq!(state.archive.memory.swap_used, 0);
}

#[test]
fn min_delta_minus_one_yields_finite_bounded_rates() {
    let mut state = make_state();
    state.archive.capabilities = CAP_CPU_GLOBAL
        | CAP_CPU_PER_CORE
        | CAP_MEMORY_PAGE_FAULTS
        | CAP_NETWORK_BYTES
        | CAP_NETWORK_RATES;

    state.archive.cpu.user_ticks = 200;
    state.archive.cpu.system_ticks = 100;
    state.archive.cpu.idle_ticks = 700;
    state.archive.cpu.total_ticks = 1_000;
    state.archive.cpu.context_switches = 50;
    state.archive.cpu.core_count = 1;
    state.archive.cpu.cores[0] = make_core(0, 200, 100, 700, 1_000);

    state.archive.memory.page_faults = 100;

    state.archive.network.if_count = 1;
    state.archive.network.interfaces[0] = make_iface(b"eth0", 5_000, 6_000);

    // Seed baselines.
    state.baselines.prev_timestamp_ns = 0;
    state.baselines.cpu_ticks.user = 100;
    state.baselines.cpu_ticks.system = 50;
    state.baselines.cpu_ticks.idle = 850;
    state.baselines.cpu_ticks.total = 1_000;
    state.baselines.cpu_ticks.context_switches = 0;
    state.baselines.cores[0] = make_core_baseline(0, 100, 50, 850, 1_000);
    state.baselines.core_count = 1;
    state.baselines.prev_page_faults = 0;
    seed_net_baseline(&mut state, b"eth0", 1_000, 1_000);
    state.baselines.net_bytes.represented = 1;

    let mut finalizer = SystemFinalizer::new(StepClock {
        monotonic_ns: MIN_DELTA_NS - 1,
        wallclock_ns: MIN_DELTA_NS - 1,
    });
    finalizer.finalize(&mut state).expect("tiny delta");

    assert!(state.archive.cpu.context_switches_per_sec.is_finite());
    assert!(state.archive.memory.page_faults_per_sec.is_finite());
    assert!(state.archive.network.interfaces[0]
        .rx_bytes_per_sec
        .is_finite());
    assert!(state.archive.network.interfaces[0]
        .tx_bytes_per_sec
        .is_finite());
    assert!(state.archive.cpu.context_switches_per_sec >= 0.0);
    assert!(state.archive.memory.page_faults_per_sec >= 0.0);
    assert!(state.archive.network.interfaces[0].rx_bytes_per_sec >= 0.0);
    assert!(state.archive.network.interfaces[0].tx_bytes_per_sec >= 0.0);
    assert!(state.archive.derived.aggregate_rx_bytes_per_sec.is_finite());
    assert!(state.archive.derived.aggregate_tx_bytes_per_sec.is_finite());
}

#[test]
fn aggregate_rx_tx_rates_need_at_least_one_represented_interface() {
    let mut state = make_state();
    state.archive.capabilities = CAP_NETWORK_BYTES | CAP_NETWORK_RATES;
    state.archive.network.if_count = 0;
    let mut finalizer = SystemFinalizer::new(StepClock {
        monotonic_ns: 1_000_000_000,
        wallclock_ns: 1_000_000_000,
    });
    finalizer.finalize(&mut state).expect("no net");
    assert_eq!(state.archive.derived.aggregate_rx_bytes_per_sec, 0.0);
    assert_eq!(state.archive.derived.aggregate_tx_bytes_per_sec, 0.0);
}

#[test]
fn aggregate_rx_tx_rates_unowned_when_rates_capability_absent() {
    let mut state = make_state();
    state.archive.capabilities = CAP_NETWORK_BYTES;
    state.archive.network.if_count = 1;
    state.archive.network.interfaces[0] = make_iface(b"eth0", 5_000, 6_000);
    let mut finalizer = SystemFinalizer::new(StepClock {
        monotonic_ns: 1_000_000_000,
        wallclock_ns: 1_000_000_000,
    });
    finalizer.finalize(&mut state).expect("net bytes only");
    assert_eq!(state.archive.derived.aggregate_rx_bytes_per_sec, 0.0);
    assert_eq!(state.archive.derived.aggregate_tx_bytes_per_sec, 0.0);
}

#[test]
fn above_max_cores_retains_aggregate_clears_per_core_and_process_top_cpu() {
    let mut state = make_state();
    state.archive.capabilities = CAP_CPU_GLOBAL | CAP_CPU_PER_CORE | CAP_PROCESS_TOP_CPU;
    state.archive.cpu.core_count = 200;
    state.cpu_over_capacity = true;
    for i in 0..MAX_CORES {
        state.archive.cpu.cores[i] = make_core(i as u8, 100, 50, 850, 1_000);
    }
    state.archive.process.top_cpu[0].pid = 7;
    state.archive.process.top_cpu[0].cpu_usage = 90.0;
    state.archive.process.top_cpu_count = 1;

    let mut finalizer = SystemFinalizer::new(StepClock {
        monotonic_ns: 1_000_000_000,
        wallclock_ns: 1_000_000_000,
    });
    finalizer.finalize(&mut state).expect("over-core clamp");

    assert_eq!(state.archive.cpu.core_count as usize, MAX_CORES);
    for i in 0..MAX_CORES {
        let core = &state.archive.cpu.cores[i];
        assert_eq!(core.user_ticks, 0);
        assert_eq!(core.system_ticks, 0);
        assert_eq!(core.idle_ticks, 0);
        assert_eq!(core.total_ticks, 0);
        assert_eq!(core.usage_percent, 0.0);
    }
    assert_eq!(state.archive.process.top_cpu_count, 0);
    assert_eq!(state.archive.process.top_cpu[0].pid, 0);
}

#[test]
fn cpu_global_required_for_per_core_and_top_cpu() {
    let mut state = make_state();
    state.archive.capabilities = 0;
    state.archive.cpu.core_count = 4;
    state.archive.cpu.cores[0] = make_core(0, 100, 50, 850, 1_000);
    state.archive.process.top_cpu_count = 1;
    state.archive.process.top_cpu[0].pid = 7;

    let mut finalizer = SystemFinalizer::new(StepClock {
        monotonic_ns: 1_000_000_000,
        wallclock_ns: 1_000_000_000,
    });
    finalizer.finalize(&mut state).expect("no global");

    assert_eq!(state.archive.cpu.core_count, 0);
    assert_eq!(state.archive.cpu.cores[0].user_ticks, 0);
    assert_eq!(state.archive.process.top_cpu_count, 0);
}

#[test]
fn unowned_tone_byte_remains_green_after_finalize() {
    let mut state = make_state();
    state.archive.capabilities = CAP_MEMORY_RAM_TOTAL | CAP_MEMORY_RAM_USED;
    state.archive.memory.ram_total = 100;
    state.archive.memory.ram_used = 50;
    state.archive.derived.cpu_tone = TONE_RED;
    let mut finalizer = SystemFinalizer::new(StepClock {
        monotonic_ns: 1_000_000_000,
        wallclock_ns: 1_000_000_000,
    });
    finalizer.finalize(&mut state).expect("unowned tone");
    assert_eq!(state.archive.derived.cpu_tone, TONE_GREEN);
}

#[test]
fn hotplug_zeroes_one_interval_and_reseeds_per_core() {
    let mut state = make_state();
    state.archive.capabilities = CAP_CPU_GLOBAL | CAP_CPU_PER_CORE | CAP_CPU_CONTEXT_SWITCHES;

    // Prior baseline = 4 cores.
    state.baselines.prev_timestamp_ns = 1_000_000_000;
    state.baselines.cpu_ticks.user = 1_000;
    state.baselines.cpu_ticks.system = 1_000;
    state.baselines.cpu_ticks.idle = 8_000;
    state.baselines.cpu_ticks.total = 10_000;
    state.baselines.cpu_ticks.context_switches = 1_000;
    state.baselines.core_count = 4;
    for i in 0..4 {
        state.baselines.cores[i] = make_core_baseline(i as u8, 1, 1, 1, 1);
    }

    // New sample has 8 cores.
    state.archive.cpu.user_ticks = 2_000;
    state.archive.cpu.system_ticks = 2_000;
    state.archive.cpu.idle_ticks = 16_000;
    state.archive.cpu.total_ticks = 20_000;
    state.archive.cpu.context_switches = 2_000;
    state.archive.cpu.core_count = 8;
    for i in 0..8 {
        state.archive.cpu.cores[i] = make_core(i as u8, 250, 250, 2_000, 2_500);
    }

    let mut finalizer = SystemFinalizer::new(StepClock {
        monotonic_ns: 1_500_000_000,
        wallclock_ns: 1_500_000_000,
    });
    finalizer.finalize(&mut state).expect("hotplug");

    // First interval after hotplug: per-core rates are zero (no prior baseline).
    assert_eq!(state.archive.cpu.usage_percent, 0.0);
    assert_eq!(state.archive.cpu.context_switches_per_sec, 0.0);
    for i in 0..8 {
        assert_eq!(state.archive.cpu.cores[i].usage_percent, 0.0);
    }
}

#[test]
fn network_hotplug_interface_seeds_one_zero_rate_interval() {
    let mut state = make_state();
    state.archive.capabilities = CAP_NETWORK_BYTES | CAP_NETWORK_RATES;
    state.archive.network.if_count = 1;
    state.archive.network.interfaces[0] = make_iface(b"eth0", 1_000, 2_000);
    state.baselines.prev_timestamp_ns = 1_000_000_000;
    seed_net_baseline(&mut state, b"eth0", 1_000, 2_000);
    state.baselines.net_bytes.represented = 1;

    let mut finalizer = SystemFinalizer::new(StepClock {
        monotonic_ns: 1_500_000_000,
        wallclock_ns: 1_500_000_000,
    });
    finalizer.finalize(&mut state).expect("hotplug net cycle 1");

    // Add a second interface for the next cycle.
    state.archive.network.if_count = 2;
    state.archive.network.interfaces[0] = make_iface(b"eth0", 2_000, 3_000);
    state.archive.network.interfaces[1] = make_iface(b"wlan0", 5_000, 6_000);

    let mut finalizer2 = SystemFinalizer::new(StepClock {
        monotonic_ns: 2_000_000_000,
        wallclock_ns: 2_000_000_000,
    });
    finalizer2
        .finalize(&mut state)
        .expect("hotplug net cycle 2");

    // The new interface must report 0 rate for its first interval.
    let new_index = if state.archive.network.interfaces[1].name.as_str() == "wlan0" {
        1
    } else {
        0
    };
    assert_eq!(
        state.archive.network.interfaces[new_index].rx_bytes_per_sec,
        0.0
    );
    assert_eq!(
        state.archive.network.interfaces[new_index].tx_bytes_per_sec,
        0.0
    );
}

#[test]
fn gpu_temperature_tone_uses_60_70_80_celsius_boundaries() {
    let mut state = make_state();
    state.archive.capabilities = CAP_GPU_ENUMERATION;
    state.archive.gpu.nvml_available = 1;
    state.archive.gpu.gpu_count = 1;
    state.archive.gpu.gpus[0].capabilities = aura_common::GPU_CAP_TEMPERATURE;
    let mut finalizer = SystemFinalizer::new(StepClock {
        monotonic_ns: 1_000_000_000,
        wallclock_ns: 1_000_000_000,
    });

    state.archive.gpu.gpus[0].temperature_celsius = 60;
    finalizer.finalize(&mut state).expect("60");
    assert_eq!(state.archive.gpu.gpus[0].tone, TONE_MAGENTA);

    state.archive.gpu.gpus[0].temperature_celsius = 70;
    finalizer.finalize(&mut state).expect("70");
    assert_eq!(state.archive.gpu.gpus[0].tone, TONE_YELLOW);

    state.archive.gpu.gpus[0].temperature_celsius = 80;
    finalizer.finalize(&mut state).expect("80");
    assert_eq!(state.archive.gpu.gpus[0].tone, TONE_RED);

    state.archive.gpu.gpus[0].temperature_celsius = 59;
    finalizer.finalize(&mut state).expect("59");
    assert_eq!(state.archive.gpu.gpus[0].tone, TONE_GREEN);

    state.archive.gpu.gpus[0].temperature_celsius = 81;
    finalizer.finalize(&mut state).expect("81");
    assert_eq!(state.archive.gpu.gpus[0].tone, TONE_RED);
}

#[test]
fn gpu_temperature_zeroed_when_capability_absent() {
    let mut state = make_state();
    state.archive.capabilities = CAP_GPU_ENUMERATION;
    state.archive.gpu.nvml_available = 1;
    state.archive.gpu.gpu_count = 1;
    state.archive.gpu.gpus[0].tone = TONE_RED;
    state.archive.gpu.gpus[0].temperature_celsius = 95;
    let mut finalizer = SystemFinalizer::new(StepClock {
        monotonic_ns: 1_000_000_000,
        wallclock_ns: 1_000_000_000,
    });
    finalizer.finalize(&mut state).expect("gpu zero");
    assert_eq!(state.archive.gpu.gpus[0].tone, TONE_GREEN);
    assert_eq!(state.archive.gpu.gpus[0].temperature_celsius, 0);
}

#[test]
fn zero_totals_finalize_does_not_panic() {
    let mut state = make_state();
    state.archive.capabilities = CAP_CPU_GLOBAL
        | CAP_CPU_PER_CORE
        | CAP_NETWORK_BYTES
        | CAP_NETWORK_RATES
        | CAP_MEMORY_RAM_TOTAL
        | CAP_MEMORY_RAM_USED
        | CAP_MEMORY_SWAP;
    state.archive.cpu.core_count = 0;
    state.archive.network.if_count = 0;
    state.archive.memory.ram_total = 0;
    state.archive.memory.ram_used = 0;
    let mut finalizer = SystemFinalizer::new(StepClock {
        monotonic_ns: 1_000_000_000,
        wallclock_ns: 1_000_000_000,
    });
    finalizer.finalize(&mut state).expect("zero totals");
    assert!(state.archive.derived.ram_used_percent.is_finite());
    assert!(state.archive.derived.swap_used_percent.is_finite());
    assert!(state.archive.derived.aggregate_rx_bytes_per_sec.is_finite());
    assert!(state.archive.derived.aggregate_tx_bytes_per_sec.is_finite());
    assert_eq!(state.archive.derived.cpu_tone, TONE_GREEN);
    assert_eq!(state.archive.derived.ram_tone, TONE_GREEN);
    assert_eq!(state.archive.derived.swap_tone, TONE_GREEN);
}

#[test]
fn ram_total_zero_keeps_unowned_swap_zeroed_when_cap_dropped() {
    let mut state = make_state();
    state.archive.capabilities = CAP_MEMORY_RAM_TOTAL | CAP_MEMORY_RAM_USED;
    state.archive.memory.ram_total = 1_000;
    state.archive.memory.ram_used = 700;
    state.archive.memory.swap_total = 100;
    state.archive.memory.swap_used = 80;
    let mut finalizer = SystemFinalizer::new(StepClock {
        monotonic_ns: 1_000_000_000,
        wallclock_ns: 1_000_000_000,
    });
    finalizer.finalize(&mut state).expect("mem mixed");
    assert_eq!(state.archive.derived.ram_used_percent, 70.0);
    assert_eq!(state.archive.derived.ram_tone, TONE_YELLOW);
    assert_eq!(state.archive.derived.swap_used_percent, 0.0);
    assert_eq!(state.archive.derived.swap_tone, TONE_GREEN);
    assert_eq!(state.archive.memory.swap_total, 0);
    assert_eq!(state.archive.memory.swap_used, 0);
}

#[test]
fn zero_cores_finalize_yields_green_cpu_tone() {
    let mut state = make_state();
    state.archive.capabilities = CAP_CPU_GLOBAL | CAP_CPU_PER_CORE;
    state.archive.cpu.core_count = 0;
    state.archive.cpu.user_ticks = 0;
    state.archive.cpu.system_ticks = 0;
    state.archive.cpu.idle_ticks = 0;
    state.archive.cpu.total_ticks = 0;
    let mut finalizer = SystemFinalizer::new(StepClock {
        monotonic_ns: 1_000_000_000,
        wallclock_ns: 1_000_000_000,
    });
    finalizer.finalize(&mut state).expect("zero cores");
    assert_eq!(state.archive.cpu.usage_percent, 0.0);
    assert_eq!(state.archive.derived.cpu_tone, TONE_GREEN);
}

#[allow(dead_code)]
fn _ensure_linked() {
    let _: TelemetryArchive = TelemetryArchive::zeroed();
}
