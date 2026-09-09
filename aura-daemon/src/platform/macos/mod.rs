mod cpu;
mod ffi;
mod memory;
mod metadata;
mod process;

use std::sync::OnceLock;

#[cfg(target_os = "macos")]
use std::sync::Mutex;

use aura_common::{AuraError, AuraResult, CpuGlobalStat, MemoryStats, ProcessStats};

#[cfg(target_os = "macos")]
use self::{ffi::MachPort, process::ProcessSnapshot};

pub use metadata::{boot_time, cache_os_fingerprint};

pub trait PlatformStatsProvider: Send + Sync {
    fn name(&self) -> &'static str;
    fn cpu_stats(&self) -> AuraResult<CpuGlobalStat>;
    fn memory_stats(&self) -> AuraResult<MemoryStats>;
    fn process_stats(&self) -> AuraResult<ProcessStats>;
}

static PROVIDER: OnceLock<Box<dyn PlatformStatsProvider>> = OnceLock::new();

pub fn init() -> AuraResult<&'static dyn PlatformStatsProvider> {
    if PROVIDER.get().is_none() {
        let provider: Box<dyn PlatformStatsProvider> = Box::new(MacosPlatform::new()?);
        let _ = PROVIDER.set(provider);
    }
    provider()
}

pub fn provider() -> AuraResult<&'static dyn PlatformStatsProvider> {
    PROVIDER.get().map(|p| p.as_ref()).ok_or_else(|| {
        AuraError::PlatformNotSupported("platform provider not initialized".to_string())
    })
}

#[derive(Debug)]
pub struct MacosPlatform {
    #[cfg(target_os = "macos")]
    host_port: MachPort,
    #[cfg(target_os = "macos")]
    process_snapshot: Mutex<ProcessSnapshot>,
}

impl MacosPlatform {
    pub fn new() -> AuraResult<Self> {
        #[cfg(target_os = "macos")]
        {
            // SAFETY: `mach_host_self` takes no arguments and returns the current task's host send right.
            let host_port = unsafe { ffi::mach_host_self() };
            Ok(Self {
                host_port,
                process_snapshot: Mutex::new(ProcessSnapshot::default()),
            })
        }

        #[cfg(not(target_os = "macos"))]
        {
            Err(AuraError::PlatformNotSupported(
                "macOS platform is only available on macOS targets".to_string(),
            ))
        }
    }
}

impl PlatformStatsProvider for MacosPlatform {
    fn name(&self) -> &'static str {
        "macos"
    }

    fn cpu_stats(&self) -> AuraResult<CpuGlobalStat> {
        #[cfg(target_os = "macos")]
        {
            cpu::collect(self)
        }
        #[cfg(not(target_os = "macos"))]
        Err(AuraError::PlatformNotSupported(
            "macOS platform is only available on macOS targets".to_string(),
        ))
    }

    fn memory_stats(&self) -> AuraResult<MemoryStats> {
        #[cfg(target_os = "macos")]
        {
            memory::collect(self)
        }
        #[cfg(not(target_os = "macos"))]
        Err(AuraError::PlatformNotSupported(
            "macOS platform is only available on macOS targets".to_string(),
        ))
    }

    fn process_stats(&self) -> AuraResult<ProcessStats> {
        #[cfg(target_os = "macos")]
        {
            process::collect(self)
        }
        #[cfg(not(target_os = "macos"))]
        Err(AuraError::PlatformNotSupported(
            "macOS platform is only available on macOS targets".to_string(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::{MacosPlatform, PlatformStatsProvider};

    #[test]
    #[cfg(not(target_os = "macos"))]
    fn constructor_is_unsupported_off_macos() {
        let err = MacosPlatform::new().expect_err("expected unsupported platform error");
        let msg = err.to_string();
        assert!(msg.contains("macOS platform is only available"));
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn stub_collectors_return_structs() {
        let provider = MacosPlatform::new().expect("macos provider");
        let cpu = provider.cpu_stats().expect("cpu");
        let mem = provider.memory_stats().expect("memory");
        let proc = provider.process_stats().expect("process");

        assert!(cpu.total_ticks >= cpu.idle_ticks);
        assert!(mem.ram_total >= mem.ram_free);
        assert!(proc.total <= u32::MAX);
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn init_then_provider_returns_same_instance() {
        let p1 = crate::platform::macos::init().expect("init should succeed");
        let p2 = crate::platform::macos::provider().expect("provider should return Ok after init");

        assert!(
            std::ptr::eq(p1 as *const _, p2 as *const _),
            "provider() should return the same instance initialized by init()"
        );
    }
}
