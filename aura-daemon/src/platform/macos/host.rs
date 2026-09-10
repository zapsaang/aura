use std::sync::MutexGuard;

use crate::collectors::cpu::macos::MacosCpuProbe;
use crate::collectors::memory::macos::MacosMemoryProbe;
use crate::collectors::network::macos::MacosNetworkProbe;

use super::ffi;
use super::MacPorts;

/// Per-cycle host handle: copies the init-time cached Mach ports, holds the
/// lock on the one fixed init-time NET_RT_IFLIST2 routing buffer, and owns
/// every transient kernel buffer acquired during the cycle. Each acquired
/// Mach info buffer is vm_deallocated exactly once (on replace or drop);
/// the routing buffer is reused at its exact capacity and never grows.
pub(crate) struct MacosHost {
    ports: MacPorts,
    iflist2: MutexGuard<'static, Vec<u8>>,
    cpu_info: Option<MachInfo>,
    vm_buf: [i32; 64],
}

impl MacosHost {
    pub(crate) fn new(ports: MacPorts, iflist2: MutexGuard<'static, Vec<u8>>) -> Self {
        Self {
            ports,
            iflist2,
            cpu_info: None,
            vm_buf: [0; 64],
        }
    }
}

struct MachInfo {
    ptr: *mut libc::c_int,
    ints: usize,
    task: ffi::MachPort,
}

impl Drop for MachInfo {
    fn drop(&mut self) {
        if !self.ptr.is_null() && self.ints > 0 {
            // SAFETY: `ptr`/`ints` are exactly the allocation returned by a
            // successful host_processor_info, and `task` is the cached
            // mach_task_self port; the buffer is deallocated exactly once.
            let _ = unsafe { ffi::vm_deallocate(self.task, self.ptr as usize, self.ints * 4) };
        }
    }
}

fn errno() -> i32 {
    std::io::Error::last_os_error().raw_os_error().unwrap_or(-1)
}

impl MacosCpuProbe for MacosHost {
    fn processor_load_info(&mut self) -> Result<(u32, &[i32]), i32> {
        let mut count: libc::c_uint = 0;
        let mut info: ffi::ProcessorInfoArray = std::ptr::null_mut();
        let mut info_count: ffi::MachMsgTypeNumber = 0;
        // SAFETY: all out-parameters reference valid storage and `ports.host`
        // is the cached mach_host_self port.
        let kr = unsafe {
            ffi::host_processor_info(
                self.ports.host,
                ffi::PROCESSOR_CPU_LOAD_INFO,
                &mut count,
                &mut info,
                &mut info_count,
            )
        };
        if kr != ffi::KERN_SUCCESS {
            return Err(kr);
        }
        if info.is_null() || info_count == 0 {
            return Err(ffi::KERN_FAILURE);
        }
        self.cpu_info = Some(MachInfo {
            ptr: info,
            ints: info_count as usize,
            task: self.ports.task,
        });
        // SAFETY: the kernel reported exactly `info_count` integer_t lanes at
        // `info`, and the guard stored above keeps the allocation alive for
        // the returned borrow.
        let lanes = unsafe { std::slice::from_raw_parts(info, info_count as usize) };
        Ok((count, lanes))
    }
}

impl MacosMemoryProbe for MacosHost {
    fn vm_info64(&mut self) -> Result<(u32, &[i32]), i32> {
        let mut count = self.vm_buf.len() as ffi::MachMsgTypeNumber;
        // SAFETY: `vm_buf` is writable for `count` lanes and `ports.host` is
        // the cached mach_host_self port.
        let kr = unsafe {
            ffi::host_statistics64(
                self.ports.host,
                ffi::HOST_VM_INFO64,
                self.vm_buf.as_mut_ptr(),
                &mut count,
            )
        };
        if kr != ffi::KERN_SUCCESS {
            return Err(kr);
        }
        let len = (count as usize).min(self.vm_buf.len());
        Ok((count, &self.vm_buf[..len]))
    }

    fn sysctlbyname(&mut self, name: &[u8], out: &mut [u8]) -> Result<usize, i32> {
        let mut cname = [0u8; 64];
        if name.is_empty() || name.len() >= cname.len() {
            return Err(ffi::EINVAL);
        }
        cname[..name.len()].copy_from_slice(name);
        let mut size = out.len();
        // SAFETY: `cname` is NUL-terminated, `out` is writable for `size`
        // bytes, and no new value is written.
        let ret = unsafe {
            libc::sysctlbyname(
                cname.as_ptr().cast(),
                out.as_mut_ptr().cast(),
                &mut size,
                std::ptr::null_mut(),
                0,
            )
        };
        if ret != 0 {
            return Err(errno());
        }
        Ok(size)
    }

    fn page_size(&self) -> u64 {
        self.ports.page_size
    }
}

impl MacosNetworkProbe for MacosHost {
    fn iflist2_dump(&mut self) -> Result<&[u8], i32> {
        let mib = [ffi::CTL_NET, ffi::PF_ROUTE, 0, 0, ffi::NET_RT_IFLIST2, 0];
        let mut len = self.iflist2.len();
        // SAFETY: `mib` names the NET_RT_IFLIST2 table and `iflist2` is the
        // fixed init-time buffer writable for exactly `len` bytes; no new
        // value is written. ENOMEM reports a kernel need beyond capacity.
        let ret = unsafe {
            ffi::sysctl(
                mib.as_ptr(),
                mib.len() as libc::c_uint,
                self.iflist2.as_mut_ptr().cast(),
                &mut len,
                std::ptr::null_mut(),
                0,
            )
        };
        if ret != 0 {
            return Err(errno());
        }
        Ok(&self.iflist2[..len])
    }
}
