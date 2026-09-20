use crate::error::AuraResult;
use crate::{ProcessStat, TelemetryArchive};

use super::{check_f32_range, check_nonempty_text, expect_zero, expect_zero_u8, fault, owned_u32};
use crate::archive::capabilities::{
    CAP_PROCESS_BLOCKED, CAP_PROCESS_RUNNING, CAP_PROCESS_SLEEPING, CAP_PROCESS_TOP_CPU,
    CAP_PROCESS_TOP_MEMORY, CAP_PROCESS_TOTAL, KNOWN_PROCESS_FLAGS_MASK,
};
use crate::archive::MAX_TOP_N;

const PROCESS_MASK: u64 = CAP_PROCESS_TOTAL
    | CAP_PROCESS_RUNNING
    | CAP_PROCESS_BLOCKED
    | CAP_PROCESS_SLEEPING
    | CAP_PROCESS_TOP_CPU
    | CAP_PROCESS_TOP_MEMORY;

fn validate_top_cpu_record(
    record: &ProcessStat,
    index: usize,
    owned: bool,
    listed: bool,
    max_usage: f32,
) -> AuraResult<()> {
    let base = format!("process.top_cpu[{index}]");
    if !owned {
        return expect_zero(bytemuck::bytes_of(record), &base);
    }
    if !listed {
        return Ok(());
    }
    check_f32_range(
        record.cpu_usage,
        &format!("{base}.cpu_usage"),
        0.0,
        max_usage,
    )?;
    check_nonempty_text(
        &record.comm.bytes,
        &format!("{base}.comm"),
        "process.top_cpu_count",
    )?;
    Ok(())
}

fn validate_top_mem_record(
    record: &ProcessStat,
    index: usize,
    owned: bool,
    listed: bool,
) -> AuraResult<()> {
    let base = format!("process.top_memory[{index}]");
    if !owned {
        return expect_zero(bytemuck::bytes_of(record), &base);
    }
    if !listed {
        return Ok(());
    }
    expect_zero(
        &record.cpu_usage.to_le_bytes(),
        &format!("{base}.unowned_cpu_usage"),
    )?;
    check_nonempty_text(
        &record.comm.bytes,
        &format!("{base}.comm"),
        "process.top_mem_count",
    )?;
    Ok(())
}

pub(super) fn validate(a: &TelemetryArchive) -> AuraResult<()> {
    let caps = a.capabilities;
    let p = &a.process;
    let has_total = caps & CAP_PROCESS_TOTAL != 0;
    let has_running = caps & CAP_PROCESS_RUNNING != 0;
    let has_blocked = caps & CAP_PROCESS_BLOCKED != 0;
    let has_sleeping = caps & CAP_PROCESS_SLEEPING != 0;
    let has_top_cpu = caps & CAP_PROCESS_TOP_CPU != 0;
    let has_top_mem = caps & CAP_PROCESS_TOP_MEMORY != 0;

    owned_u32(p.total, has_total, "process.total")?;
    owned_u32(p.running, has_running, "process.running")?;
    if has_total {
        let sum = p.running as u64 + p.blocked as u64 + p.sleeping as u64;
        if sum > p.total as u64 {
            return Err(fault(
                "process.running".to_string(),
                "inconsistent with process.total".to_string(),
            ));
        }
    }
    owned_u32(p.blocked, has_blocked, "process.blocked")?;
    owned_u32(p.sleeping, has_sleeping, "process.sleeping")?;

    let top_cpu_listed = (p.top_cpu_count as usize).min(MAX_TOP_N);
    let max_usage = a.cpu.core_count as f32 * 100.0;
    for (index, record) in p.top_cpu.iter().enumerate() {
        validate_top_cpu_record(
            record,
            index,
            has_top_cpu,
            index < top_cpu_listed,
            max_usage,
        )?;
    }
    let top_mem_listed = (p.top_mem_count as usize).min(MAX_TOP_N);
    for (index, record) in p.top_mem.iter().enumerate() {
        validate_top_mem_record(record, index, has_top_mem, index < top_mem_listed)?;
    }

    if !has_top_cpu {
        expect_zero_u8(p.top_cpu_count, "process.top_cpu_count")?;
    } else if p.top_cpu_count as usize > MAX_TOP_N {
        return Err(fault(
            "process.top_cpu_count".to_string(),
            format!("{} exceeds {MAX_TOP_N}", p.top_cpu_count),
        ));
    }
    if !has_top_mem {
        expect_zero_u8(p.top_mem_count, "process.top_mem_count")?;
    } else if p.top_mem_count as usize > MAX_TOP_N {
        return Err(fault(
            "process.top_mem_count".to_string(),
            format!("{} exceeds {MAX_TOP_N}", p.top_mem_count),
        ));
    }

    if caps & PROCESS_MASK == 0 {
        expect_zero_u8(p.flags, "process.flags")?;
    } else {
        let unknown = p.flags & !KNOWN_PROCESS_FLAGS_MASK;
        if unknown != 0 {
            return Err(fault(
                "process.flags".to_string(),
                format!("unknown bits 0x{unknown:02x}"),
            ));
        }
    }
    expect_zero(&p._pad0, "process.padding")?;
    Ok(())
}
