use aura_common::{FixedString16, OsFingerprint};

use super::{copy_text_truncated, empty_fingerprint};

pub const KERN_OSTYPE: &[u8] = b"kern.ostype";
pub const KERN_OSPRODUCTVERSION: &[u8] = b"kern.osproductversion";
pub const KERN_VERSION: &[u8] = b"kern.version";

const MACOS_ID: &[u8] = b"macos";
const PRETTY_PREFIX: &[u8] = b"macOS ";

/// Raw-bytes access to public `sysctlbyname` identity keys, implemented by
/// the macOS host handle and by scripted test probes.
pub trait MacosMetaProbe {
    fn sysctlbyname(&mut self, name: &[u8], out: &mut [u8]) -> Result<usize, i32>;
}

#[derive(Clone, Copy, Debug)]
pub struct MacosIdentity {
    pub fingerprint: OsFingerprint,
    pub identity: bool,
    pub version: bool,
    pub version_id: bool,
}

/// Map public sysctl identity keys onto the cached fingerprint. Bit 27
/// requires a valid `kern.ostype`, the literal ID `macos`, and the pretty
/// name exactly `macOS {kern.osproductversion}`; failure of any clears all
/// three. `kern.version` owns bit 28 and `kern.osproductversion` bit 29;
/// the codename (bit 30) is unsupported and stays zero. Every failure is
/// local: this function never aborts and never allocates.
pub fn collect_identity<P: MacosMetaProbe + ?Sized>(probe: &mut P) -> MacosIdentity {
    let mut fingerprint = empty_fingerprint();
    let mut scratch = [0u8; 256];
    let mut product = [0u8; 64];
    let mut product_len = 0usize;

    let mut ostype_ok = false;
    if let Some(text) = read_text(probe, KERN_OSTYPE, &mut scratch) {
        fingerprint.os_type = FixedString16::from_bytes(text);
        ostype_ok = true;
    }
    let mut version_id = false;
    if let Some(text) = read_text(probe, KERN_OSPRODUCTVERSION, &mut product) {
        product_len = text.len();
        fingerprint.os_version_id = FixedString16::from_bytes(text);
        version_id = true;
    }
    let mut version = false;
    if let Some(text) = read_text(probe, KERN_VERSION, &mut scratch) {
        copy_text_truncated(&mut fingerprint.version, text);
        version = true;
    }

    let identity = ostype_ok && version_id;
    if identity {
        fingerprint.os_id = FixedString16::from_bytes(MACOS_ID);
        fingerprint.os_pretty_name[..PRETTY_PREFIX.len()].copy_from_slice(PRETTY_PREFIX);
        copy_text_truncated(
            &mut fingerprint.os_pretty_name[PRETTY_PREFIX.len()..],
            &product[..product_len],
        );
    } else {
        fingerprint.os_type = FixedString16::new();
    }

    MacosIdentity {
        fingerprint,
        identity,
        version,
        version_id,
    }
}

fn read_text<'a, P: MacosMetaProbe + ?Sized>(
    probe: &mut P,
    name: &[u8],
    buf: &'a mut [u8],
) -> Option<&'a [u8]> {
    let n = probe.sysctlbyname(name, buf).ok()?;
    let mut text = &buf[..n.min(buf.len())];
    while text.last() == Some(&0) {
        text = &text[..text.len() - 1];
    }
    if text.is_empty() || std::str::from_utf8(text).is_err() {
        return None;
    }
    if text.iter().any(u8::is_ascii_control) {
        return None;
    }
    Some(text)
}
