pub mod cpu;
pub mod disk;
pub mod gpu;
pub mod heap;
pub mod memory;
pub mod meta;
pub mod network;
pub mod parsing;
pub mod process;

use std::collections::HashMap;

use aura_common::{AuraResult, TelemetryArchive, MAX_DISKS, MAX_NETIFS, PROC_BUFFER_SIZE};

#[derive(Clone, Copy)]
pub struct CpuTickSnapshot {
    pub user: u64,
    pub system: u64,
    pub idle: u64,
    pub total: u64,
    pub context_switches: u64,
}

impl CpuTickSnapshot {
    pub const fn zero() -> Self {
        Self {
            user: 0,
            system: 0,
            idle: 0,
            total: 0,
            context_switches: 0,
        }
    }
}

#[derive(Clone, Copy)]
pub struct DiskSectorSnapshot {
    pub devices: [(u64, u64); MAX_DISKS],
    pub count: usize,
}

impl DiskSectorSnapshot {
    pub const fn zero() -> Self {
        Self {
            devices: [(0, 0); MAX_DISKS],
            count: 0,
        }
    }
}

#[derive(Clone, Copy)]
pub struct NetByteSnapshot {
    pub interfaces: [(u64, u64); MAX_NETIFS],
    pub count: usize,
}

impl NetByteSnapshot {
    pub const fn zero() -> Self {
        Self {
            interfaces: [(0, 0); MAX_NETIFS],
            count: 0,
        }
    }
}

pub struct CollectorState {
    pub telemetry: TelemetryArchive,
    pub prev_cpu_ticks: CpuTickSnapshot,
    pub prev_disk_sectors: DiskSectorSnapshot,
    pub prev_net_bytes: NetByteSnapshot,
    pub prev_page_faults: u64,
    pub prev_timestamp_ns: u64,
    pub prev_proc_total_ticks: u64,
    pub prev_proc_ticks: HashMap<u32, u64>,
    pub proc_fd_cache: process::ProcFdCache,
    pub proc_buffer: [u8; PROC_BUFFER_SIZE],
    pub aux_buffer: [u8; PROC_BUFFER_SIZE],
}

impl CollectorState {
    pub fn new() -> Self {
        Self {
            telemetry: TelemetryArchive::zeroed(),
            prev_cpu_ticks: CpuTickSnapshot::zero(),
            prev_disk_sectors: DiskSectorSnapshot::zero(),
            prev_net_bytes: NetByteSnapshot::zero(),
            prev_page_faults: 0,
            prev_timestamp_ns: 0,
            prev_proc_total_ticks: 0,
            prev_proc_ticks: HashMap::with_capacity(1024),
            proc_fd_cache: process::ProcFdCache::new(),
            proc_buffer: [0; PROC_BUFFER_SIZE],
            aux_buffer: [0; PROC_BUFFER_SIZE],
        }
    }
}

impl Default for CollectorState {
    fn default() -> Self {
        Self::new()
    }
}

pub fn init(state: &mut CollectorState) -> AuraResult<()> {
    #[cfg(target_os = "linux")]
    {
        meta::cache_os_fingerprint(&mut state.telemetry.meta)?;
        gpu::init_nvml(&mut state.telemetry.gpu)?;
    }

    #[cfg(target_os = "macos")]
    {
        crate::platform::macos::init()?;
        state.telemetry.meta.os.os_type = aura_common::FixedString16::from_bytes(b"darwin");
    }

    state.prev_timestamp_ns = aura_common::monotonic_ns();
    Ok(())
}

pub fn collect_all(state: &mut CollectorState) -> AuraResult<()> {
    let now = aura_common::monotonic_ns();
    let delta_secs = if state.prev_timestamp_ns == 0 {
        0.0
    } else {
        (now.saturating_sub(state.prev_timestamp_ns)) as f32 / 1_000_000_000.0
    };

    #[cfg(target_os = "linux")]
    {
        cpu::collect(
            &mut state.proc_buffer,
            &mut state.telemetry.cpu,
            &mut state.prev_cpu_ticks,
            delta_secs,
        )?;

        process::collect_top_n(
            &mut state.proc_buffer,
            &mut state.telemetry.process,
            &mut state.prev_proc_ticks,
            &mut state.prev_proc_total_ticks,
            state.telemetry.cpu.total_ticks,
            state.telemetry.cpu.core_count,
            &mut state.proc_fd_cache,
        )?;

        memory::collect(
            &mut state.proc_buffer,
            &mut state.aux_buffer,
            &mut state.telemetry.memory,
            &mut state.prev_page_faults,
            delta_secs,
        )?;

        disk::collect(
            &mut state.proc_buffer,
            &mut state.aux_buffer,
            &mut state.telemetry.storage,
            &mut state.prev_disk_sectors,
            delta_secs,
        )?;

        network::collect(
            &mut state.proc_buffer,
            &mut state.telemetry.network,
            &mut state.prev_net_bytes,
            delta_secs,
        )?;

        meta::collect(&mut state.telemetry.meta)?;
        gpu::collect_nvml(&mut state.telemetry.gpu)?;
    }

    #[cfg(target_os = "macos")]
    {
        let provider = crate::platform::macos::provider()?;
        state.telemetry.cpu = provider.cpu_stats()?;
        state.telemetry.memory = provider.memory_stats()?;
        state.telemetry.process = provider.process_stats()?;
        disk::collect_macos(
            &mut state.telemetry.storage,
            &mut state.prev_disk_sectors,
            delta_secs,
        )?;
    }

    state.telemetry.meta.timestamp_ns = now;
    state.prev_timestamp_ns = now;
    Ok(())
}
