use std::fs::File;
use std::io::Read;
use std::sync::OnceLock;

use aura_common::{AuraResult, CpuCoreStat, CpuGlobalStat, MAX_CORES};
use log::warn;

use crate::collectors::parsing::{parse_u64, split_whitespace};
use crate::collectors::CpuTickSnapshot;

static CORE_LIMIT_WARNED: OnceLock<()> = OnceLock::new();

pub fn parse_cpu_stat(buf: &[u8]) -> AuraResult<(u64, u64, u64, u64, u64)> {
    let mut user = 0u64;
    let mut nice = 0u64;
    let mut system = 0u64;
    let mut idle = 0u64;
    let mut iowait = 0u64;
    let mut irq = 0u64;
    let mut softirq = 0u64;
    let mut steal = 0u64;
    let mut ctxt = 0u64;

    let mut line_start = 0usize;
    for i in 0..buf.len() {
        if buf[i] != b'\n' {
            continue;
        }
        let line = &buf[line_start..i];
        line_start = i + 1;

        if line.starts_with(b"cpu ") {
            let fields = &line[4..];
            for (idx, field) in split_whitespace(fields).enumerate() {
                let v = parse_u64(field)?;
                match idx {
                    0 => user = v,
                    1 => nice = v,
                    2 => system = v,
                    3 => idle = v,
                    4 => iowait = v,
                    5 => irq = v,
                    6 => softirq = v,
                    7 => steal = v,
                    _ => {}
                }
            }
        } else if line.starts_with(b"ctxt ") {
            ctxt = parse_u64(&line[5..])?;
        }
    }

    let total = user + nice + system + idle + iowait + irq + softirq + steal;
    Ok((user, system, idle, total, ctxt))
}

pub fn parse_core_stats(
    buf: &[u8],
    out_cores: &mut [CpuCoreStat; MAX_CORES],
    core_count: &mut u8,
) -> AuraResult<()> {
    let mut line_start = 0usize;
    let mut count = 0usize;

    for i in 0..buf.len() {
        if buf[i] != b'\n' {
            continue;
        }
        let line = &buf[line_start..i];
        line_start = i + 1;

        if count >= MAX_CORES {
            break;
        }

        if line.len() < 5 || &line[0..3] != b"cpu" || !line[3].is_ascii_digit() {
            continue;
        }

        let mut field_start = 3;
        while field_start < line.len() && line[field_start].is_ascii_digit() {
            field_start += 1;
        }
        while field_start < line.len() && line[field_start] == b' ' {
            field_start += 1;
        }

        let mut fields = [0u64; 8];
        for (fi, field) in split_whitespace(&line[field_start..]).enumerate() {
            if fi >= fields.len() {
                break;
            }
            fields[fi] = parse_u64(field)?;
        }

        let user = fields[0];
        let system = fields[2];
        let idle = fields[3];
        let total = fields.iter().copied().sum();

        out_cores[count] = CpuCoreStat {
            core_index: count as u8,
            _pad0: [0; 7],
            user_ticks: user,
            system_ticks: system,
            idle_ticks: idle,
            total_ticks: total,
            usage_percent: if total > 0 {
                (((total - idle) as f64 / total as f64) * 100.0) as f32
            } else {
                0.0
            },
            _pad1: [0; 4],
        };
        count += 1;
    }

    if count >= MAX_CORES && CORE_LIMIT_WARNED.get().is_none() {
        warn!(
            "CPU core limit reached: {} cores detected (MAX_CORES={}). \
            Run 'cat /proc/cpuinfo' to see all cores.",
            count, MAX_CORES
        );
        CORE_LIMIT_WARNED.set(()).ok();
    }

    *core_count = count as u8;
    Ok(())
}

pub fn collect(
    buf: &mut Vec<u8>,
    out: &mut CpuGlobalStat,
    prev: &mut CpuTickSnapshot,
    delta_secs: f64,
) -> AuraResult<()> {
    buf.clear();
    let mut file = File::open("/proc/stat")?;
    file.read_to_end(buf)?;
    let data = &buf[..];

    let (user, system, idle, total, ctxt) = parse_cpu_stat(data)?;

    let delta_total = total.saturating_sub(prev.total);
    let delta_idle = idle.saturating_sub(prev.idle);
    let delta_ctxt = ctxt.saturating_sub(prev.context_switches);

    prev.user = user;
    prev.system = system;
    prev.idle = idle;
    prev.total = total;
    prev.context_switches = ctxt;

    out.user_ticks = user;
    out.system_ticks = system;
    out.idle_ticks = idle;
    out.total_ticks = total;
    out.context_switches = ctxt;
    out.context_switches_per_sec = if delta_secs > 0.0 {
        (delta_ctxt as f64 / delta_secs) as f32
    } else {
        0.0
    };
    out.usage_percent = if delta_total > 0 {
        let busy = delta_total.saturating_sub(delta_idle);
        ((busy as f64 / delta_total as f64) * 100.0) as f32
    } else {
        0.0
    };

    parse_core_stats(data, &mut out.cores, &mut out.core_count)
}

#[cfg(test)]
mod tests {
    use super::{parse_core_stats, parse_cpu_stat};
    use aura_common::{CpuCoreStat, MAX_CORES};

    #[test]
    fn parse_global_cpu_and_ctxt() {
        let fixture = include_bytes!("../../../tests/fixtures/proc_stat_sample.txt");
        let (user, system, idle, total, ctxt) = parse_cpu_stat(fixture).expect("parse");
        assert_eq!(user, 2255);
        assert_eq!(system, 2290);
        assert_eq!(idle, 22625563);
        assert!(total > idle);
        assert_eq!(ctxt, 1990473);
    }

    #[test]
    fn parse_core_rows() {
        let fixture = include_bytes!("../../../tests/fixtures/proc_stat_sample.txt");
        let mut cores = [CpuCoreStat {
            core_index: 0,
            _pad0: [0; 7],
            user_ticks: 0,
            system_ticks: 0,
            idle_ticks: 0,
            total_ticks: 0,
            usage_percent: 0.0,
            _pad1: [0; 4],
        }; MAX_CORES];
        let mut count = 0u8;
        parse_core_stats(fixture, &mut cores, &mut count).expect("parse");
        assert_eq!(count, 2);
        assert_eq!(cores[0].core_index, 0);
        assert!(cores[0].total_ticks > 0);
    }
}
