pub const ARCHIVE_VERSION: u64 = 2;

pub const CAP_CPU_GLOBAL: u64 = 1 << 0;
pub const CAP_CPU_PER_CORE: u64 = 1 << 1;
pub const CAP_CPU_CONTEXT_SWITCHES: u64 = 1 << 2;
pub const CAP_PROCESS_TOTAL: u64 = 1 << 3;
pub const CAP_PROCESS_RUNNING: u64 = 1 << 4;
pub const CAP_PROCESS_BLOCKED: u64 = 1 << 5;
pub const CAP_PROCESS_SLEEPING: u64 = 1 << 6;
pub const CAP_PROCESS_TOP_CPU: u64 = 1 << 7;
pub const CAP_PROCESS_TOP_MEMORY: u64 = 1 << 8;
pub const CAP_MEMORY_RAM_TOTAL: u64 = 1 << 9;
pub const CAP_MEMORY_RAM_FREE: u64 = 1 << 10;
pub const CAP_MEMORY_RAM_USED: u64 = 1 << 11;
pub const CAP_MEMORY_BUFFERS: u64 = 1 << 12;
pub const CAP_MEMORY_CACHED: u64 = 1 << 13;
pub const CAP_MEMORY_SWAP: u64 = 1 << 14;
pub const CAP_MEMORY_PAGE_FAULTS: u64 = 1 << 15;
pub const CAP_STORAGE_DISK_BYTES: u64 = 1 << 16;
pub const CAP_STORAGE_DISK_RATES: u64 = 1 << 17;
pub const CAP_STORAGE_DISK_IOPS: u64 = 1 << 18;
pub const CAP_STORAGE_DISK_QUEUE_DEPTH: u64 = 1 << 19;
pub const CAP_STORAGE_DISK_LATENCY: u64 = 1 << 20;
pub const CAP_STORAGE_MOUNTS: u64 = 1 << 21;
pub const CAP_NETWORK_BYTES: u64 = 1 << 22;
pub const CAP_NETWORK_RATES: u64 = 1 << 23;
pub const CAP_META_UPTIME: u64 = 1 << 24;
pub const CAP_META_LOAD_AVERAGE: u64 = 1 << 25;
pub const CAP_META_TIMEZONE: u64 = 1 << 26;
pub const CAP_META_OS_IDENTITY: u64 = 1 << 27;
pub const CAP_META_OS_VERSION: u64 = 1 << 28;
pub const CAP_META_OS_VERSION_ID: u64 = 1 << 29;
pub const CAP_META_OS_CODENAME: u64 = 1 << 30;
pub const CAP_META_WALLCLOCK: u64 = 1 << 31;
pub const CAP_GPU_ENUMERATION: u64 = 1 << 32;

pub const CAPABILITY_COUNT: u32 = 33;
pub const KNOWN_CAPABILITIES_MASK: u64 = (1 << CAPABILITY_COUNT) - 1;

pub const GPU_CAP_NAME: u64 = 1 << 0;
pub const GPU_CAP_MEMORY_TOTAL: u64 = 1 << 1;
pub const GPU_CAP_MEMORY_USED: u64 = 1 << 2;
pub const GPU_CAP_UTILIZATION: u64 = 1 << 3;
pub const GPU_CAP_POWER: u64 = 1 << 4;
pub const GPU_CAP_TEMPERATURE: u64 = 1 << 5;
pub const KNOWN_GPU_RECORD_MASK: u64 = 0x3F;

pub const PROCESS_TRUNCATED: u8 = 1;
pub const KNOWN_PROCESS_FLAGS_MASK: u8 = PROCESS_TRUNCATED;

const CAPABILITY_NAMES: [&str; CAPABILITY_COUNT as usize] = [
    "cpu_global",
    "cpu_per_core",
    "cpu_context_switches",
    "process_total",
    "process_running",
    "process_blocked",
    "process_sleeping",
    "process_top_cpu",
    "process_top_memory",
    "memory_ram_total",
    "memory_ram_free",
    "memory_ram_used",
    "memory_buffers",
    "memory_cached",
    "memory_swap",
    "memory_page_faults",
    "storage_disk_bytes",
    "storage_disk_rates",
    "storage_disk_iops",
    "storage_disk_queue_depth",
    "storage_disk_latency",
    "storage_mounts",
    "network_bytes",
    "network_rates",
    "meta_uptime",
    "meta_load_average",
    "meta_timezone",
    "meta_os_identity",
    "meta_os_version",
    "meta_os_version_id",
    "meta_os_codename",
    "meta_wallclock",
    "gpu_enumeration",
];

pub fn capability_name(bit: u32) -> Option<&'static str> {
    CAPABILITY_NAMES.get(bit as usize).copied()
}
