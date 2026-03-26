use rkyv::{Archive, Deserialize, Serialize};

pub const MAX_PROC_NAME_LEN: usize = 16;
pub const MAX_TOP_N: usize = 5;
pub const MAX_CORES: usize = 128;
pub const MAX_NETIFS: usize = 16;
pub const MAX_MOUNTS: usize = 32;
pub const MAX_DISKS: usize = 16;

#[derive(Archive, Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
#[archive_attr(derive(Copy, Clone, PartialEq, Eq))]
pub struct FixedString16 {
    pub bytes: [u8; 16],
}

impl FixedString16 {
    pub const fn new() -> Self {
        Self { bytes: [0u8; 16] }
    }

    pub fn from_bytes(b: &[u8]) -> Self {
        let mut s = Self::new();
        let end = find_utf8_truncation_point(b, 16);
        let mut i = 0;
        while i < end {
            s.bytes[i] = b[i];
            i += 1;
        }
        s
    }

    pub fn as_str(&self) -> &str {
        let len = self.bytes.iter().position(|&b| b == 0).unwrap_or(16);
        let slice = &self.bytes[..len];
        match std::str::from_utf8(slice) {
            Ok(s) => s,
            Err(e) => unsafe { std::str::from_utf8_unchecked(&slice[..e.valid_up_to()]) },
        }
    }
}

fn find_utf8_truncation_point(b: &[u8], max_len: usize) -> usize {
    let len = b.len().min(max_len);
    if len == 0 {
        return 0;
    }

    let mut i = 0;
    while i < len {
        let byte = b[i];

        if byte & 0x80 == 0 {
            i += 1;
            continue;
        }

        let cont_needed = if (byte & 0xE0) == 0xC0 {
            1
        } else if (byte & 0xF0) == 0xE0 {
            2
        } else if (byte & 0xF8) == 0xF0 {
            3
        } else {
            i += 1;
            continue;
        };

        if i + cont_needed < len {
            i += cont_needed + 1;
        } else {
            return i;
        }
    }

    len
}

impl Default for FixedString16 {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Archive, Serialize, Deserialize, Clone, Copy)]
#[archive_attr(derive(Copy, Clone))]
pub struct CpuCoreStat {
    pub core_index: u8,
    pub user_ticks: u64,
    pub system_ticks: u64,
    pub idle_ticks: u64,
    pub total_ticks: u64,
    pub usage_percent: f32,
}

#[derive(Archive, Serialize, Deserialize, Clone, Copy)]
#[archive_attr(derive(Copy, Clone))]
pub struct CpuGlobalStat {
    pub user_ticks: u64,
    pub system_ticks: u64,
    pub idle_ticks: u64,
    pub total_ticks: u64,
    pub context_switches: u64,
    pub context_switches_per_sec: f32,
    pub usage_percent: f32,
    pub cores: [CpuCoreStat; MAX_CORES],
    pub core_count: u8,
}

#[derive(Archive, Serialize, Deserialize, Clone, Copy)]
#[archive_attr(derive(Copy, Clone))]
pub struct ProcessStat {
    pub pid: u32,
    pub cpu_usage: f32,
    pub memory_bytes: u64,
    pub comm: FixedString16,
}

#[derive(Archive, Serialize, Deserialize, Clone, Copy)]
#[archive_attr(derive(Copy, Clone))]
pub struct ProcessStats {
    pub total: u32,
    pub running: u32,
    pub blocked: u32,
    pub sleeping: u32,
    pub top_cpu: [ProcessStat; MAX_TOP_N],
    pub top_mem: [ProcessStat; MAX_TOP_N],
}

#[derive(Archive, Serialize, Deserialize, Clone, Copy)]
#[archive_attr(derive(Copy, Clone))]
pub struct MemoryStats {
    pub ram_total: u64,
    pub ram_free: u64,
    pub ram_used: u64,
    pub buffers: u64,
    pub cached: u64,
    pub swap_total: u64,
    pub swap_free: u64,
    pub swap_used: u64,
    pub page_faults: u64,
    pub page_faults_per_sec: f32,
}

#[derive(Archive, Serialize, Deserialize, Clone, Copy)]
#[archive_attr(derive(Copy, Clone))]
pub struct DiskStat {
    pub name: FixedString16,
    pub rx_bytes: u64,
    pub wx_bytes: u64,
    pub rx_per_sec: f32,
    pub wx_per_sec: f32,
}

#[derive(Archive, Serialize, Deserialize, Clone, Copy)]
#[archive_attr(derive(Copy, Clone))]
pub struct MountStat {
    pub mountpoint: [u8; 256],
    pub fstype: FixedString16,
    pub total: u64,
    pub available: u64,
    pub used: u64,
    pub percent: f32,
}

#[derive(Archive, Serialize, Deserialize, Clone, Copy)]
#[archive_attr(derive(Copy, Clone))]
pub struct StorageStats {
    pub disks: [DiskStat; MAX_DISKS],
    pub disk_count: u8,
    pub mounts: [MountStat; MAX_MOUNTS],
    pub mount_count: u16,
}

#[derive(Archive, Serialize, Deserialize, Clone, Copy)]
#[archive_attr(derive(Copy, Clone))]
pub struct NetIfStat {
    pub name: FixedString16,
    pub rx_bytes: u64,
    pub tx_bytes: u64,
    pub rx_bytes_per_sec: f32,
    pub tx_bytes_per_sec: f32,
}

#[derive(Archive, Serialize, Deserialize, Clone, Copy)]
#[archive_attr(derive(Copy, Clone))]
pub struct NetworkStats {
    pub interfaces: [NetIfStat; MAX_NETIFS],
    pub if_count: u8,
}

#[derive(Archive, Serialize, Deserialize, Clone, Copy)]
#[archive_attr(derive(Copy, Clone))]
pub struct OsFingerprint {
    pub os_type: FixedString16,
    pub os_id: FixedString16,
    pub os_version_id: FixedString16,
    pub os_pretty_name: [u8; 128],
}

#[derive(Archive, Serialize, Deserialize, Clone, Copy)]
#[archive_attr(derive(Copy, Clone))]
pub struct MetaStats {
    pub timestamp_ns: u64,
    pub uptime_secs: u64,
    pub load_avg_1m: f32,
    pub load_avg_5m: f32,
    pub load_avg_15m: f32,
    pub timezone_name: [u8; 8],
    pub timezone_offset_secs: i32,
    pub os: OsFingerprint,
}

#[derive(Archive, Serialize, Deserialize, Clone, Copy)]
#[archive_attr(derive(Copy, Clone))]
pub struct GpuStat {
    pub name: FixedString16,
    pub memory_total: u64,
    pub memory_used: u64,
    pub utilization_percent: f32,
    pub power_watts: f32,
    pub temperature_celsius: i16,
    pub available: bool,
}

#[derive(Archive, Serialize, Deserialize, Clone, Copy)]
#[archive_attr(derive(Copy, Clone))]
pub struct GpuStats {
    pub gpus: [GpuStat; 8],
    pub gpu_count: u8,
    pub nvml_available: bool,
}

#[derive(Archive, Serialize, Deserialize, Clone, Copy)]
#[archive_attr(derive(Copy, Clone))]
pub struct TelemetryArchive {
    pub version: u64,
    pub cpu: CpuGlobalStat,
    pub process: ProcessStats,
    pub memory: MemoryStats,
    pub storage: StorageStats,
    pub network: NetworkStats,
    pub meta: MetaStats,
    pub gpu: GpuStats,
    pub checksum: u32,
}

impl TelemetryArchive {
    pub fn calculate_checksum(&self) -> u32 {
        let bytes = rkyv::to_bytes::<TelemetryArchive, 65536>(self)
            .expect("TelemetryArchive should always serialize");

        let mut hash_input = Vec::with_capacity(bytes.len() + 4);
        hash_input.extend_from_slice(&bytes);
        hash_input.extend_from_slice(&0u32.to_le_bytes());

        crc32fast::hash(&hash_input)
    }

    pub fn zeroed() -> Self {
        unsafe { std::mem::zeroed() }
    }
}

const _: () = assert!(
    std::mem::size_of::<TelemetryArchive>() <= 65536,
    "TelemetryArchive must fit in 64KB for mmap efficiency"
);
