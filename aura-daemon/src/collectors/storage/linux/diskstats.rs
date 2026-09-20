//! `/proc/diskstats` parsing. Kernel record order is preserved; whole
//! devices and partitions are included while `loop`/`ram`/`dm-` pseudo
//! devices are skipped deterministically. Malformed individual lines skip
//! the record and set `disk_truncated`.

use aura_common::{AuraResult, DiskStat, FixedString16, StorageStats, MAX_DISKS};

use super::super::state::DiskRawSnapshot;
use super::super::EMPTY_DISK;
use crate::collectors::parsing::{parse_u64_strict, split_whitespace, trim_ascii};

/// Field positions in one diskstats record (0-based after whitespace split).
const FIELD_READS_COMPLETED: usize = 3;
const FIELD_SECTORS_READ: usize = 5;
const FIELD_READ_MS: usize = 6;
const FIELD_WRITES_COMPLETED: usize = 7;
const FIELD_SECTORS_WRITTEN: usize = 9;
const FIELD_WRITE_MS: usize = 10;
const FIELD_IN_FLIGHT: usize = 11;
const MIN_FIELDS: usize = 12;

const SECTOR_BYTES: u64 = 512;

pub fn parse_diskstats(
    buf: &[u8],
    out: &mut StorageStats,
    raw: &mut [DiskRawSnapshot; MAX_DISKS],
) -> AuraResult<()> {
    let mut count = 0usize;
    let mut truncated = 0u8;
    let mut line_start = 0usize;

    for i in 0..buf.len() {
        if buf[i] != b'\n' {
            continue;
        }
        let line = &buf[line_start..i];
        line_start = i + 1;
        if trim_ascii(line).is_empty() {
            continue;
        }
        parse_line(line, out, raw, &mut count, &mut truncated);
    }
    if line_start < buf.len() && !trim_ascii(&buf[line_start..]).is_empty() {
        // Unterminated tail record: the read was cut short.
        truncated = 1;
    }

    out.disk_count = count as u8;
    out.disk_truncated = truncated;
    for slot in out.disks.iter_mut().skip(count) {
        *slot = EMPTY_DISK;
    }
    for slot in raw.iter_mut().skip(count) {
        *slot = DiskRawSnapshot::zero();
    }
    Ok(())
}

fn parse_line(
    line: &[u8],
    out: &mut StorageStats,
    raw: &mut [DiskRawSnapshot; MAX_DISKS],
    count: &mut usize,
    truncated: &mut u8,
) {
    let mut fields: [&[u8]; MIN_FIELDS] = [&[]; MIN_FIELDS];
    let mut field_count = 0usize;
    for token in split_whitespace(line) {
        if field_count < MIN_FIELDS {
            fields[field_count] = token;
        }
        field_count += 1;
    }
    if field_count < MIN_FIELDS {
        *truncated = 1;
        return;
    }

    let (Some(major), Some(minor)) = (parse_u32(fields[0]), parse_u32(fields[1])) else {
        *truncated = 1;
        return;
    };
    let name = fields[2];
    if is_pseudo_device(name) {
        return;
    }
    if name.is_empty() || name.len() > 16 {
        *truncated = 1;
        return;
    }

    let mut counters = [0u64; 7];
    let positions = [
        FIELD_READS_COMPLETED,
        FIELD_SECTORS_READ,
        FIELD_READ_MS,
        FIELD_WRITES_COMPLETED,
        FIELD_SECTORS_WRITTEN,
        FIELD_WRITE_MS,
        FIELD_IN_FLIGHT,
    ];
    for (slot, position) in counters.iter_mut().zip(positions) {
        match parse_u64_strict(fields[position]) {
            Ok(value) => *slot = value,
            Err(_) => {
                *truncated = 1;
                return;
            }
        }
    }

    let Ok(in_flight) = u32::try_from(counters[6]) else {
        *truncated = 1;
        return;
    };
    let (Some(read_bytes), Some(write_bytes)) = (
        counters[1].checked_mul(SECTOR_BYTES),
        counters[4].checked_mul(SECTOR_BYTES),
    ) else {
        *truncated = 1;
        return;
    };
    if out.disks[..*count]
        .iter()
        .any(|disk| disk.major == major && disk.minor == minor)
    {
        *truncated = 1;
        return;
    }
    if *count >= MAX_DISKS {
        *truncated = 1;
        return;
    }

    out.disks[*count] = DiskStat {
        name: FixedString16::from_bytes(name),
        major,
        minor,
        read_bytes,
        write_bytes,
        read_bytes_per_sec: 0.0,
        write_bytes_per_sec: 0.0,
        read_iops: 0.0,
        write_iops: 0.0,
        queue_depth: in_flight,
        read_latency_ms: 0.0,
        write_latency_ms: 0.0,
        _pad0: [0; 4],
    };
    raw[*count] = DiskRawSnapshot {
        sectors_read: counters[1],
        sectors_written: counters[4],
        reads_completed: counters[0],
        read_ms: counters[2],
        writes_completed: counters[3],
        write_ms: counters[5],
    };
    *count += 1;
}

/// Deterministic skips: `loop`/`ram` immediately followed by a digit (covers
/// partition forms such as `loop0p1`) and `dm-` followed by a digit.
fn is_pseudo_device(name: &[u8]) -> bool {
    fn prefixed_with_digit(name: &[u8], prefix: &[u8]) -> bool {
        name.len() > prefix.len() && name.starts_with(prefix) && name[prefix.len()].is_ascii_digit()
    }
    prefixed_with_digit(name, b"loop")
        || prefixed_with_digit(name, b"ram")
        || prefixed_with_digit(name, b"dm-")
}

fn parse_u32(token: &[u8]) -> Option<u32> {
    parse_u64_strict(token)
        .ok()
        .and_then(|v| u32::try_from(v).ok())
}
