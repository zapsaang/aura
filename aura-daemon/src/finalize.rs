use aura_common::{
    AuraError, AuraResult, TelemetryArchive, ARCHIVE_VERSION, CAP_CPU_CONTEXT_SWITCHES,
    CAP_CPU_GLOBAL, CAP_CPU_PER_CORE, CAP_GPU_ENUMERATION, CAP_MEMORY_BUFFERS, CAP_MEMORY_CACHED,
    CAP_MEMORY_PAGE_FAULTS, CAP_MEMORY_RAM_FREE, CAP_MEMORY_RAM_TOTAL, CAP_MEMORY_RAM_USED,
    CAP_MEMORY_SWAP, CAP_META_LOAD_AVERAGE, CAP_META_OS_CODENAME, CAP_META_OS_IDENTITY,
    CAP_META_OS_VERSION, CAP_META_OS_VERSION_ID, CAP_META_TIMEZONE, CAP_META_UPTIME,
    CAP_META_WALLCLOCK, CAP_NETWORK_BYTES, CAP_NETWORK_RATES, MAX_CORES,
};

use crate::collectors::FixedCollectorState;
use crate::lifecycle::Finalizer;

mod rates;

#[derive(Clone, Copy, Debug)]
pub struct ClockSample {
    pub monotonic_ns: u64,
    pub wallclock_ns: u64,
}

pub trait Clock {
    fn sample(&mut self) -> AuraResult<ClockSample>;
}

pub struct PosixClock;

impl Clock for PosixClock {
    fn sample(&mut self) -> AuraResult<ClockSample> {
        Ok(ClockSample {
            monotonic_ns: clock_ns(libc::CLOCK_MONOTONIC)?,
            wallclock_ns: clock_ns(libc::CLOCK_REALTIME)?,
        })
    }
}

pub struct SystemFinalizer<C = PosixClock> {
    clock: C,
}

impl Default for SystemFinalizer<PosixClock> {
    fn default() -> Self {
        Self { clock: PosixClock }
    }
}

impl<C> SystemFinalizer<C> {
    pub const fn new(clock: C) -> Self {
        Self { clock }
    }
}

impl<C: Clock> Finalizer for SystemFinalizer<C> {
    fn finalize(&mut self, state: &mut FixedCollectorState) -> AuraResult<()> {
        let sample = self.clock.sample()?;
        if sample.monotonic_ns == 0 || sample.wallclock_ns == 0 {
            return Err(AuraError::Fatal("clock returned zero".to_string()));
        }
        rates::apply(state, sample.monotonic_ns);
        state.archive.meta.timestamp_ns = sample.monotonic_ns;
        state.archive.meta.wallclock_ns = sample.wallclock_ns;
        state.archive.capabilities |= CAP_META_WALLCLOCK;
        zero_unowned(&mut state.archive);
        state.archive.version = ARCHIVE_VERSION;
        state.archive.checksum = 0;
        state.archive.checksum = state.archive.calculate_checksum();
        Ok(())
    }
}

fn clock_ns(clock_id: libc::clockid_t) -> AuraResult<u64> {
    let mut value = std::mem::MaybeUninit::<libc::timespec>::uninit();
    // SAFETY: value points to writable timespec storage and clock_id is a supported POSIX clock.
    let result = unsafe { libc::clock_gettime(clock_id, value.as_mut_ptr()) };
    if result != 0 {
        return Err(AuraError::Fatal(format!(
            "clock_gettime failed: {}",
            std::io::Error::last_os_error()
        )));
    }
    // SAFETY: clock_gettime returned success and initialized the timespec.
    let value = unsafe { value.assume_init() };
    if value.tv_sec < 0 || !(0..1_000_000_000).contains(&value.tv_nsec) {
        return Err(AuraError::Fatal(
            "clock_gettime returned invalid value".to_string(),
        ));
    }
    let seconds = u64::try_from(value.tv_sec)
        .map_err(|_| AuraError::Fatal("clock seconds overflow".to_string()))?;
    let nanos = u64::try_from(value.tv_nsec)
        .map_err(|_| AuraError::Fatal("clock nanoseconds overflow".to_string()))?;
    seconds
        .checked_mul(1_000_000_000)
        .and_then(|base| base.checked_add(nanos))
        .ok_or_else(|| AuraError::Fatal("clock nanoseconds overflow".to_string()))
}

fn zero_unowned(archive: &mut TelemetryArchive) {
    let zero = TelemetryArchive::zeroed();
    let caps = archive.capabilities;
    if caps & CAP_CPU_GLOBAL == 0 {
        archive.cpu = zero.cpu;
    } else {
        if caps & CAP_CPU_CONTEXT_SWITCHES == 0 {
            archive.cpu.context_switches = 0;
            archive.cpu.context_switches_per_sec = 0.0;
        }
        let represented = if caps & CAP_CPU_PER_CORE == 0 {
            0
        } else {
            (archive.cpu.core_count as usize).min(MAX_CORES)
        };
        for core in &mut archive.cpu.cores[represented..] {
            *core = zero.cpu.cores[0];
        }
    }
    archive.process = zero.process;
    zero_memory(archive, &zero, caps);
    archive.storage = zero.storage;
    if caps & CAP_NETWORK_BYTES == 0 {
        archive.network = zero.network;
    } else if caps & CAP_NETWORK_RATES == 0 {
        for interface in &mut archive.network.interfaces {
            interface.rx_bytes_per_sec = 0.0;
            interface.tx_bytes_per_sec = 0.0;
        }
    }
    let represented = (archive.network.if_count as usize).min(archive.network.interfaces.len());
    for interface in &mut archive.network.interfaces[represented..] {
        *interface = zero.network.interfaces[0];
    }
    zero_meta(archive, &zero, caps);
    if caps & CAP_GPU_ENUMERATION == 0 {
        archive.gpu = zero.gpu;
    } else {
        zero_gpu_records(archive, &zero);
    }
}

fn zero_gpu_records(archive: &mut TelemetryArchive, zero: &TelemetryArchive) {
    let caps = aura_common::GPU_CAP_NAME
        | aura_common::GPU_CAP_MEMORY_TOTAL
        | aura_common::GPU_CAP_MEMORY_USED
        | aura_common::GPU_CAP_UTILIZATION
        | aura_common::GPU_CAP_POWER
        | aura_common::GPU_CAP_TEMPERATURE;
    for gpu in &mut archive.gpu.gpus {
        let owned = gpu.capabilities;
        if owned & caps == 0 {
            *gpu = zero.gpu.gpus[0];
        } else {
            if owned & aura_common::GPU_CAP_NAME == 0 {
                gpu.name = zero.gpu.gpus[0].name;
            }
            if owned & aura_common::GPU_CAP_MEMORY_TOTAL == 0 {
                gpu.memory_total = 0;
            }
            if owned & aura_common::GPU_CAP_MEMORY_USED == 0 {
                gpu.memory_used = 0;
            }
            if owned & aura_common::GPU_CAP_UTILIZATION == 0 {
                gpu.utilization_percent = 0.0;
            }
            if owned & aura_common::GPU_CAP_POWER == 0 {
                gpu.power_watts = 0.0;
            }
            if owned & aura_common::GPU_CAP_TEMPERATURE == 0 {
                gpu.temperature_celsius = 0;
                gpu.tone = 0;
            }
        }
    }
}

fn zero_memory(archive: &mut TelemetryArchive, zero: &TelemetryArchive, caps: u64) {
    let memory = &mut archive.memory;
    let empty = &zero.memory;
    if caps & CAP_MEMORY_RAM_TOTAL == 0 {
        memory.ram_total = empty.ram_total;
    }
    if caps & CAP_MEMORY_RAM_FREE == 0 {
        memory.ram_free = empty.ram_free;
    }
    if caps & CAP_MEMORY_RAM_USED == 0 {
        memory.ram_used = empty.ram_used;
    }
    if caps & CAP_MEMORY_BUFFERS == 0 {
        memory.buffers = empty.buffers;
    }
    if caps & CAP_MEMORY_CACHED == 0 {
        memory.cached = empty.cached;
    }
    if caps & CAP_MEMORY_SWAP == 0 {
        memory.swap_total = 0;
        memory.swap_free = 0;
        memory.swap_used = 0;
    }
    if caps & CAP_MEMORY_PAGE_FAULTS == 0 {
        memory.page_faults = 0;
        memory.page_faults_per_sec = 0.0;
    }
}

fn zero_meta(archive: &mut TelemetryArchive, zero: &TelemetryArchive, caps: u64) {
    let meta = &mut archive.meta;
    if caps & CAP_META_UPTIME == 0 {
        meta.uptime_secs = 0;
    }
    if caps & CAP_META_LOAD_AVERAGE == 0 {
        meta.load_avg_1m = 0.0;
        meta.load_avg_5m = 0.0;
        meta.load_avg_15m = 0.0;
    }
    if caps & CAP_META_TIMEZONE == 0 {
        meta.timezone_name = [0; 8];
        meta.timezone_offset_secs = 0;
    }
    if caps & CAP_META_OS_IDENTITY == 0 {
        meta.os.os_type = zero.meta.os.os_type;
        meta.os.os_id = zero.meta.os.os_id;
        meta.os.os_pretty_name = [0; 128];
    }
    if caps & CAP_META_OS_VERSION == 0 {
        meta.os.version = [0; 64];
    }
    if caps & CAP_META_OS_VERSION_ID == 0 {
        meta.os.os_version_id = zero.meta.os.os_version_id;
    }
    if caps & CAP_META_OS_CODENAME == 0 {
        meta.os.version_codename = zero.meta.os.version_codename;
    }
}

#[cfg(test)]
mod tests {
    use aura_common::{CAP_CPU_CONTEXT_SWITCHES, CAP_CPU_GLOBAL, CAP_MEMORY_PAGE_FAULTS};

    use super::rates;
    use crate::collectors::FixedCollectorState;

    #[test]
    fn warmed_zero_counter_baselines_still_produce_rates() {
        let mut state = FixedCollectorState::default();
        state.archive.capabilities =
            CAP_CPU_GLOBAL | CAP_CPU_CONTEXT_SWITCHES | CAP_MEMORY_PAGE_FAULTS;
        state.archive.cpu.total_ticks = 10;
        state.archive.cpu.idle_ticks = 4;
        state.archive.cpu.context_switches = 5;
        state.archive.memory.page_faults = 7;
        state.baselines.prev_timestamp_ns = 1_000_000_000;

        rates::apply(&mut state, 2_000_000_000);

        assert_eq!(state.archive.cpu.usage_percent, 60.0);
        assert_eq!(state.archive.cpu.context_switches_per_sec, 5.0);
        assert_eq!(state.archive.memory.page_faults_per_sec, 7.0);
    }
}
