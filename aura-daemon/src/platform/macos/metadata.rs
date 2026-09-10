use std::process::Command;

use aura_common::{AuraError, AuraResult, FixedString16, MetaStats, OsFingerprint};

use crate::collectors::memory::macos::{
    parse_timeval, MacosMemoryProbe, KERN_BOOTTIME_LEN, SYSCTL_KERN_BOOTTIME,
};

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

pub fn cache_os_fingerprint(meta: &mut MetaStats) -> AuraResult<()> {
    let mut os = OsFingerprint {
        os_type: FixedString16::from_bytes(b"darwin"),
        os_id: FixedString16::new(),
        os_version_id: FixedString16::new(),
        version_codename: FixedString16::new(),
        version: [0; 64],
        os_pretty_name: [0; 128],
    };
    if let Ok(output) = Command::new("sw_vers").output() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        for line in stdout.lines() {
            if let Some((key, value)) = line.split_once(':') {
                let key = key.trim();
                let value = value.trim();
                match key {
                    "ProductVersion" => {
                        os.os_version_id = FixedString16::from_bytes(value.as_bytes());
                    }
                    "ProductName" => {
                        let n = value.len().min(128);
                        os.os_pretty_name[..n].copy_from_slice(&value.as_bytes()[..n]);
                    }
                    "BuildVersion" => {
                        os.os_id = FixedString16::from_bytes(value.as_bytes());
                    }
                    _ => {}
                }
            }
        }
    }
    meta.os = os;
    Ok(())
}
