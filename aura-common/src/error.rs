use thiserror::Error;

#[derive(Error, Debug)]
pub enum AuraError {
    #[error("SeqLock validation failed: version mismatch")]
    SeqLockInvalid,

    #[error("Data checksum mismatch: expected {expected}, got {actual}")]
    ChecksumMismatch { expected: u32, actual: u32 },

    #[error("Shared memory error: {0}")]
    SharedMemory(#[from] std::io::Error),

    #[error("mmap mapping failed: {0}")]
    MmapFailed(String),

    #[error("Data is stale (age: {age_ms}ms > threshold: {threshold_ms}ms)")]
    StaleData { age_ms: u64, threshold_ms: u64 },

    #[error("No NVML/GPU available")]
    GpuUnavailable,

    #[error("Parse error: {0}")]
    ParseError(String),

    #[error("Platform not supported: {0}")]
    PlatformNotSupported(String),

    #[error("Security validation failed: {0}")]
    Security(String),

    #[error("Another aura-daemon instance is already running (SHM file locked)")]
    AlreadyRunning,
}

pub type AuraResult<T> = Result<T, AuraError>;
