use aura_common::{
    bytes_to_string, TelemetryArchive, CAP_META_LOAD_AVERAGE, CAP_META_OS_CODENAME,
    CAP_META_OS_IDENTITY, CAP_META_OS_VERSION, CAP_META_OS_VERSION_ID, CAP_META_TIMEZONE,
    CAP_META_UPTIME, CAP_META_WALLCLOCK,
};

use crate::args::ColorMode;

use super::color::NA;

fn token(out: &mut String, label: &str, owned: bool, rendered: String) {
    out.push_str(label);
    if owned {
        out.push_str(&rendered);
    } else {
        out.push_str(NA);
    }
}

pub fn render(_color: ColorMode, t: &TelemetryArchive) -> String {
    let caps = t.capabilities;
    let m = &t.meta;
    let mut out = String::from("META\n");

    token(
        &mut out,
        "  wallclock_ns: ",
        caps & CAP_META_WALLCLOCK != 0,
        m.wallclock_ns.to_string(),
    );
    token(
        &mut out,
        "\n  uptime: ",
        caps & CAP_META_UPTIME != 0,
        format!("{}s", m.uptime_secs),
    );

    out.push_str("\n  load: ");
    if caps & CAP_META_LOAD_AVERAGE != 0 {
        out.push_str(&format!(
            "{:.2} {:.2} {:.2}",
            m.load_avg_1m, m.load_avg_5m, m.load_avg_15m
        ));
    } else {
        out.push_str(NA);
        out.push(' ');
        out.push_str(NA);
        out.push(' ');
        out.push_str(NA);
    }

    out.push_str("\n  timezone: ");
    if caps & CAP_META_TIMEZONE != 0 {
        out.push_str(&format!(
            "{} ({})",
            bytes_to_string(&m.timezone_name),
            m.timezone_offset_secs
        ));
    } else {
        out.push_str(NA);
        out.push_str(" (");
        out.push_str(NA);
        out.push(')');
    }

    out.push_str("\n  os: ");
    let identity = caps & CAP_META_OS_IDENTITY != 0;
    if identity {
        out.push_str(&bytes_to_string(&m.os.os_pretty_name));
    } else {
        out.push_str(NA);
    }
    out.push_str(" id=");
    if identity {
        out.push_str(m.os.os_id.as_str());
    } else {
        out.push_str(NA);
    }
    token(
        &mut out,
        " version=",
        caps & CAP_META_OS_VERSION != 0,
        bytes_to_string(&m.os.version),
    );
    token(
        &mut out,
        " version_id=",
        caps & CAP_META_OS_VERSION_ID != 0,
        m.os.os_version_id.as_str().to_string(),
    );
    token(
        &mut out,
        " codename=",
        caps & CAP_META_OS_CODENAME != 0,
        m.os.version_codename.as_str().to_string(),
    );

    out
}
