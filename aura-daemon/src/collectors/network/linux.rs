use std::fs::File;
use std::io::Read;

use aura_common::{AuraResult, FixedString16, NetIfStat, NetworkStats, MAX_NETIFS};

use crate::collectors::parsing::{parse_u64, split_whitespace, trim_ascii};
use crate::collectors::NetByteSnapshot;

pub fn parse_net_dev(buf: &[u8], out: &mut NetworkStats) -> AuraResult<()> {
    let mut count = 0usize;
    let mut line_start = 0usize;
    let mut line_no = 0usize;
    out.truncated = 0;

    for i in 0..buf.len() {
        if buf[i] != b'\n' {
            continue;
        }
        let line = &buf[line_start..i];
        line_start = i + 1;

        line_no += 1;
        if line_no <= 2 {
            continue;
        }

        let Some(colon) = line.iter().position(|&c| c == b':') else {
            continue;
        };

        let name = trim_ascii(&line[..colon]);
        if name == b"lo" || name.starts_with(b"docker") || name.starts_with(b"veth") {
            continue;
        }

        let values = trim_ascii(&line[colon + 1..]);
        let mut rx = 0u64;
        let mut tx = 0u64;
        for (idx, tok) in split_whitespace(values).enumerate() {
            if idx == 0 {
                rx = parse_u64(tok).unwrap_or(0);
            } else if idx == 8 {
                tx = parse_u64(tok).unwrap_or(0);
                break;
            }
        }

        if count >= MAX_NETIFS {
            out.truncated = 1;
            break;
        }
        out.interfaces[count] = NetIfStat {
            name: FixedString16::from_bytes(name),
            rx_bytes: rx,
            tx_bytes: tx,
            rx_bytes_per_sec: 0.0,
            tx_bytes_per_sec: 0.0,
        };
        count += 1;
    }

    out.if_count = count as u8;
    Ok(())
}

pub fn collect(
    buf: &mut Vec<u8>,
    out: &mut NetworkStats,
    prev: &mut NetByteSnapshot,
    delta_secs: f64,
) -> AuraResult<()> {
    buf.clear();
    let mut f = File::open("/proc/net/dev")?;
    f.read_to_end(buf)?;
    parse_net_dev(&buf[..], out)?;

    apply_rate_calculations(out, prev, delta_secs);

    Ok(())
}

fn apply_rate_calculations(out: &mut NetworkStats, prev: &mut NetByteSnapshot, delta_secs: f64) {
    let count = out.if_count as usize;
    let mut i = 0usize;
    while i < count && i < MAX_NETIFS {
        let rx = out.interfaces[i].rx_bytes;
        let tx = out.interfaces[i].tx_bytes;
        let (prx, ptx) = prev.interfaces[i];
        out.interfaces[i].rx_bytes_per_sec = calculate_rate(rx, prx, delta_secs);
        out.interfaces[i].tx_bytes_per_sec = calculate_rate(tx, ptx, delta_secs);
        prev.interfaces[i] = (rx, tx);
        i += 1;
    }
    prev.count = count;
}

fn calculate_rate(current: u64, previous: u64, delta_secs: f64) -> f32 {
    if delta_secs > 0.0 {
        (current.saturating_sub(previous) as f64 / delta_secs) as f32
    } else {
        0.0
    }
}

#[cfg(test)]
mod tests {
    use aura_common::{FixedString16, NetIfStat, NetworkStats, MAX_NETIFS};

    use super::{apply_rate_calculations, parse_net_dev};
    use crate::collectors::NetByteSnapshot;

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

    #[test]
    fn parse_net_dev_sample() {
        let fixture = include_bytes!("../../../tests/fixtures/proc_net_dev_sample.txt");
        let mut stats = empty_stats();
        stats.truncated = 1;
        parse_net_dev(fixture, &mut stats).expect("parse");
        assert_eq!(stats.if_count, 1);
        assert_eq!(stats.truncated, 0);
        assert_eq!(stats.interfaces[0].name.as_str(), "eth0");
        assert_eq!(stats.interfaces[0].rx_bytes, 5678);
        assert_eq!(stats.interfaces[0].tx_bytes, 8765);
    }

    #[test]
    fn calculate_net_rates_from_parsed_sample() {
        let fixture = include_bytes!("../../../tests/fixtures/proc_net_dev_sample.txt");
        let mut stats = empty_stats();
        parse_net_dev(fixture, &mut stats).expect("parse");

        let mut prev = NetByteSnapshot::zero();
        prev.interfaces[0] = (1000, 2000);
        apply_rate_calculations(&mut stats, &mut prev, 1.0);

        assert_eq!(stats.interfaces[0].rx_bytes_per_sec, 4678.0);
        assert_eq!(stats.interfaces[0].tx_bytes_per_sec, 6765.0);
        assert_eq!(prev.interfaces[0], (5678, 8765));
        assert_eq!(prev.count, 1);
    }
}
