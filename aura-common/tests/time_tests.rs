use aura_common::monotonic_ns;

#[test]
fn monotonic_ns_returns_reasonable_value() {
    let t1 = monotonic_ns();
    std::thread::sleep(std::time::Duration::from_millis(1));
    let t2 = monotonic_ns();
    assert!(t2 > t1, "monotonic_ns should always increase");
    assert!(t2 - t1 < 10_000_000, "1ms should be less than 10ms in ns");
}
