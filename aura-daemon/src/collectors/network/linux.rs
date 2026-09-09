use std::fs::File;

use aura_common::{AuraError, AuraResult, FixedString16, NetIfStat, NetworkStats, MAX_NETIFS};

use crate::collectors::parsing::{parse_u64_strict, read_reused, split_whitespace, trim_ascii};
#[cfg(test)]
use crate::collectors::NetByteSnapshot;

pub fn parse_net_dev(buf: &[u8], out: &mut NetworkStats) -> AuraResult<()> {
    let mut headers = buf.split(|byte| *byte == b'\n');
    let first = headers.next().unwrap_or_default();
    let second = headers.next().unwrap_or_default();
    if !first.starts_with(b"Inter-|")
        || !trim_ascii(second).starts_with(b"face |")
        || !contains_bytes(second, b"bytes")
    {
        return Err(AuraError::ParseError(
            "malformed /proc/net/dev headers".to_string(),
        ));
    }
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
            if !trim_ascii(line).is_empty() {
                out.truncated = 1;
            }
            continue;
        };

        let name = trim_ascii(&line[..colon]);
        if name == b"lo" || name.starts_with(b"docker") || name.starts_with(b"veth") {
            continue;
        }
        if name.is_empty() {
            out.truncated = 1;
            continue;
        }

        let values = trim_ascii(&line[colon + 1..]);
        let Ok((rx, tx)) = parse_interface_counters(values) else {
            out.truncated = 1;
            continue;
        };

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

fn parse_interface_counters(values: &[u8]) -> AuraResult<(u64, u64)> {
    let mut rx = None;
    let mut tx = None;
    let mut count = 0usize;
    for (index, token) in split_whitespace(values).take(9).enumerate() {
        let value = parse_u64_strict(token)?;
        count += 1;
        if index == 0 {
            rx = Some(value);
        } else if index == 8 {
            tx = Some(value);
        }
    }
    if count < 9 {
        return Err(AuraError::ParseError(
            "malformed /proc/net/dev interface row".to_string(),
        ));
    }
    match (rx, tx) {
        (Some(rx), Some(tx)) => Ok((rx, tx)),
        _ => Err(AuraError::ParseError(
            "malformed /proc/net/dev interface row".to_string(),
        )),
    }
}

fn contains_bytes(haystack: &[u8], needle: &[u8]) -> bool {
    haystack
        .windows(needle.len())
        .any(|window| window == needle)
}

pub fn collect(buf: &mut Vec<u8>, out: &mut NetworkStats) -> AuraResult<()> {
    buf.clear();
    let mut f = File::open("/proc/net/dev")?;
    read_reused(&mut f, buf)?;
    parse_net_dev(&buf[..], out)?;

    Ok(())
}

#[cfg(test)]
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

#[cfg(test)]
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
