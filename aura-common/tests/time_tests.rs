use aura_common::monotonic_ns;

#[test]
fn monotonic_ns_returns_reasonable_value() {
    let t1 = monotonic_ns();
    std::thread::sleep(std::time::Duration::from_millis(1));
    let t2 = monotonic_ns();
    assert!(t2 > t1, "monotonic_ns should always increase");
    assert!(
        t2 - t1 < 1_000_000_000,
        "should be less than 1 second (sanity check)"
    );
}
