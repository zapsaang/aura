pub const TONE_GREEN: u8 = 0;
pub const TONE_MAGENTA: u8 = 1;
pub const TONE_YELLOW: u8 = 2;
pub const TONE_RED: u8 = 3;
pub const TONE_MAX: u8 = TONE_RED;

#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct DerivedStats {
    pub ram_used_percent: f32,
    pub swap_used_percent: f32,
    pub aggregate_rx_bytes_per_sec: f32,
    pub aggregate_tx_bytes_per_sec: f32,
    pub cpu_tone: u8,
    pub ram_tone: u8,
    pub swap_tone: u8,
    pub _reserved0: u8,
    pub _pad0: [u8; 4],
}
