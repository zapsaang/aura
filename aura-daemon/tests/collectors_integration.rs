#[cfg(target_os = "linux")]
#[test]
fn collect_from_real_proc_files() {
    use aura_daemon::collectors;

    let mut state = collectors::CollectorState::new();
    collectors::init(&mut state).expect("init");
    let sample = collectors::collect_sample(&mut state).expect("collect");

    assert!(sample.archive.cpu.total_ticks > 0);
    assert!(sample.archive.memory.ram_total > 0);
    assert!(sample.archive.meta.uptime_secs > 0);
}

#[cfg(target_os = "linux")]
#[test]
fn collect_cpu_mem_network_meta_only() {
    use aura_common::MAX_NETIFS;
    use aura_daemon::collectors;

    let mut state = collectors::CollectorState::new();
    collectors::init(&mut state).expect("init");
    let sample = collectors::collect_sample(&mut state).expect("collect");

    assert!(sample.archive.cpu.total_ticks > 0, "cpu should work");
    assert!(sample.archive.memory.ram_total > 0, "memory should work");
    assert!(
        sample.archive.network.if_count <= MAX_NETIFS as u8,
        "network if_count valid"
    );
    assert!(
        sample.archive.meta.uptime_secs <= u64::MAX / 2,
        "meta uptime valid"
    );
}

#[cfg(target_os = "linux")]
#[test]
fn finalized_real_cycle_satisfies_the_archive_contract() {
    let mut state = aura_daemon::collectors::CollectorState::new();
    aura_daemon::collectors::init(&mut state).expect("init");
    let sample = aura_daemon::collectors::collect_sample(&mut state).expect("collect");
    aura_common::validate_archive(&sample.archive).expect("valid archive");
}

#[cfg(target_os = "linux")]
#[test]
fn reusable_collector_buffers_do_not_grow_between_cycles() {
    let mut state = aura_daemon::collectors::CollectorState::new();
    aura_daemon::collectors::init(&mut state).expect("init");
    let capacities = state.scratch_capacities();
    for _ in 0..3 {
        aura_daemon::collectors::collect_sample(&mut state).expect("collect");
    }
    assert_eq!(state.scratch_capacities(), capacities);
}
