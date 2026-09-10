use aura_common::{AuraError, AuraResult, MetaStats};

use crate::collectors::memory::macos::{
    parse_timeval, MacosMemoryProbe, KERN_BOOTTIME_LEN, SYSCTL_KERN_BOOTTIME,
};
use crate::collectors::meta::macos::collect_identity;

pub fn boot_time() -> AuraResult<u64> {
    let mut host = super::host()?;
    let mut buf = [0u8; KERN_BOOTTIME_LEN];
    let read = host
        .sysctlbyname(SYSCTL_KERN_BOOTTIME, &mut buf)
        .map_err(|code| {
            AuraError::Fatal(format!("sysctlbyname kern.boottime failed: errno {code}"))
        })?;
    if read != KERN_BOOTTIME_LEN {
        return Err(AuraError::Fatal(format!(
            "sysctlbyname kern.boottime size mismatch: {read} bytes"
        )));
    }
    let (boot_sec, _) = parse_timeval(&buf)?;
    // SAFETY: `time` permits a null output pointer when only the return value is needed.
    let now = unsafe { libc::time(std::ptr::null_mut()) };
    Ok(now.saturating_sub(boot_sec) as u64)
}

/// Cache the macOS OS fingerprint at daemon init using only public
/// `sysctlbyname` keys (no helper tools, plists, or private frameworks).
pub fn cache_os_fingerprint(meta: &mut MetaStats) -> AuraResult<()> {
    let mut host = super::host()?;
    let identity = collect_identity(&mut host);
    meta.os = identity.fingerprint;
    Ok(())
}
