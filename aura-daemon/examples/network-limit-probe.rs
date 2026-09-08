//! Release probe for the network-interface capacity guard.
//!
//! Parses the over-cap fixture twice in one process under the release profile
//! (`panic = "abort"`). Each cycle must cap the count at MAX_NETIFS, set
//! truncation, keep the deterministic first-MAX_NETIFS prefix, and leave the
//! trailing canary untouched. Any violation exits nonzero before printing.

use std::process::ExitCode;

#[cfg(target_os = "linux")]
mod probe {
    use std::process::ExitCode;

    use aura_common::{FixedString16, NetIfStat, NetworkStats, MAX_NETIFS};
    use aura_daemon::collectors::network::linux::parse_net_dev;

    const OVER_CAP_FIXTURE: &[u8] = include_bytes!("../tests/fixtures/proc_net_dev_over_cap.txt");
    const CANARY: u64 = 0x5A5A_5A5A_5A5A_5A5A;

    struct Probe {
        stats: NetworkStats,
        canary: u64,
    }

    impl Probe {
        fn new() -> Self {
            Self {
                stats: NetworkStats {
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
                },
                canary: CANARY,
            }
        }
    }

    fn fail(cycle: u32, reason: String) -> ExitCode {
        eprintln!("network-limit-probe: cycle {cycle}: {reason}");
        ExitCode::FAILURE
    }

    fn run_cycle(cycle: u32) -> Result<(), ExitCode> {
        let mut probe = Probe::new();
        parse_net_dev(OVER_CAP_FIXTURE, &mut probe.stats)
            .map_err(|error| fail(cycle, format!("parse failed: {error}")))?;

        if probe.canary != CANARY {
            return Err(fail(cycle, "canary clobbered".to_string()));
        }
        if probe.stats.if_count != MAX_NETIFS as u8 {
            return Err(fail(cycle, "count not capped".to_string()));
        }
        if probe.stats.truncated != 1 {
            return Err(fail(cycle, "truncation not set".to_string()));
        }
        for (index, iface) in probe.stats.interfaces.iter().enumerate() {
            let expected_rx = 10_000 + index as u64;
            let expected_tx = 20_000 + index as u64;
            if iface.name.as_str() != format!("eth{index}")
                || iface.rx_bytes != expected_rx
                || iface.tx_bytes != expected_tx
            {
                return Err(fail(cycle, "retained prefix drifted".to_string()));
            }
        }
        Ok(())
    }

    pub fn run() -> ExitCode {
        for cycle in 1..=2u32 {
            if let Err(code) = run_cycle(cycle) {
                return code;
            }
        }
        println!("{{\"checks\":2,\"status\":\"ok\"}}");
        ExitCode::SUCCESS
    }
}

#[cfg(target_os = "linux")]
fn main() -> ExitCode {
    probe::run()
}

#[cfg(not(target_os = "linux"))]
fn main() -> ExitCode {
    eprintln!("network-limit-probe: supported on Linux only");
    ExitCode::FAILURE
}
