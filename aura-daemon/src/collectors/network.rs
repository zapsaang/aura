use std::fs::File;
use std::io::Read;

use aura_common::{AuraResult, FixedString16, NetIfStat, NetworkStats, MAX_NETIFS};

use super::NetByteSnapshot;

pub fn parse_net_dev(
    buf: &[u8],
    interfaces_out: &mut [NetIfStat; MAX_NETIFS],
    count_out: &mut u8,
) -> AuraResult<()> {
    let mut count = 0usize;
    let mut line_start = 0usize;
    let mut line_no = 0usize;

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
        if count >= MAX_NETIFS {
            break;
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
                rx = parse_u64(tok);
            } else if idx == 8 {
                tx = parse_u64(tok);
                break;
            }
        }

        interfaces_out[count] = NetIfStat {
            name: FixedString16::from_bytes(name),
            rx_bytes: rx,
            tx_bytes: tx,
            rx_bytes_per_sec: 0.0,
            tx_bytes_per_sec: 0.0,
        };
        count += 1;
    }

    *count_out = count as u8;
    Ok(())
}

pub fn collect(
    buf: &mut [u8; 4096],
    out: &mut NetworkStats,
    prev: &mut NetByteSnapshot,
    delta_secs: f32,
) -> AuraResult<()> {
    let mut f = File::open("/proc/net/dev")?;
    let n = f.read(buf)?;
    parse_net_dev(&buf[..n], &mut out.interfaces, &mut out.if_count)?;

    let count = out.if_count as usize;
    let mut i = 0usize;
    while i < count && i < MAX_NETIFS {
        let rx = out.interfaces[i].rx_bytes;
        let tx = out.interfaces[i].tx_bytes;
        let (prx, ptx) = prev.interfaces[i];
        out.interfaces[i].rx_bytes_per_sec = if delta_secs > 0.0 {
            rx.saturating_sub(prx) as f32 / delta_secs
        } else {
            0.0
        };
        out.interfaces[i].tx_bytes_per_sec = if delta_secs > 0.0 {
            tx.saturating_sub(ptx) as f32 / delta_secs
        } else {
            0.0
        };
        prev.interfaces[i] = (rx, tx);
        i += 1;
    }
    prev.count = count;

    Ok(())
}

fn parse_u64(b: &[u8]) -> u64 {
    let mut out = 0u64;
    let mut seen = false;
    for &c in b {
        if c.is_ascii_digit() {
            out = out.saturating_mul(10).saturating_add((c - b'0') as u64);
            seen = true;
        } else if seen {
            break;
        }
    }
    out
}

fn trim_ascii(mut b: &[u8]) -> &[u8] {
    while !b.is_empty() && b[0].is_ascii_whitespace() {
        b = &b[1..];
    }
    while !b.is_empty() && b[b.len() - 1].is_ascii_whitespace() {
        b = &b[..b.len() - 1];
    }
    b
}

fn split_whitespace(mut b: &[u8]) -> impl Iterator<Item = &[u8]> {
    std::iter::from_fn(move || {
        while !b.is_empty() && b[0].is_ascii_whitespace() {
            b = &b[1..];
        }
        if b.is_empty() {
            return None;
        }
        let mut end = 0usize;
        while end < b.len() && !b[end].is_ascii_whitespace() {
            end += 1;
        }
        let token = &b[..end];
        b = &b[end..];
        Some(token)
    })
}

#[cfg(test)]
mod tests {
    use aura_common::{NetIfStat, MAX_NETIFS};

    use super::parse_net_dev;

    #[test]
    fn parse_net_dev_sample() {
        let fixture = include_bytes!("../../tests/fixtures/proc_net_dev_sample.txt");
        let mut interfaces = [NetIfStat {
            name: aura_common::FixedString16::new(),
            rx_bytes: 0,
            tx_bytes: 0,
            rx_bytes_per_sec: 0.0,
            tx_bytes_per_sec: 0.0,
        }; MAX_NETIFS];
        let mut count = 0u8;
        parse_net_dev(fixture, &mut interfaces, &mut count).expect("parse");
        assert_eq!(count, 1);
        assert_eq!(interfaces[0].name.as_str(), "eth0");
        assert_eq!(interfaces[0].rx_bytes, 5678);
        assert_eq!(interfaces[0].tx_bytes, 8765);
    }
}
