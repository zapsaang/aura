#[cfg(target_os = "linux")]
pub mod linux;

#[cfg(target_os = "linux")]
pub use linux::collect;

pub mod macos;

#[cfg(target_os = "macos")]
pub use macos::collect;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MemoryAvailability {
    pub buffers: bool,
    pub cached: bool,
    pub swap: bool,
    pub page_faults: bool,
}
