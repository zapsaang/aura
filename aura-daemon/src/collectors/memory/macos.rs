use aura_common::{AuraError, AuraResult, MemoryStats};

use super::MemoryAvailability;

/// Known integer_t lane count for the HOST_VM_INFO64 struct. Kernels may
/// report additional lanes, which collectors ignore.
pub const HOST_VM_INFO64_COUNT: u32 = 38;

pub const SYSCTL_HW_MEMSIZE: &[u8] = b"hw.memsize";
pub const SYSCTL_KERN_BOOTTIME: &[u8] = b"kern.boottime";
pub const SYSCTL_VM_SWAPUSAGE: &[u8] = b"vm.swapusage";
pub const SYSCTL_HW_PAGESIZE: &[u8] = b"hw.pagesize";

pub const HW_MEMSIZE_LEN: usize = 8;
pub const KERN_BOOTTIME_LEN: usize = 16;
pub const XSW_USAGE_LEN: usize = 32;
pub const HW_PAGESIZE_LEN: usize = 4;

/// Darwin errno values for the swap-local outcome classes. These are the
/// macOS numeric values regardless of the host compiling this module.
pub const MACOS_EPERM: i32 = 1;
pub const MACOS_ENOENT: i32 = 2;
pub const MACOS_ENOTSUP: i32 = 45;

const VM_STAT64_FREE: usize = 0;
const VM_STAT64_INACTIVE: usize = 2;
const VM_STAT64_FAULTS_LANE: usize = 12;

/// Raw host_statistics64/sysctlbyname queries. The trait exposes only
/// handles and stable byte/query descriptors; this module owns parsing and
/// every capability decision.
pub trait MacosMemoryProbe {
    /// Returns `(reported_count, raw integer_t lanes)` for the
    /// HOST_VM_INFO64 flavor, or the raw `kern_return_t` on failure.
    fn vm_info64(&mut self) -> Result<(u32, &[i32]), i32>;
    /// Runs `sysctlbyname` for `name` into `out`, returning the number of
    /// bytes written or the raw errno.
    fn sysctlbyname(&mut self, name: &[u8], out: &mut [u8]) -> Result<usize, i32>;
    /// Process page size from `sysconf(_SC_PAGESIZE)`, validated at init.
    fn page_size(&self) -> u64;
}

fn fatal(message: String) -> AuraError {
    AuraError::Fatal(message)
}

/// Parses one exact-size `struct timeval` (`kern.boottime`): seconds at
/// offset 0, microseconds at offset 8.
pub fn parse_timeval(bytes: &[u8]) -> AuraResult<(i64, i64)> {
    if bytes.len() != KERN_BOOTTIME_LEN {
        return Err(fatal(format!(
            "sysctlbyname timeval size mismatch: {} bytes",
            bytes.len()
        )));
    }
    let sec = i64::from_le_bytes(bytes[..8].try_into().expect("8 bytes"));
    let usec = i32::from_le_bytes(bytes[8..12].try_into().expect("4 bytes")) as i64;
    Ok((sec, usec))
}

fn parse_swap(bytes: &[u8; XSW_USAGE_LEN]) -> AuraResult<(u64, u64, u64)> {
    let total = u64::from_le_bytes(bytes[0..8].try_into().expect("8 bytes"));
    let avail = u64::from_le_bytes(bytes[8..16].try_into().expect("8 bytes"));
    let used = u64::from_le_bytes(bytes[16..24].try_into().expect("8 bytes"));
    let consistent = avail
        .checked_add(used)
        .map(|sum| sum == total)
        .unwrap_or(false);
    if !consistent {
        return Err(fatal(
            "sysctlbyname vm.swapusage inconsistent values".to_string(),
        ));
    }
    Ok((total, avail, used))
}

/// Collects RAM/swap telemetry from the mandatory HOST_VM_INFO64 flavor and
/// `hw.memsize`, plus the independently optional `vm.swapusage`.
pub fn collect_memory_from_probe<P: MacosMemoryProbe + ?Sized>(
    probe: &mut P,
    out: &mut MemoryStats,
) -> AuraResult<MemoryAvailability> {
    let (count, lanes) = probe
        .vm_info64()
        .map_err(|code| fatal(format!("host_statistics64 failed: kern_return_t {code}")))?;
    let usable = (count as usize)
        .min(lanes.len())
        .min(HOST_VM_INFO64_COUNT as usize);
    if usable < VM_STAT64_FAULTS_LANE + 2 {
        return Err(fatal(format!(
            "host_statistics64 HOST_VM_INFO64 count mismatch: {count}"
        )));
    }
    let free_pages = lanes[VM_STAT64_FREE] as u32 as u64;
    let inactive_pages = lanes[VM_STAT64_INACTIVE] as u32 as u64;
    let faults_lo = lanes[VM_STAT64_FAULTS_LANE] as u32 as u64;
    let faults_hi = lanes[VM_STAT64_FAULTS_LANE + 1] as u32 as u64;
    let faults = faults_lo | (faults_hi << 32);

    let mut memsize_buf = [0u8; HW_MEMSIZE_LEN];
    let read = probe
        .sysctlbyname(SYSCTL_HW_MEMSIZE, &mut memsize_buf)
        .map_err(|code| fatal(format!("sysctlbyname hw.memsize failed: errno {code}")))?;
    if read != HW_MEMSIZE_LEN {
        return Err(fatal(format!(
            "sysctlbyname hw.memsize size mismatch: {read} bytes"
        )));
    }
    let memsize = u64::from_le_bytes(memsize_buf);

    let page_size = probe.page_size();
    let free = free_pages.saturating_mul(page_size);
    let cached = inactive_pages.saturating_mul(page_size);

    out.ram_total = memsize;
    out.ram_free = free;
    out.ram_used = memsize.saturating_sub(free);
    out.buffers = 0;
    out.cached = cached;
    out.page_faults = faults;
    out.page_faults_per_sec = 0.0;

    let mut swap_available = false;
    let mut swap_buf = [0u8; XSW_USAGE_LEN];
    match probe.sysctlbyname(SYSCTL_VM_SWAPUSAGE, &mut swap_buf) {
        Ok(read) if read == XSW_USAGE_LEN => {
            let (total, avail, used) = parse_swap(&swap_buf)?;
            out.swap_total = total;
            out.swap_free = avail;
            out.swap_used = used;
            swap_available = true;
        }
        Ok(read) => {
            return Err(fatal(format!(
                "sysctlbyname vm.swapusage size mismatch: {read} bytes"
            )));
        }
        Err(MACOS_ENOENT) | Err(MACOS_ENOTSUP) | Err(MACOS_EPERM) => {
            out.swap_total = 0;
            out.swap_free = 0;
            out.swap_used = 0;
        }
        Err(code) => {
            return Err(fatal(format!(
                "sysctlbyname vm.swapusage failed: errno {code}"
            )));
        }
    }

    Ok(MemoryAvailability {
        buffers: false,
        cached: true,
        swap: swap_available,
        page_faults: true,
    })
}

#[cfg(target_os = "macos")]
pub fn collect(
    _meminfo_buf: &mut Vec<u8>,
    _vmstat_buf: &mut Vec<u8>,
    out: &mut MemoryStats,
) -> AuraResult<MemoryAvailability> {
    let mut host = crate::platform::macos::host()?;
    collect_memory_from_probe(&mut host, out)
}
