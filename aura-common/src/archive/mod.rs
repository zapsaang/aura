mod cpu;
mod fixed_string;
mod gpu;
mod memory;
mod meta;
mod network;
mod process;
mod storage;
mod telemetry;

pub use cpu::{CpuCoreStat, CpuGlobalStat};
pub use fixed_string::{bytes_to_string, FixedString16};
pub use gpu::{GpuStat, GpuStats};
pub use memory::MemoryStats;
pub use meta::{MetaStats, OsFingerprint};
pub use network::{NetIfStat, NetworkStats};
pub use process::{ProcessStat, ProcessStats};
pub use storage::{DiskStat, MountStat, StorageStats};
pub use telemetry::TelemetryArchive;

pub const MAX_PROC_NAME_LEN: usize = 16;
pub const MAX_TOP_N: usize = 5;
pub const MAX_CORES: usize = 128;
pub const MAX_NETIFS: usize = 16;
pub const MAX_MOUNTS: usize = 32;
pub const MAX_DISKS: usize = 16;

const _: () = assert!(
    std::mem::size_of::<TelemetryArchive>() == 65_536,
    "TelemetryArchive must remain exactly 64KB"
);
const _: () = assert!(
    std::mem::align_of::<TelemetryArchive>() == 8,
    "TelemetryArchive must remain 8-byte aligned"
);
