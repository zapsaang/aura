#[cfg(target_os = "linux")]
pub mod linux;

#[cfg(target_os = "linux")]
pub use linux::collect;

pub mod macos;

#[cfg(target_os = "macos")]
pub use macos::collect;
