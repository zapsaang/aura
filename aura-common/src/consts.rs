#[cfg(target_os = "linux")]
pub const SHM_PATH: &str = "/dev/shm/aura_state.dat";

#[cfg(target_os = "macos")]
pub const SHM_PATH: &str = "/tmp/aura_state.dat";

/// Shared memory file size (64KB - must be page-aligned)
pub const SHM_SIZE: usize = 65536;

/// SHM file permissions: world-readable/writable for cross-user IPC
pub const SHM_FILE_MODE: u32 = 0o666;

/// SeqLock version offset in mmap (first 8 bytes)
pub const VERSION_OFFSET: usize = 0;

/// Data offset in mmap (after version)
pub const DATA_OFFSET: usize = 8;

/// Default heartbeat interval in milliseconds
pub const DEFAULT_HEARTBEAT_MS: u64 = 500;

/// Maximum spin wait time before declaring offline (milliseconds)
pub const MAX_SPIN_WAIT_MS: u64 = 100;

/// Offline threshold in seconds
pub const OFFLINE_THRESHOLD_SECS: f64 = 2.0;

/// Maximum number of processes to scan (/proc/PID max)
pub const MAX_PID: u32 = 65535;

/// Page size for /proc parsing buffer
pub const PROC_BUFFER_SIZE: usize = 4096;

/// NVML library name
pub const NVML_LIBRARY: &str = "libnvidia-ml.so.1";
