use crate::error::AuraResult;
use crate::TelemetryArchive;

use super::{
    check_percent, check_rate, check_tone, expect_zero, expect_zero_u8, owned_f32, rate_path,
};
use crate::archive::capabilities::{
    CAP_CPU_GLOBAL, CAP_MEMORY_RAM_TOTAL, CAP_MEMORY_RAM_USED, CAP_MEMORY_SWAP, CAP_NETWORK_BYTES,
    CAP_NETWORK_RATES,
};

pub(super) fn validate(a: &TelemetryArchive) -> AuraResult<()> {
    let caps = a.capabilities;
    let d = &a.derived;
    let ram_prereq = caps & CAP_MEMORY_RAM_TOTAL != 0 && caps & CAP_MEMORY_RAM_USED != 0;
    let swap_prereq = caps & CAP_MEMORY_SWAP != 0;
    let net_prereq = caps & CAP_NETWORK_BYTES != 0 && caps & CAP_NETWORK_RATES != 0;
    let cpu_prereq = caps & CAP_CPU_GLOBAL != 0;

    owned_f32(
        d.ram_used_percent,
        ram_prereq,
        "derived.ram_used_percent",
        check_percent,
    )?;
    owned_f32(
        d.swap_used_percent,
        swap_prereq,
        "derived.swap_used_percent",
        check_percent,
    )?;
    owned_f32(
        d.aggregate_rx_bytes_per_sec,
        net_prereq,
        &rate_path("derived", "aggregate_rx_bytes"),
        check_rate,
    )?;
    owned_f32(
        d.aggregate_tx_bytes_per_sec,
        net_prereq,
        &rate_path("derived", "aggregate_tx_bytes"),
        check_rate,
    )?;
    if cpu_prereq {
        check_tone(d.cpu_tone, "derived.cpu_tone")?;
    } else {
        expect_zero_u8(d.cpu_tone, "derived.cpu_tone")?;
    }
    if ram_prereq {
        check_tone(d.ram_tone, "derived.ram_tone")?;
    } else {
        expect_zero_u8(d.ram_tone, "derived.ram_tone")?;
    }
    if swap_prereq {
        check_tone(d.swap_tone, "derived.swap_tone")?;
    } else {
        expect_zero_u8(d.swap_tone, "derived.swap_tone")?;
    }
    expect_zero_u8(d._reserved0, "derived.reserved")?;
    expect_zero(&d._pad0, "derived.padding")?;
    Ok(())
}
