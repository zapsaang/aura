use std::fs::File;
use std::io::Read;
use std::sync::OnceLock;

use aura_common::{AuraResult, FixedString16, NetIfStat, NetworkStats, MAX_NETIFS};
use log::warn;

use crate::collectors::parsing::{parse_u64, split_whitespace, trim_ascii};
use crate::collectors::NetByteSnapshot;

static NETIF_LIMIT_WARNED: OnceLock<()> = OnceLock::new();

#[derive(Default)]
pub struct LinuxNetworkCollector;

impl LinuxNetworkCollector {
    pub const fn new() -> Self {
        Self
    }
}

impl super::NetworkCollector for LinuxNetworkCollector {
    fn collect(
        &self,
        buf: &mut [u8; 4096],
        out: &mut NetworkStats,
        prev: &mut NetByteSnapshot,
        delta_secs: f32,
    ) -> AuraResult<()> {
        collect(buf, out, prev, delta_secs)
    }
}

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
        if count >= MAX_NETIFS && NETIF_LIMIT_WARNED.get().is_none() {
            warn!(
                "Network interface limit reached: {} interfaces detected (MAX_NETIFS={}). \
                Some interfaces will not be monitored.",
                line_no.saturating_sub(2),
                MAX_NETIFS
            );
            NETIF_LIMIT_WARNED.set(()).ok();
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
                rx = parse_u64(tok).unwrap_or(0);
            } else if idx == 8 {
                tx = parse_u64(tok).unwrap_or(0);
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

#[cfg(test)]
mod tests {
    use aura_common::{NetIfStat, MAX_NETIFS};

    use super::parse_net_dev;

    #[test]
    fn parse_net_dev_sample() {
        let fixture = include_bytes!("../../../tests/fixtures/proc_net_dev_sample.txt");
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
