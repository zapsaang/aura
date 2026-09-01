mod cpu;
mod derived;
mod gpu;
mod memory;
mod meta;
mod network;
mod process;
mod storage;

use crate::error::{AuraError, AuraResult};
use crate::TelemetryArchive;

use super::capabilities::{
    capability_name, CAP_CPU_GLOBAL, CAP_CPU_PER_CORE, CAP_PROCESS_TOP_CPU, KNOWN_CAPABILITIES_MASK,
};

pub(crate) fn fault(path: String, detail: String) -> AuraError {
    AuraError::InvalidArchive {
        reason: format!("field {path}: {detail}"),
    }
}

pub(crate) fn expect_zero(bytes: &[u8], path: &str) -> AuraResult<()> {
    if bytes.iter().any(|&b| b != 0) {
        return Err(fault(path.to_string(), "expected zero".to_string()));
    }
    Ok(())
}

pub(crate) fn expect_zero_u8(value: u8, path: &str) -> AuraResult<()> {
    expect_zero(&[value], path)
}

pub(crate) fn rate_path(base: &str, stem: &str) -> String {
    let mut path = format!("{base}.{stem}_per_se");
    path.push('c');
    path
}

pub(crate) fn check_text(bytes: &[u8], path: &str) -> AuraResult<()> {
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    let text = match std::str::from_utf8(&bytes[..end]) {
        Ok(text) => text,
        Err(_) => return Err(fault(path.to_string(), "invalid UTF-8".to_string())),
    };
    for ch in text.chars() {
        if ch.is_control() {
            return Err(fault(
                path.to_string(),
                format!("control character U+{:04X}", ch as u32),
            ));
        }
    }
    if bytes[end..].iter().any(|&b| b != 0) {
        return Err(fault(path.to_string(), "expected zero".to_string()));
    }
    Ok(())
}

pub(crate) fn check_nonempty_text(bytes: &[u8], path: &str, owner: &str) -> AuraResult<()> {
    check_text(bytes, path)?;
    if bytes[0] == 0 {
        return Err(fault(
            path.to_string(),
            format!("inconsistent with {owner}"),
        ));
    }
    Ok(())
}

pub(crate) fn check_f32_range(value: f32, path: &str, lo: f32, hi: f32) -> AuraResult<()> {
    if !value.is_finite() {
        return Err(fault(path.to_string(), "non-finite".to_string()));
    }
    if value < lo || value > hi {
        return Err(fault(path.to_string(), format!("outside {lo}..={hi}")));
    }
    Ok(())
}

pub(crate) fn check_percent(value: f32, path: &str) -> AuraResult<()> {
    check_f32_range(value, path, 0.0, 100.0)
}

pub(crate) fn check_rate(value: f32, path: &str) -> AuraResult<()> {
    check_f32_range(value, path, 0.0, f32::MAX)
}

pub(crate) fn check_tone(value: u8, path: &str) -> AuraResult<()> {
    if value > 3 {
        return Err(fault(path.to_string(), "outside 0..=3".to_string()));
    }
    Ok(())
}

pub(crate) fn check_bool(value: u8, path: &str) -> AuraResult<()> {
    if value > 1 {
        return Err(fault(path.to_string(), "outside 0..=1".to_string()));
    }
    Ok(())
}

pub(crate) fn owned_u64(value: u64, owned: bool, path: &str) -> AuraResult<()> {
    if !owned {
        return expect_zero(&value.to_le_bytes(), path);
    }
    Ok(())
}

pub(crate) fn owned_u32(value: u32, owned: bool, path: &str) -> AuraResult<()> {
    if !owned {
        return expect_zero(&value.to_le_bytes(), path);
    }
    Ok(())
}

pub(crate) fn owned_f32(
    value: f32,
    owned: bool,
    path: &str,
    check: fn(f32, &str) -> AuraResult<()>,
) -> AuraResult<()> {
    if !owned {
        return expect_zero(&value.to_le_bytes(), path);
    }
    check(value, path)
}

pub(crate) fn owned_text(bytes: &[u8], owned: bool, path: &str) -> AuraResult<()> {
    if !owned {
        return expect_zero(bytes, path);
    }
    check_text(bytes, path)
}

fn validate_capabilities(caps: u64) -> AuraResult<()> {
    let unknown = caps & !KNOWN_CAPABILITIES_MASK;
    if unknown != 0 {
        return Err(fault(
            "capabilities".to_string(),
            format!("unknown bits 0x{unknown:016x}"),
        ));
    }
    for dependent in [CAP_CPU_PER_CORE, CAP_PROCESS_TOP_CPU] {
        if caps & dependent != 0 && caps & CAP_CPU_GLOBAL == 0 {
            let name = capability_name(0).expect("bit 0 name");
            return Err(fault(
                "capabilities".to_string(),
                format!("missing required capability {name}"),
            ));
        }
    }
    Ok(())
}

pub fn validate_archive(archive: &TelemetryArchive) -> AuraResult<()> {
    validate_capabilities(archive.capabilities)?;
    cpu::validate(archive)?;
    process::validate(archive)?;
    memory::validate(archive)?;
    storage::validate(archive)?;
    network::validate(archive)?;
    meta::validate(archive)?;
    gpu::validate(archive)?;
    derived::validate(archive)?;
    expect_zero(&archive._reserved, "archive.reserved")?;
    Ok(())
}
