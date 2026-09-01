use crate::error::AuraResult;
use crate::{NetIfStat, TelemetryArchive};

use super::{
    check_bool, check_nonempty_text, check_rate, expect_zero, expect_zero_u8, fault, owned_f32,
    owned_u64, rate_path,
};
use crate::archive::capabilities::{CAP_NETWORK_BYTES, CAP_NETWORK_RATES};
use crate::archive::MAX_NETIFS;

fn validate_interface(iface: &NetIfStat, index: usize, listed: bool, caps: u64) -> AuraResult<()> {
    let base = format!("network.interfaces[{index}]");
    if !listed {
        return expect_zero(bytemuck::bytes_of(iface), &base);
    }
    check_nonempty_text(
        &iface.name.bytes,
        &format!("{base}.name"),
        "network.if_count",
    )?;
    let bytes_owned = caps & CAP_NETWORK_BYTES != 0;
    owned_u64(iface.rx_bytes, bytes_owned, &format!("{base}.rx_bytes"))?;
    owned_u64(iface.tx_bytes, bytes_owned, &format!("{base}.tx_bytes"))?;
    let rates_owned = caps & CAP_NETWORK_RATES != 0;
    owned_f32(
        iface.rx_bytes_per_sec,
        rates_owned,
        &rate_path(&base, "rx_bytes"),
        check_rate,
    )?;
    owned_f32(
        iface.tx_bytes_per_sec,
        rates_owned,
        &rate_path(&base, "tx_bytes"),
        check_rate,
    )?;
    Ok(())
}

pub(super) fn validate(a: &TelemetryArchive) -> AuraResult<()> {
    let caps = a.capabilities;
    let n = &a.network;
    let any_net = caps & (CAP_NETWORK_BYTES | CAP_NETWORK_RATES) != 0;

    let listed = (n.if_count as usize).min(MAX_NETIFS);
    for (index, iface) in n.interfaces.iter().enumerate() {
        validate_interface(iface, index, any_net && index < listed, caps)?;
    }
    if !any_net {
        expect_zero_u8(n.if_count, "network.if_count")?;
        expect_zero_u8(n.truncated, "network.truncated")?;
    } else {
        if n.if_count as usize > MAX_NETIFS {
            return Err(fault(
                "network.if_count".to_string(),
                format!("{} exceeds {MAX_NETIFS}", n.if_count),
            ));
        }
        check_bool(n.truncated, "network.truncated")?;
    }
    expect_zero(&n._pad0, "network.padding")?;
    Ok(())
}
