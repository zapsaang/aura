#[cfg(not(target_endian = "little"))]
compile_error!("AURA shared-memory archives require a little-endian target");

#[cfg(not(target_has_atomic = "64"))]
compile_error!("AURA shared-memory archives require native 64-bit atomics");

pub mod archive;
pub mod consts;
pub mod double_buffer;
pub mod error;
pub mod seqlock;
pub mod time;

pub use archive::{
    bytes_to_string, hidden_interval_paths, hidden_intervals, validate_archive, CpuCoreStat,
    CpuGlobalStat, DerivedStats, DiskStat, FixedString16, GpuStat, GpuStats, HiddenInterval,
    HiddenKind, MemoryStats, MetaStats, MountStat, NetIfStat, NetworkStats, OsFingerprint,
    ProcessStat, ProcessStats, StorageStats, TelemetryArchive, ARCHIVE_VERSION, CAPABILITY_COUNT,
    CAP_CPU_CONTEXT_SWITCHES, CAP_CPU_GLOBAL, CAP_CPU_PER_CORE, CAP_GPU_ENUMERATION,
    CAP_MEMORY_BUFFERS, CAP_MEMORY_CACHED, CAP_MEMORY_PAGE_FAULTS, CAP_MEMORY_RAM_FREE,
    CAP_MEMORY_RAM_TOTAL, CAP_MEMORY_RAM_USED, CAP_MEMORY_SWAP, CAP_META_LOAD_AVERAGE,
    CAP_META_OS_CODENAME, CAP_META_OS_IDENTITY, CAP_META_OS_VERSION, CAP_META_OS_VERSION_ID,
    CAP_META_TIMEZONE, CAP_META_UPTIME, CAP_META_WALLCLOCK, CAP_NETWORK_BYTES, CAP_NETWORK_RATES,
    CAP_PROCESS_BLOCKED, CAP_PROCESS_RUNNING, CAP_PROCESS_SLEEPING, CAP_PROCESS_TOP_CPU,
    CAP_PROCESS_TOP_MEMORY, CAP_PROCESS_TOTAL, CAP_STORAGE_DISK_BYTES, CAP_STORAGE_DISK_IOPS,
    CAP_STORAGE_DISK_LATENCY, CAP_STORAGE_DISK_QUEUE_DEPTH, CAP_STORAGE_DISK_RATES,
    CAP_STORAGE_MOUNTS, GPU_CAP_MEMORY_TOTAL, GPU_CAP_MEMORY_USED, GPU_CAP_NAME, GPU_CAP_POWER,
    GPU_CAP_TEMPERATURE, GPU_CAP_UTILIZATION, KNOWN_CAPABILITIES_MASK, KNOWN_GPU_RECORD_MASK,
    KNOWN_PROCESS_FLAGS_MASK, MAX_CORES, MAX_DISKS, MAX_GPUS, MAX_MOUNTS, MAX_NETIFS,
    MAX_PROC_NAME_LEN, MAX_TOP_N, PROCESS_TRUNCATED, TONE_GREEN, TONE_MAGENTA, TONE_MAX, TONE_RED,
    TONE_YELLOW,
};
pub use consts::{
    system_page_size, BUFFER_0_OFFSET, BUFFER_1_OFFSET, BUFFER_SIZE, DATA_OFFSET,
    DEFAULT_HEARTBEAT_MS, HEADER_SIZE, MAX_PID, MAX_SPIN_WAIT_MS, MIN_DELTA_NS, NVML_LIBRARY,
    OFFLINE_THRESHOLD_SECS, PROC_BUFFER_SIZE, SHM_FILE_MODE, SHM_PATH, SHM_SIZE, VERSION_OFFSET,
};
pub use double_buffer::{read_double_buffer, write_double_buffer, DoubleBufferHeader};
pub use error::{AuraError, AuraResult};
pub use seqlock::validate_freshness;
pub use time::monotonic_ns;
