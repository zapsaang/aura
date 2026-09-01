use std::mem::MaybeUninit;
use std::process::Command;

use aura_common::{AuraError, AuraResult, FixedString16, MetaStats, OsFingerprint};

pub fn boot_time() -> AuraResult<u64> {
    let mut mib = [libc::CTL_KERN, libc::KERN_BOOTTIME];
    let mut boot_time_val = MaybeUninit::<libc::timeval>::uninit();
    let mut size = std::mem::size_of::<libc::timeval>();
    // SAFETY: `mib` names `kern.boottime`, `boot_time_val` is valid writable timeval storage, and `size` matches that type.
    let ret = unsafe {
        libc::sysctl(
            mib.as_mut_ptr(),
            mib.len() as libc::c_uint,
            boot_time_val.as_mut_ptr() as *mut _,
            &mut size,
            std::ptr::null_mut(),
            0,
        )
    };
    if ret != 0 {
        return Err(AuraError::PlatformNotSupported(
            "sysctl kern.boottime failed".into(),
        ));
    }
    // SAFETY: `sysctl` returned success, so `boot_time_val` was initialized by the kernel.
    let bt = unsafe { boot_time_val.assume_init() };
    // SAFETY: `time` permits a null output pointer when only the return value is needed.
    let now = unsafe { libc::time(std::ptr::null_mut()) };
    Ok(now.saturating_sub(bt.tv_sec as i64) as u64)
}

pub fn cache_os_fingerprint(meta: &mut MetaStats) -> AuraResult<()> {
    let mut os = OsFingerprint {
        os_type: FixedString16::from_bytes(b"darwin"),
        os_id: FixedString16::new(),
        os_version_id: FixedString16::new(),
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
