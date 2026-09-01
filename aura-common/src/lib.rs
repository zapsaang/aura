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
    bytes_to_string, CpuCoreStat, CpuGlobalStat, DiskStat, FixedString16, GpuStat, GpuStats,
    MemoryStats, MetaStats, MountStat, NetIfStat, NetworkStats, OsFingerprint, ProcessStat,
    ProcessStats, StorageStats, TelemetryArchive, MAX_CORES, MAX_DISKS, MAX_MOUNTS, MAX_NETIFS,
    MAX_PROC_NAME_LEN, MAX_TOP_N,
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
