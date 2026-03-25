#[cfg(target_os = "linux")]
#[test]
fn collect_from_real_proc_files() {
    use aura_daemon::collectors;

    let mut state = collectors::CollectorState::new();
    collectors::init(&mut state).expect("init");
    collectors::collect_all(&mut state).expect("collect");

    assert!(state.telemetry.cpu.total_ticks > 0);
    assert!(state.telemetry.memory.ram_total > 0);
    assert!(state.telemetry.process.total > 0);
    assert!(state.telemetry.meta.uptime_secs > 0);
}
