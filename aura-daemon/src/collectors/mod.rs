pub mod cpu;
pub mod gpu;
pub mod heap;
pub mod memory;
pub mod meta;
pub mod network;
pub mod parsing;

use aura_common::{AuraResult, TelemetryArchive, MAX_NETIFS, MIN_DELTA_NS, PROC_BUFFER_SIZE};

const NS_PER_SEC: f64 = 1_000_000_000.0;

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
    pub prev_net_bytes: NetByteSnapshot,
    pub prev_page_faults: u64,
    pub prev_timestamp_ns: u64,
    pub proc_buffer: Vec<u8>,
    pub aux_buffer: Vec<u8>,
}

impl CollectorState {
    pub fn new() -> Self {
        Self {
            telemetry: TelemetryArchive::zeroed(),
            prev_cpu_ticks: CpuTickSnapshot::zero(),
            prev_net_bytes: NetByteSnapshot::zero(),
            prev_page_faults: 0,
            prev_timestamp_ns: 0,
            proc_buffer: Vec::with_capacity(PROC_BUFFER_SIZE),
            aux_buffer: Vec::with_capacity(PROC_BUFFER_SIZE),
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
        crate::platform::macos::cache_os_fingerprint(&mut state.telemetry.meta)?;
    }

    state.prev_timestamp_ns = aura_common::monotonic_ns();
    Ok(())
}

pub fn collect_all(state: &mut CollectorState) -> AuraResult<()> {
    let now = aura_common::monotonic_ns();
    let raw_delta_ns = now.saturating_sub(state.prev_timestamp_ns);

    let delta_secs: f64 = if state.prev_timestamp_ns == 0 {
        0.0
    } else if raw_delta_ns < MIN_DELTA_NS {
        log::warn!(
            "Suspiciously fast collection: delta_ns={} < MIN_DELTA_NS={}",
            raw_delta_ns,
            MIN_DELTA_NS
        );
        MIN_DELTA_NS as f64 / NS_PER_SEC
    } else {
        raw_delta_ns as f64 / NS_PER_SEC
    };

    cpu::collect(
        &mut state.proc_buffer,
        &mut state.telemetry.cpu,
        &mut state.prev_cpu_ticks,
        delta_secs,
    )?;

    memory::collect(
        &mut state.proc_buffer,
        &mut state.aux_buffer,
        &mut state.telemetry.memory,
        &mut state.prev_page_faults,
        delta_secs,
    )?;

    network::collect(
        &mut state.proc_buffer,
        &mut state.telemetry.network,
        &mut state.prev_net_bytes,
        delta_secs,
    )?;

    collect_meta_and_gpu(state)?;

    state.prev_timestamp_ns = now;
    Ok(())
}

fn collect_meta_and_gpu(state: &mut CollectorState) -> AuraResult<()> {
    #[cfg(target_os = "linux")]
    {
        meta::collect(&mut state.telemetry.meta)?;
        gpu::collect_nvml(&mut state.telemetry.gpu)?;
    }

    #[cfg(target_os = "macos")]
    {
        state.telemetry.meta.timestamp_ns = aura_common::monotonic_ns();
        if let Ok(uptime) = crate::platform::macos::boot_time() {
            state.telemetry.meta.uptime_secs = uptime;
        }
    }

    Ok(())
}
