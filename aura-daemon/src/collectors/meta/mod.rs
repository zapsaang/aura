pub mod linux;
pub mod macos;

use std::fs::File;
use std::io::Read;

use aura_common::{
    AuraResult, FixedString16, MetaStats, OsFingerprint, CAP_GPU_ENUMERATION,
    CAP_META_LOAD_AVERAGE, CAP_META_OS_CODENAME, CAP_META_OS_IDENTITY, CAP_META_OS_VERSION,
    CAP_META_OS_VERSION_ID, CAP_META_TIMEZONE, CAP_META_UPTIME,
};

use super::parsing::split_whitespace;

#[cfg(target_os = "linux")]
pub use linux::cache_os_fingerprint;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MetaGpuAvailability {
    pub uptime: bool,
    pub load_average: bool,
    pub timezone: bool,
    pub os_identity: bool,
    pub os_version: bool,
    pub os_version_id: bool,
    pub os_codename: bool,
    pub gpu_enumeration: bool,
}

impl MetaGpuAvailability {
    pub(super) const fn capability_mask(self) -> u64 {
        let mut capabilities = 0;
        if self.uptime {
            capabilities |= CAP_META_UPTIME;
        }
        if self.load_average {
            capabilities |= CAP_META_LOAD_AVERAGE;
        }
        if self.timezone {
            capabilities |= CAP_META_TIMEZONE;
        }
        if self.os_identity {
            capabilities |= CAP_META_OS_IDENTITY;
        }
        if self.os_version {
            capabilities |= CAP_META_OS_VERSION;
        }
        if self.os_version_id {
            capabilities |= CAP_META_OS_VERSION_ID;
        }
        if self.os_codename {
            capabilities |= CAP_META_OS_CODENAME;
        }
        if self.gpu_enumeration {
            capabilities |= CAP_GPU_ENUMERATION;
        }
        capabilities
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OsAvailability {
    pub identity: bool,
    pub version: bool,
    pub version_id: bool,
    pub codename: bool,
}

/// Derive the per-cycle bit 27..30 state from the init-time cached
/// fingerprint. The cache maintains the all-or-nothing identity-triple
/// invariant, so a non-empty `os_id` implies valid type/id/pretty.
pub fn cached_os_availability(os: &OsFingerprint) -> OsAvailability {
    OsAvailability {
        identity: os.os_id.bytes[0] != 0,
        version: os.version[0] != 0,
        version_id: os.os_version_id.bytes[0] != 0,
        codename: os.version_codename.bytes[0] != 0,
    }
}

pub(crate) fn empty_fingerprint() -> OsFingerprint {
    OsFingerprint {
        os_type: FixedString16::new(),
        os_id: FixedString16::new(),
        os_version_id: FixedString16::new(),
        version_codename: FixedString16::new(),
        version: [0; 64],
        os_pretty_name: [0; 128],
    }
}

/// Copy `src` into `dest`, truncating at the last UTF-8 boundary that fits.
pub(crate) fn copy_text_truncated(dest: &mut [u8], src: &[u8]) {
    let end = src.len().min(dest.len());
    let valid = match std::str::from_utf8(&src[..end]) {
        Ok(_) => end,
        Err(error) => error.valid_up_to(),
    };
    dest[..valid].copy_from_slice(&src[..valid]);
}

pub fn collect(meta: &mut MetaStats) -> AuraResult<()> {
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

fn timezone_info() -> ([u8; 8], i32) {
    let mut out = [0u8; 8];
    let mut offset = 0i32;

    // SAFETY: `local_tm` is valid `tm` storage, `localtime_r` return is checked, and `tm_zone` is checked for null before `CStr`.
    unsafe {
        let now = libc::time(std::ptr::null_mut());
        let mut local_tm = std::mem::zeroed::<libc::tm>();
        if libc::localtime_r(&now, &mut local_tm).is_null() {
            return (out, offset);
        }

        // Use tm_gmtoff directly - it's set by localtime_r and handles
        // UTC+12/13/14 correctly without manual day-boundary calculation
        offset = local_tm.tm_gmtoff as i32;

        if !local_tm.tm_zone.is_null() {
            let cstr = std::ffi::CStr::from_ptr(local_tm.tm_zone);
            let bytes = cstr.to_bytes();
            let n = bytes.len().min(8);
            out[..n].copy_from_slice(&bytes[..n]);
        }
    }

    (out, offset)
}
