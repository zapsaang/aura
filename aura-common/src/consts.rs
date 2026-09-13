#[cfg(target_os = "linux")]
pub const SHM_PATH: &str = "/dev/shm/aura_state.dat";

#[cfg(target_os = "macos")]
pub const SHM_PATH: &str = "/tmp/aura_state.dat";

pub const HEADER_SIZE: usize = 24; // active_index (8) + seq[2] (16)
pub const BUFFER_SIZE: usize = 65536;
pub const BUFFER_0_OFFSET: usize = HEADER_SIZE;
pub const BUFFER_1_OFFSET: usize = HEADER_SIZE + BUFFER_SIZE;
pub const SHM_SIZE: usize = HEADER_SIZE + (2 * BUFFER_SIZE);

/// SHM leaf permissions: owner-only for private per-user IPC
pub const SHM_FILE_MODE: u32 = 0o600;

/// SeqLock version offset in mmap (first 8 bytes)
pub const VERSION_OFFSET: usize = 0;

/// Data offset in mmap (after version)
pub const DATA_OFFSET: usize = BUFFER_0_OFFSET;

/// Default heartbeat interval in milliseconds
pub const DEFAULT_HEARTBEAT_MS: u64 = 500;

/// Maximum spin wait time before declaring offline (milliseconds)
pub const MAX_SPIN_WAIT_MS: u64 = 100;

/// Offline threshold in seconds
pub const OFFLINE_THRESHOLD_SECS: f64 = 2.0;

/// Maximum elapsed time at which another SeqLock read attempt may begin.
pub const SEQLOCK_RETRY_ADMISSION_MS: u64 = 10;

/// Maximum number of processes to scan (/proc/PID max)
pub const MAX_PID: u32 = 65535;

/// Fixed reusable capacity for `/proc` parsing buffers.
pub const PROC_BUFFER_SIZE: usize = 8 * 1024;

pub const MIN_DELTA_NS: u64 = 1_000_000;

pub fn system_page_size() -> usize {
    static PAGE_SIZE: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
    *PAGE_SIZE.get_or_init(|| {
        #[cfg(unix)]
        {
            // SAFETY: `_SC_PAGESIZE` is a valid `sysconf` name and the call uses no pointers or shared mutable state.
            unsafe { libc::sysconf(libc::_SC_PAGESIZE) as usize }
        }
        #[cfg(windows)]
        {
            4096
        }
    })
}

/// NVML library name
pub const NVML_LIBRARY: &str = "libnvidia-ml.so.1";
