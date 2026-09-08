#![cfg(target_os = "linux")]

use aura_common::{FixedString16, NetIfStat, NetworkStats, MAX_NETIFS};
use aura_daemon::collectors::network::linux::parse_net_dev;

const OVER_CAP_FIXTURE: &[u8] = include_bytes!("fixtures/proc_net_dev_over_cap.txt");
const SAMPLE_FIXTURE: &[u8] = include_bytes!("fixtures/proc_net_dev_sample.txt");

fn empty_stats() -> NetworkStats {
    NetworkStats {
        interfaces: [NetIfStat {
            name: FixedString16::new(),
            rx_bytes: 0,
            tx_bytes: 0,
            rx_bytes_per_sec: 0.0,
            tx_bytes_per_sec: 0.0,
        }; MAX_NETIFS],
        if_count: 0,
        truncated: 0,
        _pad0: [0; 6],
    }
}

fn expected_name(index: usize) -> String {
    format!("eth{index}")
}

fn expected_rx(index: usize) -> u64 {
    10_000 + index as u64
}

fn expected_tx(index: usize) -> u64 {
    20_000 + index as u64
}

fn assert_retained_prefix(stats: &NetworkStats) {
    for (index, iface) in stats.interfaces.iter().enumerate() {
        assert_eq!(iface.name.as_str(), expected_name(index), "name[{index}]");
        assert_eq!(iface.rx_bytes, expected_rx(index), "rx[{index}]");
        assert_eq!(iface.tx_bytes, expected_tx(index), "tx[{index}]");
    }
}

fn at_cap_input() -> String {
    let mut input =
        String::from("Inter-|   Receive   | Transmit\n face |bytes packets | bytes packets\n");
    for index in 0..MAX_NETIFS {
        input.push_str(&format!(
            "  eth{index}: {rx} 1 0 0 0 0 0 0 {tx} 1 0 0 0 0 0 0\n",
            rx = expected_rx(index),
            tx = expected_tx(index),
        ));
    }
    input
}

#[test]
fn over_cap_cycle_reports_max_and_truncation() {
    let mut stats = empty_stats();
    parse_net_dev(OVER_CAP_FIXTURE, &mut stats).expect("parse");
    assert_eq!(stats.if_count, MAX_NETIFS as u8);
    assert_eq!(stats.truncated, 1);
    assert_retained_prefix(&stats);
}

#[test]
fn over_cap_two_cycles_in_one_process_are_identical() {
    let mut first = empty_stats();
    parse_net_dev(OVER_CAP_FIXTURE, &mut first).expect("first parse");

    let mut second = empty_stats();
    parse_net_dev(OVER_CAP_FIXTURE, &mut second)
        .expect("second parse in the same process must not panic");

    assert_eq!(first.if_count, MAX_NETIFS as u8);
    assert_eq!(second.if_count, MAX_NETIFS as u8);
    assert_eq!(first.truncated, 1, "first overflowing cycle must truncate");
    assert_eq!(second.truncated, 1, "later overflowing cycle must truncate");
    assert_retained_prefix(&first);
    assert_retained_prefix(&second);
}

#[test]
fn over_cap_filtering_semantics_unchanged() {
    let mut stats = empty_stats();
    parse_net_dev(OVER_CAP_FIXTURE, &mut stats).expect("parse");

    assert_eq!(stats.if_count, MAX_NETIFS as u8);
    assert_eq!(stats.truncated, 1);
    for (index, iface) in stats.interfaces.iter().enumerate() {
        let name = iface.name.as_str();
        assert_eq!(name, expected_name(index));
        assert_ne!(name, "lo");
        assert!(!name.starts_with("docker"), "docker row leaked: {name}");
        assert!(!name.starts_with("veth"), "veth row leaked: {name}");
    }
}

#[test]
fn at_cap_input_keeps_every_interface_without_truncation() {
    let input = at_cap_input();
    let mut stats = empty_stats();
    parse_net_dev(input.as_bytes(), &mut stats).expect("parse");
    assert_eq!(stats.if_count, MAX_NETIFS as u8);
    assert_eq!(stats.truncated, 0);
    assert_retained_prefix(&stats);
}

#[test]
fn under_cap_sample_reports_no_truncation() {
    let mut stats = empty_stats();
    stats.truncated = 1;
    parse_net_dev(SAMPLE_FIXTURE, &mut stats).expect("parse");
    assert_eq!(stats.if_count, 1);
    assert_eq!(
        stats.truncated, 0,
        "non-overflowing cycle must clear truncation"
    );
    assert_eq!(stats.interfaces[0].name.as_str(), "eth0");
    assert_eq!(stats.interfaces[0].rx_bytes, 5678);
    assert_eq!(stats.interfaces[0].tx_bytes, 8765);
}
