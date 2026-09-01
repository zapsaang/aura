use super::FixedString16;

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct OsFingerprint {
    pub os_type: FixedString16,
    pub os_id: FixedString16,
    pub os_version_id: FixedString16,
    pub os_pretty_name: [u8; 128],
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
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
