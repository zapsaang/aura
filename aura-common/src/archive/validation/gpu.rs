use crate::error::AuraResult;
use crate::{GpuStat, TelemetryArchive};

use super::{
    check_bool, check_f32_range, check_nonempty_text, check_tone, expect_zero, expect_zero_u8,
    fault, owned_f32, owned_u64,
};
use crate::archive::capabilities::{
    CAP_GPU_ENUMERATION, GPU_CAP_MEMORY_TOTAL, GPU_CAP_MEMORY_USED, GPU_CAP_NAME, GPU_CAP_POWER,
    GPU_CAP_TEMPERATURE, GPU_CAP_UTILIZATION, KNOWN_GPU_RECORD_MASK,
};
use crate::archive::MAX_GPUS;

fn validate_record(record: &GpuStat, index: usize, listed: bool) -> AuraResult<()> {
    let base = format!("gpu.gpus[{index}]");
    if !listed {
        return expect_zero(bytemuck::bytes_of(record), &base);
    }
    let rcaps = record.capabilities;
    if rcaps & GPU_CAP_NAME != 0 {
        check_nonempty_text(
            &record.name.bytes,
            &format!("{base}.name"),
            &format!("{base}.capabilities"),
        )?;
    } else {
        expect_zero(&record.name.bytes, &format!("{base}.name"))?;
    }
    let total_owned = rcaps & GPU_CAP_MEMORY_TOTAL != 0;
    let used_owned = rcaps & GPU_CAP_MEMORY_USED != 0;
    owned_u64(
        record.memory_total,
        total_owned,
        &format!("{base}.memory_total"),
    )?;
    if !used_owned {
        expect_zero(
            &record.memory_used.to_le_bytes(),
            &format!("{base}.memory_used"),
        )?;
    } else if total_owned && record.memory_used > record.memory_total {
        return Err(fault(
            format!("{base}.memory_used"),
            format!("{} exceeds {}", record.memory_used, record.memory_total),
        ));
    }
    owned_f32(
        record.utilization_percent,
        rcaps & GPU_CAP_UTILIZATION != 0,
        &format!("{base}.utilization_percent"),
        |v, p| check_f32_range(v, p, 0.0, 100.0),
    )?;
    owned_f32(
        record.power_watts,
        rcaps & GPU_CAP_POWER != 0,
        &format!("{base}.power_watts"),
        |v, p| check_f32_range(v, p, 0.0, f32::MAX),
    )?;
    let temp_owned = rcaps & GPU_CAP_TEMPERATURE != 0;
    if !temp_owned {
        expect_zero(
            &record.temperature_celsius.to_le_bytes(),
            &format!("{base}.temperature_celsius"),
        )?;
    } else if !(-273..=1000).contains(&record.temperature_celsius) {
        return Err(fault(
            format!("{base}.temperature_celsius"),
            "outside -273..=1000".to_string(),
        ));
    }
    check_bool(record.available, &format!("{base}.available"))?;
    if !temp_owned {
        expect_zero_u8(record.tone, &format!("{base}.tone"))?;
    } else {
        check_tone(record.tone, &format!("{base}.tone"))?;
    }
    expect_zero(&record._pad0, &format!("{base}.padding"))?;
    let unknown = rcaps & !KNOWN_GPU_RECORD_MASK;
    if unknown != 0 {
        return Err(fault(
            format!("{base}.capabilities"),
            format!("unknown bits 0x{unknown:016x}"),
        ));
    }
    Ok(())
}

pub(super) fn validate(a: &TelemetryArchive) -> AuraResult<()> {
    let g = &a.gpu;
    let owned = a.capabilities & CAP_GPU_ENUMERATION != 0;
    let listed = (g.gpu_count as usize).min(MAX_GPUS);
    for (index, record) in g.gpus.iter().enumerate() {
        validate_record(record, index, owned && index < listed)?;
    }
    if !owned {
        expect_zero_u8(g.gpu_count, "gpu.gpu_count")?;
        expect_zero_u8(g.nvml_available, "gpu.nvml_available")?;
        expect_zero_u8(g.truncated, "gpu.truncated")?;
    } else {
        if g.gpu_count as usize > MAX_GPUS {
            return Err(fault(
                "gpu.gpu_count".to_string(),
                format!("{} exceeds {MAX_GPUS}", g.gpu_count),
            ));
        }
        check_bool(g.nvml_available, "gpu.nvml_available")?;
        check_bool(g.truncated, "gpu.truncated")?;
    }
    expect_zero(&g._pad0, "gpu.padding")?;
    Ok(())
}
