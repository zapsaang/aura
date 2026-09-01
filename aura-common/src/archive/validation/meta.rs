use crate::error::AuraResult;
use crate::TelemetryArchive;

use super::{
    check_nonempty_text, check_rate, expect_zero, fault, owned_f32, owned_text, owned_u64,
};
use crate::archive::capabilities::{
    CAP_META_LOAD_AVERAGE, CAP_META_OS_CODENAME, CAP_META_OS_IDENTITY, CAP_META_OS_VERSION,
    CAP_META_OS_VERSION_ID, CAP_META_TIMEZONE, CAP_META_UPTIME, CAP_META_WALLCLOCK,
};

pub(super) fn validate(a: &TelemetryArchive) -> AuraResult<()> {
    let caps = a.capabilities;
    let m = &a.meta;

    if m.timestamp_ns == 0 {
        return Err(fault(
            "meta.timestamp_ns".to_string(),
            "outside 1..=18446744073709551615".to_string(),
        ));
    }
    owned_u64(
        m.wallclock_ns,
        caps & CAP_META_WALLCLOCK != 0,
        "meta.wallclock_ns",
    )?;
    owned_u64(
        m.uptime_secs,
        caps & CAP_META_UPTIME != 0,
        "meta.uptime_secs",
    )?;
    let load_owned = caps & CAP_META_LOAD_AVERAGE != 0;
    owned_f32(m.load_avg_1m, load_owned, "meta.load_avg_1m", check_rate)?;
    owned_f32(m.load_avg_5m, load_owned, "meta.load_avg_5m", check_rate)?;
    owned_f32(m.load_avg_15m, load_owned, "meta.load_avg_15m", check_rate)?;
    let tz_owned = caps & CAP_META_TIMEZONE != 0;
    owned_text(&m.timezone_name, tz_owned, "meta.timezone_name")?;
    if !tz_owned {
        expect_zero(
            &m.timezone_offset_secs.to_le_bytes(),
            "meta.timezone_offset_secs",
        )?;
    } else if !(-86_400..=86_400).contains(&m.timezone_offset_secs) {
        return Err(fault(
            "meta.timezone_offset_secs".to_string(),
            "outside -86400..=86400".to_string(),
        ));
    }

    let identity = caps & CAP_META_OS_IDENTITY != 0;
    let os = &m.os;
    if identity {
        check_nonempty_text(&os.os_type.bytes, "meta.os.os_type", "capabilities")?;
        check_nonempty_text(&os.os_id.bytes, "meta.os.os_id", "capabilities")?;
    } else {
        expect_zero(&os.os_type.bytes, "meta.os.os_type")?;
        expect_zero(&os.os_id.bytes, "meta.os.os_id")?;
    }
    owned_text(
        &os.os_version_id.bytes,
        caps & CAP_META_OS_VERSION_ID != 0,
        "meta.os.os_version_id",
    )?;
    owned_text(
        &os.version_codename.bytes,
        caps & CAP_META_OS_CODENAME != 0,
        "meta.os.version_codename",
    )?;
    owned_text(
        &os.version,
        caps & CAP_META_OS_VERSION != 0,
        "meta.os.version",
    )?;
    if identity {
        check_nonempty_text(&os.os_pretty_name, "meta.os.os_pretty_name", "capabilities")?;
    } else {
        expect_zero(&os.os_pretty_name, "meta.os.os_pretty_name")?;
    }
    Ok(())
}
