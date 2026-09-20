use std::fs::File;

use aura_common::{AuraError, AuraResult, MemoryStats};

use crate::collectors::parsing::{parse_u64_strict, read_reused, split_whitespace};
use crate::collectors::ProviderOutcome;

use super::MemoryAvailability;

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

pub fn parse_meminfo_checked(buf: &[u8]) -> AuraResult<MemoryStats> {
    parse_meminfo_with_availability(buf).map(|(stats, _)| stats)
}

pub fn parse_meminfo_with_availability(
    buf: &[u8],
) -> AuraResult<(MemoryStats, MemoryAvailability)> {
    let mut stats = parse_meminfo(buf);
    stats.ram_total = required_meminfo_bytes(buf, b"MemTotal")?;
    stats.ram_free = required_meminfo_bytes(buf, b"MemFree")?;
    if stats.ram_total == 0 || stats.ram_free > stats.ram_total {
        return Err(AuraError::ParseError(
            "inconsistent core /proc/meminfo values".to_string(),
        ));
    }
    stats.ram_used = stats.ram_total - stats.ram_free;
    let buffers = optional_meminfo_bytes(buf, b"Buffers");
    let cached = optional_meminfo_bytes(buf, b"Cached");
    let swap_total = optional_meminfo_bytes(buf, b"SwapTotal");
    let swap_free = optional_meminfo_bytes(buf, b"SwapFree");
    stats.buffers = buffers.unwrap_or(0);
    stats.cached = cached.unwrap_or(0);
    let swap = matches!((swap_total, swap_free), (Some(total), Some(free)) if free <= total);
    if swap {
        stats.swap_total = swap_total.unwrap_or(0);
        stats.swap_free = swap_free.unwrap_or(0);
        stats.swap_used = stats.swap_total - stats.swap_free;
    } else {
        stats.swap_total = 0;
        stats.swap_free = 0;
        stats.swap_used = 0;
    }
    Ok((
        stats,
        MemoryAvailability {
            buffers: buffers.is_some(),
            cached: cached.is_some(),
            swap,
            page_faults: false,
        },
    ))
}

pub fn parse_vmstat_page_faults(buf: &[u8]) -> u64 {
    match parse_vmstat_page_faults_checked(buf) {
        ProviderOutcome::Available(value) => value,
        ProviderOutcome::Unavailable | ProviderOutcome::Fatal(_) => 0,
    }
}

pub fn parse_vmstat_page_faults_checked(buf: &[u8]) -> ProviderOutcome<u64> {
    for line in buf.split(|byte| *byte == b'\n') {
        let mut fields = split_whitespace(line);
        if fields.next() != Some(&b"pgfault"[..]) {
            continue;
        }
        return match fields.next().and_then(|value| parse_u64_strict(value).ok()) {
            Some(value) => ProviderOutcome::Available(value),
            None => ProviderOutcome::Fatal(AuraError::Fatal(
                "malformed /proc/vmstat pgfault counter".to_string(),
            )),
        };
    }
    ProviderOutcome::Fatal(AuraError::Fatal(
        "missing /proc/vmstat pgfault counter".to_string(),
    ))
}

pub fn classify_vmstat_error(error: std::io::Error) -> ProviderOutcome<u64> {
    match error.raw_os_error() {
        Some(libc::ENOENT) | Some(libc::EACCES) => ProviderOutcome::Unavailable,
        _ => ProviderOutcome::Fatal(AuraError::Fatal(format!(
            "/proc/vmstat read failed: {error}"
        ))),
    }
}

pub fn collect(
    meminfo_buf: &mut Vec<u8>,
    vmstat_buf: &mut Vec<u8>,
    out: &mut MemoryStats,
) -> AuraResult<MemoryAvailability> {
    meminfo_buf.clear();
    let mut meminfo = File::open("/proc/meminfo")?;
    read_reused(&mut meminfo, meminfo_buf)?;
    let (mut stats, mut availability) = parse_meminfo_with_availability(meminfo_buf)?;

    vmstat_buf.clear();
    let page_faults_available = match File::open("/proc/vmstat") {
        Ok(mut vmstat) => {
            if let Err(error) = read_reused(&mut vmstat, vmstat_buf) {
                match classify_vmstat_error(error) {
                    ProviderOutcome::Unavailable => false,
                    ProviderOutcome::Fatal(error) => return Err(error),
                    ProviderOutcome::Available(_) => unreachable!(),
                }
            } else {
                match parse_vmstat_page_faults_checked(vmstat_buf) {
                    ProviderOutcome::Available(value) => {
                        stats.page_faults = value;
                        true
                    }
                    ProviderOutcome::Unavailable => false,
                    ProviderOutcome::Fatal(error) => return Err(error),
                }
            }
        }
        Err(error) => match classify_vmstat_error(error) {
            ProviderOutcome::Unavailable => false,
            ProviderOutcome::Fatal(error) => return Err(error),
            ProviderOutcome::Available(_) => unreachable!(),
        },
    };

    stats.page_faults_per_sec = 0.0;
    if !page_faults_available {
        stats.page_faults = 0;
    }
    availability.page_faults = page_faults_available;
    *out = stats;
    Ok(availability)
}

#[cfg(test)]
fn calculate_page_fault_rate(current: u64, previous: u64, delta_secs: f64) -> f32 {
    if delta_secs > 0.0 {
        (current.saturating_sub(previous) as f64 / delta_secs) as f32
    } else {
        0.0
    }
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

fn required_meminfo_bytes(buf: &[u8], key: &[u8]) -> AuraResult<u64> {
    for line in buf.split(|byte| *byte == b'\n') {
        let Some(colon) = line.iter().position(|byte| *byte == b':') else {
            continue;
        };
        if &line[..colon] != key {
            continue;
        }
        let token = split_whitespace(&line[colon + 1..]).next().ok_or_else(|| {
            AuraError::ParseError("malformed core /proc/meminfo value".to_string())
        })?;
        if token.iter().any(|byte| !byte.is_ascii_digit()) {
            return Err(AuraError::ParseError(
                "malformed core /proc/meminfo value".to_string(),
            ));
        }
        return parse_u64_strict(token)?
            .checked_mul(1024)
            .ok_or_else(|| AuraError::ParseError("/proc/meminfo value overflow".to_string()));
    }
    Err(AuraError::ParseError(
        "missing mandatory /proc/meminfo field".to_string(),
    ))
}

fn optional_meminfo_bytes(buf: &[u8], key: &[u8]) -> Option<u64> {
    for line in buf.split(|byte| *byte == b'\n') {
        let Some(colon) = line.iter().position(|byte| *byte == b':') else {
            continue;
        };
        if &line[..colon] != key {
            continue;
        }
        let token = split_whitespace(&line[colon + 1..]).next()?;
        return parse_u64_strict(token).ok()?.checked_mul(1024);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::{calculate_page_fault_rate, parse_meminfo, parse_vmstat_page_faults};

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

    #[test]
    fn calculate_page_fault_rate_from_parsed_sample() {
        let fixture = include_bytes!("../../../tests/fixtures/proc_vmstat_sample.txt");
        let faults = parse_vmstat_page_faults(fixture);
        let rate = calculate_page_fault_rate(faults, 1000, 1.0);

        assert_eq!(rate, 66890.0);
    }
}
