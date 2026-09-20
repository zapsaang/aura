use aura_common::MIN_DELTA_NS;

#[test]
fn delta_secs_is_zero_only_for_the_first_sample() {
    assert_eq!(calculate_delta_secs(0, 1_000_000_000), 0.0);
    assert!(calculate_delta_secs(1_000_000_000, 1_000_000_000) > 0.0);
}

fn calculate_delta_secs(previous: u64, now: u64) -> f32 {
    const NS_PER_SEC: f32 = 1_000_000_000.0;
    if previous == 0 {
        return 0.0;
    }
    now.saturating_sub(previous).max(MIN_DELTA_NS) as f32 / NS_PER_SEC
}
