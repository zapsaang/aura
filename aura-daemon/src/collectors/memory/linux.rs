use std::fs::File;
use std::io::Read;

use aura_common::{AuraResult, MemoryStats};

pub struct LinuxMemoryCollector;

impl LinuxMemoryCollector {
    pub const fn new() -> Self {
        Self
    }
}

impl super::MemoryCollector for LinuxMemoryCollector {
    fn collect(
        &self,
        meminfo_buf: &mut [u8; 4096],
        vmstat_buf: &mut [u8; 4096],
        out: &mut MemoryStats,
        prev_page_faults: &mut u64,
        delta_secs: f32,
    ) -> AuraResult<()> {
        collect(meminfo_buf, vmstat_buf, out, prev_page_faults, delta_secs)
    }
}

pub fn parse_meminfo(buf: &[u8]) -> MemoryStats {
    let mut stats = MemoryStats {
        ram_total: 0,
        ram_free: 0,
        ram_used: 0,
        buffers: 0,
        cached: 0,
        swap_total: 0,
        swap_free: 0,
        swap_used: 0,
        page_faults: 0,
        page_faults_per_sec: 0.0,
        _pad0: [0; 4],
    };

    let mut line_start = 0usize;
    for i in 0..buf.len() {
        if buf[i] != b'\n' {
            continue;
        }
        let line = &buf[line_start..i];
        line_start = i + 1;

        if let Some(colon) = line.iter().position(|&c| c == b':') {
            let key = &line[..colon];
            let val = parse_first_u64(&line[colon + 1..]).saturating_mul(1024);

            if key == b"MemTotal" {
                stats.ram_total = val;
            } else if key == b"MemFree" {
                stats.ram_free = val;
            } else if key == b"Buffers" {
                stats.buffers = val;
            } else if key == b"Cached" {
                stats.cached = val;
            } else if key == b"SwapTotal" {
                stats.swap_total = val;
            } else if key == b"SwapFree" {
                stats.swap_free = val;
            }
        }
    }

    stats.ram_used = stats.ram_total.saturating_sub(stats.ram_free);
    stats.swap_used = stats.swap_total.saturating_sub(stats.swap_free);
    stats
}

pub fn parse_vmstat_page_faults(buf: &[u8]) -> u64 {
    let mut line_start = 0usize;
    for i in 0..buf.len() {
        if buf[i] != b'\n' {
            continue;
        }
        let line = &buf[line_start..i];
        line_start = i + 1;
        if line.starts_with(b"pgfault ") {
            return parse_first_u64(&line[8..]);
        }
    }
    0
}

pub fn collect(
    meminfo_buf: &mut [u8; 4096],
    vmstat_buf: &mut [u8; 4096],
    out: &mut MemoryStats,
    prev_page_faults: &mut u64,
    delta_secs: f32,
) -> AuraResult<()> {
    let mut meminfo = File::open("/proc/meminfo")?;
    let n = meminfo.read(meminfo_buf)?;
    let mut stats = parse_meminfo(&meminfo_buf[..n]);

    let mut vmstat = File::open("/proc/vmstat")?;
    let n2 = vmstat.read(vmstat_buf)?;
    stats.page_faults = parse_vmstat_page_faults(&vmstat_buf[..n2]);

    let delta_faults = stats.page_faults.saturating_sub(*prev_page_faults);
    stats.page_faults_per_sec = if delta_secs > 0.0 {
        delta_faults as f32 / delta_secs
    } else {
        0.0
    };

    *prev_page_faults = stats.page_faults;
    *out = stats;
    Ok(())
}

fn parse_first_u64(b: &[u8]) -> u64 {
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

#[cfg(test)]
mod tests {
    use super::{parse_meminfo, parse_vmstat_page_faults};

    #[test]
    fn parse_meminfo_sample() {
        let fixture = include_bytes!("../../../tests/fixtures/proc_meminfo_sample.txt");
        let stats = parse_meminfo(fixture);
        assert_eq!(stats.ram_total, 16384000 * 1024);
        assert_eq!(stats.swap_free, 1048576 * 1024);
        assert!(stats.ram_used > 0);
    }

    #[test]
    fn parse_vmstat_sample() {
        let fixture = include_bytes!("../../../tests/fixtures/proc_vmstat_sample.txt");
        let faults = parse_vmstat_page_faults(fixture);
        assert_eq!(faults, 67890);
    }
}
