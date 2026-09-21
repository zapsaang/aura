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

pub(super) fn os_icon(os_id: &str, os_type: &str) -> &'static str {
    if os_type.eq_ignore_ascii_case("darwin") {
        return "\u{f302}";
    }

    match os_id {
        "ubuntu" => "\u{f31b}",
        "debian" => "\u{f306}",
        "arch" => "\u{f303}",
        "fedora" => "\u{f30a}",
        "rhel" => "\u{f316}",
        "centos" => "\u{f304}",
        "rocky" => "\u{f32b}",
        "alma" => "\u{f31d}",
        "opensuse" => "\u{f314}",
        "opensuse-leap" => "\u{f37e}",
        "opensuse-tumbleweed" => "\u{f37d}",
        "gentoo" => "\u{f30d}",
        "alpine" => "\u{f300}",
        "nixos" => "\u{f313}",
        "void" => "\u{f322}",
        "linuxmint" | "mint" => "\u{f30e}",
        "manjaro" => "\u{f312}",
        "endeavouros" => "\u{f323}",
        "pop" | "pop_os" => "\u{f32a}",
        "zorin" => "\u{f32f}",
        "kali" => "\u{f327}",
        "raspbian" => "\u{f315}",
        "amzn" => "\u{f270}",
        "solus" => "\u{f32d}",
        "aos\x63" => "\u{f301}",
        "archlabs" => "\u{f31e}",
        "artix" => "\u{f31f}",
        "devuan" => "\u{f307}",
        "elementary" => "\u{f309}",
        "mageia" => "\u{f310}",
        "mandriva" => "\u{f311}",
        "sabayon" => "\u{f317}",
        "slackware" => "\u{f318}",
        "deepin" => "\u{f321}",
        "guix" => "\u{f325}",
        "parrot" => "\u{f329}",
        "garuda" => "\u{f335}",
        "kubuntu" => "\u{f333}",
        "neon" => "\u{f331}",
        "nobara" => "\u{f380}",
        "openwrt" => "\u{f382}",
        "cachyos" => "\u{f385}",
        "coreos" => "\u{f305}",
        "ol" | "oracle" | "flatcar" | "container-linux" | "clearlinux" | "photon" => "\u{f31a}",
        _ => "\u{f31a}",
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

#[cfg(test)]
mod tests {
    use super::os_icon;

    #[test]
    fn os_icon_returns_apple_when_real_darwin_shape() {
        assert_eq!(os_icon("macos", "Darwin"), "\u{f302}");
    }

    #[test]
    fn os_icon_returns_ubuntu_when_ubuntu_id() {
        assert_eq!(os_icon("ubuntu", "linux"), "\u{f31b}");
    }

    #[test]
    fn os_icon_returns_coreos_when_coreos_id() {
        assert_eq!(os_icon("coreos", "linux"), "\u{f305}");
    }

    #[test]
    fn os_icon_returns_cfb3b18_glyph_when_baseline_id() {
        let cases = [
            ("debian", "\u{f306}"),
            ("arch", "\u{f303}"),
            ("fedora", "\u{f30a}"),
            ("rhel", "\u{f316}"),
            ("centos", "\u{f304}"),
            ("rocky", "\u{f32b}"),
            ("alma", "\u{f31d}"),
            ("opensuse", "\u{f314}"),
            ("gentoo", "\u{f30d}"),
            ("alpine", "\u{f300}"),
            ("nixos", "\u{f313}"),
            ("void", "\u{f322}"),
            ("manjaro", "\u{f312}"),
            ("endeavouros", "\u{f323}"),
            ("zorin", "\u{f32f}"),
            ("kali", "\u{f327}"),
            ("raspbian", "\u{f315}"),
            ("amzn", "\u{f270}"),
        ];

        for (os_id, expected) in cases {
            assert_eq!(os_icon(os_id, "linux"), expected, "{os_id}");
        }
    }

    #[test]
    fn os_icon_returns_expected_glyph_when_alias_or_extended_id() {
        let cases = [
            ("linuxmint", "\u{f30e}"),
            ("mint", "\u{f30e}"),
            ("pop", "\u{f32a}"),
            ("pop_os", "\u{f32a}"),
            ("opensuse-leap", "\u{f37e}"),
            ("opensuse-tumbleweed", "\u{f37d}"),
            ("solus", "\u{f32d}"),
            ("aos\x63", "\u{f301}"),
            ("archlabs", "\u{f31e}"),
            ("artix", "\u{f31f}"),
            ("devuan", "\u{f307}"),
            ("elementary", "\u{f309}"),
            ("mageia", "\u{f310}"),
            ("mandriva", "\u{f311}"),
            ("sabayon", "\u{f317}"),
            ("slackware", "\u{f318}"),
            ("deepin", "\u{f321}"),
            ("guix", "\u{f325}"),
            ("parrot", "\u{f329}"),
            ("garuda", "\u{f335}"),
            ("kubuntu", "\u{f333}"),
            ("neon", "\u{f331}"),
            ("nobara", "\u{f380}"),
            ("openwrt", "\u{f382}"),
            ("cachyos", "\u{f385}"),
            ("ol", "\u{f31a}"),
            ("oracle", "\u{f31a}"),
            ("flatcar", "\u{f31a}"),
            ("container-linux", "\u{f31a}"),
            ("clearlinux", "\u{f31a}"),
            ("photon", "\u{f31a}"),
        ];

        for (os_id, expected) in cases {
            assert_eq!(os_icon(os_id, "linux"), expected, "{os_id}");
        }
    }

    #[test]
    fn os_icon_returns_tux_when_id_is_unknown() {
        assert_eq!(os_icon("unknown", "linux"), "\u{f31a}");
    }
}
