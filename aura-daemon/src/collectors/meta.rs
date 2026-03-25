use std::fs::File;
use std::io::Read;

use aura_common::{AuraResult, FixedString16, MetaStats, OsFingerprint};

pub fn cache_os_fingerprint(meta: &mut MetaStats) -> AuraResult<()> {
    let mut os = OsFingerprint {
        os_type: FixedString16::from_bytes(b"linux"),
        os_id: FixedString16::new(),
        os_version_id: FixedString16::new(),
        os_pretty_name: [0; 128],
    };

    if let Ok(buf) = std::fs::read("/etc/os-release") {
        parse_os_release(&buf, &mut os);
    }

    meta.os = os;
    Ok(())
}

pub fn collect(meta: &mut MetaStats) -> AuraResult<()> {
    meta.timestamp_ns = monotonic_ns();

    let mut buf = [0u8; 4096];

    if let Ok(mut f) = File::open("/proc/uptime") {
        let n = f.read(&mut buf)?;
        meta.uptime_secs = parse_first_f64_to_u64(&buf[..n]);
    }

    if let Ok(mut f) = File::open("/proc/loadavg") {
        let n = f.read(&mut buf)?;
        parse_loadavg(&buf[..n], meta);
    }

    let (name, offset_secs) = timezone_info();
    meta.timezone_name = [0; 8];
    let n = name.len().min(8);
    meta.timezone_name[..n].copy_from_slice(&name[..n]);
    meta.timezone_offset_secs = offset_secs;

    Ok(())
}

pub fn parse_os_release(buf: &[u8], out: &mut OsFingerprint) {
    let mut line_start = 0usize;
    for i in 0..=buf.len() {
        if i < buf.len() && buf[i] != b'\n' {
            continue;
        }
        let line = &buf[line_start..i];
        line_start = i + 1;
        if line.is_empty() {
            continue;
        }
        let Some(eq) = line.iter().position(|&c| c == b'=') else {
            continue;
        };
        let key = &line[..eq];
        let val = trim_quote(trim_ascii(&line[eq + 1..]));

        if key == b"ID" {
            out.os_id = FixedString16::from_bytes(val);
        } else if key == b"VERSION_ID" {
            out.os_version_id = FixedString16::from_bytes(val);
        } else if key == b"PRETTY_NAME" {
            let n = val.len().min(128);
            out.os_pretty_name[..n].copy_from_slice(&val[..n]);
        }
    }
}

fn parse_loadavg(buf: &[u8], meta: &mut MetaStats) {
    let mut idx = 0usize;
    for (i, tok) in split_whitespace(buf).enumerate() {
        let v = parse_f32(tok);
        if i == 0 {
            meta.load_avg_1m = v;
            idx += 1;
        } else if i == 1 {
            meta.load_avg_5m = v;
            idx += 1;
        } else if i == 2 {
            meta.load_avg_15m = v;
            idx += 1;
            break;
        }
    }
    if idx < 3 {
        meta.load_avg_1m = 0.0;
        meta.load_avg_5m = 0.0;
        meta.load_avg_15m = 0.0;
    }
}

fn parse_f32(b: &[u8]) -> f32 {
    let mut result = 0.0f32;
    let mut frac_div = 1.0f32;
    let mut after_dot = false;
    for &c in b {
        if c == b'.' {
            after_dot = true;
            continue;
        }
        if !c.is_ascii_digit() {
            break;
        }
        let d = (c - b'0') as f32;
        if after_dot {
            frac_div *= 10.0;
            result += d / frac_div;
        } else {
            result = result * 10.0 + d;
        }
    }
    result
}

fn parse_first_f64_to_u64(b: &[u8]) -> u64 {
    let mut int = 0u64;
    for &c in b {
        if c == b'.' || c.is_ascii_whitespace() {
            break;
        }
        if c.is_ascii_digit() {
            int = int.saturating_mul(10).saturating_add((c - b'0') as u64);
        }
    }
    int
}

fn trim_ascii(mut b: &[u8]) -> &[u8] {
    while !b.is_empty() && b[0].is_ascii_whitespace() {
        b = &b[1..];
    }
    while !b.is_empty() && b[b.len() - 1].is_ascii_whitespace() {
        b = &b[..b.len() - 1];
    }
    b
}

fn trim_quote(b: &[u8]) -> &[u8] {
    if b.len() >= 2 && b[0] == b'"' && b[b.len() - 1] == b'"' {
        &b[1..b.len() - 1]
    } else {
        b
    }
}

fn split_whitespace(mut b: &[u8]) -> impl Iterator<Item = &[u8]> {
    std::iter::from_fn(move || {
        while !b.is_empty() && b[0].is_ascii_whitespace() {
            b = &b[1..];
        }
        if b.is_empty() {
            return None;
        }
        let mut end = 0usize;
        while end < b.len() && !b[end].is_ascii_whitespace() {
            end += 1;
        }
        let token = &b[..end];
        b = &b[end..];
        Some(token)
    })
}

fn timezone_info() -> ([u8; 8], i32) {
    let mut out = [0u8; 8];
    let mut offset = 0i32;

    unsafe {
        let now = libc::time(std::ptr::null_mut());
        let mut local_tm = std::mem::zeroed::<libc::tm>();
        let mut utc_tm = std::mem::zeroed::<libc::tm>();
        if libc::localtime_r(&now, &mut local_tm).is_null()
            || libc::gmtime_r(&now, &mut utc_tm).is_null()
        {
            return (out, offset);
        }

        let local_secs = tm_to_seconds(local_tm.tm_hour, local_tm.tm_min, local_tm.tm_sec);
        let utc_secs = tm_to_seconds(utc_tm.tm_hour, utc_tm.tm_min, utc_tm.tm_sec);
        offset = local_secs - utc_secs;

        if offset > 14 * 3600 {
            offset -= 24 * 3600;
        } else if offset < -14 * 3600 {
            offset += 24 * 3600;
        }

        if !local_tm.tm_zone.is_null() {
            let cstr = std::ffi::CStr::from_ptr(local_tm.tm_zone);
            let bytes = cstr.to_bytes();
            let n = bytes.len().min(8);
            out[..n].copy_from_slice(&bytes[..n]);
        }
    }

    (out, offset)
}

fn tm_to_seconds(h: i32, m: i32, s: i32) -> i32 {
    h.saturating_mul(3600)
        .saturating_add(m.saturating_mul(60))
        .saturating_add(s)
}

fn monotonic_ns() -> u64 {
    static START: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
    let start = START.get_or_init(std::time::Instant::now);
    start.elapsed().as_nanos() as u64
}

#[cfg(test)]
mod tests {
    use aura_common::{FixedString16, OsFingerprint};

    use super::parse_os_release;

    #[test]
    fn parse_os_release_sample() {
        let fixture = include_bytes!("../../tests/fixtures/etc_os_release_sample.txt");
        let mut os = OsFingerprint {
            os_type: FixedString16::new(),
            os_id: FixedString16::new(),
            os_version_id: FixedString16::new(),
            os_pretty_name: [0; 128],
        };
        parse_os_release(fixture, &mut os);
        assert_eq!(os.os_id.as_str(), "ubuntu");
        assert_eq!(os.os_version_id.as_str(), "22.04");
    }
}
