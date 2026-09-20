mod ffi;
mod host;
mod metadata;

use std::sync::{Mutex, OnceLock};

use aura_common::{AuraError, AuraResult};

pub use metadata::{boot_time, cache_os_fingerprint};

pub(crate) use host::MacosHost;

/// Mach ports are opaque `u32` handles acquired once at daemon init and
/// never fabricated: init fails Fatally when either selector returns
/// MACH_PORT_NULL. The host port is a task-owned send right that is not
/// reference-counted per call; the task port is acquired exactly once for
/// the process lifetime.
#[derive(Clone, Copy)]
pub(crate) struct MacPorts {
    host: ffi::MachPort,
    task: ffi::MachPort,
    page_size: u64,
}

static PORTS: OnceLock<MacPorts> = OnceLock::new();
/// The one fixed NET_RT_IFLIST2 routing buffer, allocated once at init
/// from the count query plus one page (rounded up, capped at 1 MiB) and
/// reused at its exact capacity every cycle without growth.
static IFLIST2: OnceLock<Mutex<Vec<u8>>> = OnceLock::new();

pub fn init() -> AuraResult<()> {
    if PORTS.get().is_none() {
        // SAFETY: both selectors take no arguments and return the calling
        // task's ports.
        let host = unsafe { ffi::mach_host_self() };
        if host == 0 {
            return Err(AuraError::Fatal(
                "mach_host_self returned a null port".to_string(),
            ));
        }
        // SAFETY: see above.
        let task = unsafe { ffi::mach_task_self() };
        if task == 0 {
            return Err(AuraError::Fatal(
                "mach_task_self returned a null port".to_string(),
            ));
        }
        // SAFETY: `_SC_PAGESIZE` is a supported sysconf name.
        let page_size = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
        if page_size <= 0 {
            return Err(AuraError::Fatal(format!(
                "sysconf(_SC_PAGESIZE) failed: {page_size}"
            )));
        }
        let buffer = crate::collectors::network::macos::init_iflist2_buffer(
            iflist2_count_query(),
            page_size as usize,
        )?;
        let _ = IFLIST2.set(Mutex::new(buffer));
        let _ = PORTS.set(MacPorts {
            host,
            task,
            page_size: page_size as u64,
        });
    }
    Ok(())
}

fn iflist2_count_query() -> Result<usize, i32> {
    let mib = [ffi::CTL_NET, ffi::PF_ROUTE, 0, 0, ffi::NET_RT_IFLIST2, 0];
    let mut needed: libc::size_t = 0;
    // SAFETY: a null oldp requests the required byte count without writing
    // any routing data.
    let ret = unsafe {
        ffi::sysctl(
            mib.as_ptr(),
            mib.len() as libc::c_uint,
            std::ptr::null_mut(),
            &mut needed,
            std::ptr::null_mut(),
            0,
        )
    };
    if ret != 0 {
        return Err(std::io::Error::last_os_error().raw_os_error().unwrap_or(-1));
    }
    Ok(needed)
}

pub(crate) fn host() -> AuraResult<MacosHost> {
    let ports = PORTS.get().ok_or_else(|| {
        AuraError::Fatal("macOS platform host ports are not initialized".to_string())
    })?;
    let buffer = IFLIST2.get().ok_or_else(|| {
        AuraError::Fatal("macOS network routing buffer is not initialized".to_string())
    })?;
    let guard = buffer
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    Ok(MacosHost::new(*ports, guard))
}
