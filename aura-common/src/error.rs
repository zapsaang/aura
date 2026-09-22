use thiserror::Error;

#[derive(Error, Debug)]
pub enum AuraError {
    #[error("Telemetry has not been published")]
    NotPublished,

    #[error("SeqLock read retry-admission deadline exhausted")]
    SeqLockTimeout,

    #[error("Invalid shared memory header: active buffer {found}")]
    InvalidShmHeader { found: u64 },

    #[error("SeqLock sequence exhausted at {sequence}")]
    SequenceExhausted { sequence: u64 },

    #[error("Data checksum mismatch: expected 0x{expected:08x}, got 0x{actual:08x}")]
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

    #[error("ABI version mismatch: expected 2, found {found}")]
    UnsupportedVersion { found: u64 },

    #[error("Invalid archive: {reason}")]
    InvalidArchive { reason: String },

    #[error("daemon is offline: {0}")]
    Offline(String),

    #[error("Incompatible shared memory size: expected {expected}, found {found}")]
    IncompatibleShmSize { expected: u64, found: u64 },

    #[error("fatal runtime error: {0}")]
    Fatal(String),

    #[error("invalid argument: {0}")]
    InvalidArgument(String),
}

pub type AuraResult<T> = Result<T, AuraError>;
