#[cfg(target_os = "linux")]
#[test]
fn collect_from_real_proc_files() {
    use aura_daemon::collectors;

    let mut state = collectors::CollectorState::new();
    collectors::init(&mut state).expect("init");
    collectors::collect_all(&mut state).expect("collect");

    assert!(state.telemetry.cpu.total_ticks > 0);
    assert!(state.telemetry.memory.ram_total > 0);
    assert!(state.telemetry.meta.uptime_secs > 0);
}

#[cfg(target_os = "linux")]
#[test]
fn collect_cpu_mem_network_meta_only() {
    use aura_common::MAX_NETIFS;
    use aura_daemon::collectors;

    let mut state = collectors::CollectorState::new();
    collectors::init(&mut state).expect("init");
    collectors::collect_all(&mut state).expect("collect");

    assert!(state.telemetry.cpu.total_ticks > 0, "cpu should work");
    assert!(state.telemetry.memory.ram_total > 0, "memory should work");
    assert!(
        state.telemetry.network.if_count <= MAX_NETIFS as u8,
        "network if_count valid"
    );
    assert!(
        state.telemetry.meta.uptime_secs <= u64::MAX / 2,
        "meta uptime valid"
    );
}
