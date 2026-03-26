use aura_daemon::collectors::CollectorState;

#[test]
fn collector_state_allows_high_pid_access() {
    let mut state = CollectorState::new();

    state.prev_proc_ticks.insert(4_194_304, 100);

    assert_eq!(state.prev_proc_ticks.get(&4_194_304), Some(&100));
}
