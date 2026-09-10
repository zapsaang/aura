use aura_common::{CAP_CPU_CONTEXT_SWITCHES, CAP_CPU_GLOBAL, CAP_CPU_PER_CORE};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CpuAvailability {
    pub context_switches: bool,
    pub over_capacity: bool,
}

impl CpuAvailability {
    pub const fn capability_mask(self) -> u64 {
        let mut capabilities = CAP_CPU_GLOBAL;
        if !self.over_capacity {
            capabilities |= CAP_CPU_PER_CORE;
        }
        if self.context_switches {
            capabilities |= CAP_CPU_CONTEXT_SWITCHES;
        }
        capabilities
    }
}

#[cfg(target_os = "linux")]
pub mod linux;

#[cfg(target_os = "linux")]
pub use linux::collect;

pub mod macos;

#[cfg(target_os = "macos")]
pub use macos::collect;
