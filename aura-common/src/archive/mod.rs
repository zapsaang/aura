mod capabilities;
mod cpu;
mod derived;
mod fixed_string;
mod gpu;
mod hidden;
mod memory;
mod meta;
mod network;
mod process;
mod storage;
mod telemetry;
mod validation;

pub use capabilities::{
    capability_name, ARCHIVE_VERSION, CAPABILITY_COUNT, CAP_CPU_CONTEXT_SWITCHES, CAP_CPU_GLOBAL,
    CAP_CPU_PER_CORE, CAP_GPU_ENUMERATION, CAP_MEMORY_BUFFERS, CAP_MEMORY_CACHED,
    CAP_MEMORY_PAGE_FAULTS, CAP_MEMORY_RAM_FREE, CAP_MEMORY_RAM_TOTAL, CAP_MEMORY_RAM_USED,
    CAP_MEMORY_SWAP, CAP_META_LOAD_AVERAGE, CAP_META_OS_CODENAME, CAP_META_OS_IDENTITY,
    CAP_META_OS_VERSION, CAP_META_OS_VERSION_ID, CAP_META_TIMEZONE, CAP_META_UPTIME,
    CAP_META_WALLCLOCK, CAP_NETWORK_BYTES, CAP_NETWORK_RATES, CAP_PROCESS_BLOCKED,
    CAP_PROCESS_RUNNING, CAP_PROCESS_SLEEPING, CAP_PROCESS_TOP_CPU, CAP_PROCESS_TOP_MEMORY,
    CAP_PROCESS_TOTAL, CAP_STORAGE_DISK_BYTES, CAP_STORAGE_DISK_IOPS, CAP_STORAGE_DISK_LATENCY,
    CAP_STORAGE_DISK_QUEUE_DEPTH, CAP_STORAGE_DISK_RATES, CAP_STORAGE_MOUNTS, GPU_CAP_MEMORY_TOTAL,
    GPU_CAP_MEMORY_USED, GPU_CAP_NAME, GPU_CAP_POWER, GPU_CAP_TEMPERATURE, GPU_CAP_UTILIZATION,
    KNOWN_CAPABILITIES_MASK, KNOWN_GPU_RECORD_MASK, KNOWN_PROCESS_FLAGS_MASK, PROCESS_TRUNCATED,
};
pub use cpu::{CpuCoreStat, CpuGlobalStat};
pub use derived::{DerivedStats, TONE_GREEN, TONE_MAGENTA, TONE_MAX, TONE_RED, TONE_YELLOW};
pub use fixed_string::{bytes_to_string, FixedString16};
pub use gpu::{GpuStat, GpuStats};
pub use hidden::{hidden_interval_paths, hidden_intervals, HiddenInterval, HiddenKind};
pub use memory::MemoryStats;
pub use meta::{MetaStats, OsFingerprint};
pub use network::{NetIfStat, NetworkStats};
pub use process::{ProcessStat, ProcessStats};
pub use storage::{DiskStat, MountStat, StorageStats};
pub use telemetry::{TelemetryArchive, CHECKSUM_OFFSET, RESERVED_LEN, RESERVED_OFFSET};
pub use validation::validate_archive;

pub const MAX_PROC_NAME_LEN: usize = 16;
pub const MAX_TOP_N: usize = 5;
pub const MAX_CORES: usize = 128;
pub const MAX_NETIFS: usize = 16;
pub const MAX_MOUNTS: usize = 32;
pub const MAX_DISKS: usize = 16;
pub const MAX_GPUS: usize = 8;

#[allow(clippy::assertions_on_constants)]
const _: () = assert!(
    std::mem::size_of::<TelemetryArchive>() == 65_536,
    "TelemetryArchive must remain exactly 64KB"
);
#[allow(clippy::assertions_on_constants)]
const _: () = assert!(
    std::mem::align_of::<TelemetryArchive>() == 8,
    "TelemetryArchive must remain 8-byte aligned"
);
#[allow(clippy::assertions_on_constants)]
const _: () = assert!(
    std::mem::size_of::<TelemetryArchive>() % 8 == 0,
    "TelemetryArchive size must remain a multiple of eight"
);
