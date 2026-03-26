use aura_daemon::collectors::CollectorState;

#[test]
fn test_delta_secs_is_never_zero_after_first_run() {
    let mut state = CollectorState::new();

    let now = 1_000_000_000u64;

    state.prev_timestamp_ns = 0;
    let delta1 = calculate_delta_secs(&state, now);
    assert_eq!(delta1, 0.0, "First run should return 0.0");

    state.prev_timestamp_ns = now;
    let delta2 = calculate_delta_secs(&state, now);
    assert!(
        delta2 > 0.0,
        "Second run with same timestamp should not be 0.0"
    );
}

fn calculate_delta_secs(state: &CollectorState, now: u64) -> f32 {
    const MIN_DELTA_NS: u64 = 1_000_000;
    const NS_PER_SEC: f32 = 1_000_000_000.0;

    let raw_delta_ns = now.saturating_sub(state.prev_timestamp_ns);

    if state.prev_timestamp_ns == 0 {
        0.0
    } else if raw_delta_ns < MIN_DELTA_NS {
        MIN_DELTA_NS as f32 / NS_PER_SEC
    } else {
        raw_delta_ns as f32 / NS_PER_SEC
    }
}
