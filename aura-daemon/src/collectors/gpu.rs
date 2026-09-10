//! GPU telemetry through dynamically loaded NVML with graceful degradation.
//!
//! The probe trait keeps the archive conversion logic testable without a
//! real GPU; the NVML-backed probe lives behind `gpu-nvml` on Linux only.

use aura_common::{AuraResult, FixedString16, GpuStat, GpuStats};
use aura_common::{GPU_CAP_MEMORY_TOTAL, GPU_CAP_MEMORY_USED, GPU_CAP_NAME, GPU_CAP_POWER};
use aura_common::{GPU_CAP_TEMPERATURE, GPU_CAP_UTILIZATION, MAX_GPUS};

const ZERO_STAT: GpuStat = GpuStat {
    name: FixedString16::new(),
    memory_total: 0,
    memory_used: 0,
    utilization_percent: 0.0,
    power_watts: 0.0,
    temperature_celsius: 0,
    available: 0,
    tone: 0,
    _pad0: [0; 4],
    capabilities: 0,
};

const ZERO_STATS: GpuStats = GpuStats {
    gpus: [ZERO_STAT; MAX_GPUS],
    gpu_count: 0,
    nvml_available: 0,
    truncated: 0,
    _pad0: [0; 5],
};

/// Opaque probe failure; the collector only distinguishes success/failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProbeFailure;

/// One attempt at reading a single device; handle acquisition is separate.
pub struct DeviceReading {
    pub name: Result<String, ProbeFailure>,
    pub memory_bytes: Result<(u64, u64), ProbeFailure>,
    pub utilization_percent: Result<u32, ProbeFailure>,
    pub power_milliwatts: Result<u32, ProbeFailure>,
    pub temperature_celsius: Result<u32, ProbeFailure>,
}

impl DeviceReading {
    /// A successful handle always yields `available = 1`; each failed metric
    /// clears only its own capability bits and zeroes only its own storage.
    fn into_stat(self) -> GpuStat {
        let mut stat = ZERO_STAT;
        stat.available = 1;
        if let Ok(name) = self.name {
            stat.name = FixedString16::from_bytes(name.as_bytes());
            stat.capabilities |= GPU_CAP_NAME;
        }
        if let Ok((total, used)) = self.memory_bytes {
            stat.memory_total = total;
            stat.memory_used = used;
            stat.capabilities |= GPU_CAP_MEMORY_TOTAL | GPU_CAP_MEMORY_USED;
        }
        if let Ok(percent) = self.utilization_percent {
            stat.utilization_percent = percent as f32;
            stat.capabilities |= GPU_CAP_UTILIZATION;
        }
        if let Ok(milliwatts) = self.power_milliwatts {
            stat.power_watts = milliwatts as f32 / 1000.0;
            stat.capabilities |= GPU_CAP_POWER;
        }
        if let Ok(celsius) = self.temperature_celsius {
            stat.temperature_celsius = celsius as i16;
            stat.capabilities |= GPU_CAP_TEMPERATURE;
        }
        stat
    }
}

/// Minimal device enumeration surface so tests can script NVML behaviour.
pub trait NvmlProbe {
    fn device_count(&mut self) -> Result<u32, ProbeFailure>;
    /// `Ok` means the handle was acquired (metrics may still fail); `Err`
    /// means the handle failed and the index produces no record.
    fn read_device(&mut self, index: u32) -> Result<DeviceReading, ProbeFailure>;
}

/// Initialise GPU state from a probe-creation attempt. Init failure leaves
/// the whole dimension cleared; a post-init count failure also clears it
/// locally, without logging and without failing the cycle.
pub fn init_gpu<P: NvmlProbe>(gpu: &mut GpuStats, probe: Result<P, ProbeFailure>) -> Option<P> {
    *gpu = ZERO_STATS;
    let mut probe = match probe {
        Ok(probe) => probe,
        Err(ProbeFailure) => return None,
    };
    match probe.device_count() {
        Ok(_) => {
            gpu.nvml_available = 1;
            Some(probe)
        }
        Err(ProbeFailure) => None,
    }
}

/// One collection cycle. Only indices `0..min(device_count, MAX_GPUS)` are
/// visited; successful handles are compacted in original index order.
pub fn collect_gpu<P: NvmlProbe>(gpu: &mut GpuStats, probe: &mut P) {
    let device_count = match probe.device_count() {
        Ok(count) => count,
        Err(ProbeFailure) => {
            *gpu = ZERO_STATS;
            return;
        }
    };
    let visited = (device_count as usize).min(MAX_GPUS);
    let mut records = [ZERO_STAT; MAX_GPUS];
    let mut produced = 0usize;
    let mut truncated = u8::from(device_count as usize > MAX_GPUS);
    for index in 0..visited {
        match probe.read_device(index as u32) {
            Ok(reading) => {
                records[produced] = reading.into_stat();
                produced += 1;
            }
            Err(ProbeFailure) => truncated = 1,
        }
    }
    gpu.gpus = records;
    gpu.gpu_count = produced as u8;
    gpu.nvml_available = 1;
    gpu.truncated = truncated;
    gpu._pad0 = [0; 5];
}

#[cfg(all(feature = "gpu-nvml", target_os = "linux"))]
mod imp {
    use std::sync::{Mutex, OnceLock};

    use aura_common::{AuraResult, GpuStats};
    use nvml_wrapper::enum_wrappers::device::TemperatureSensor;
    use nvml_wrapper::Nvml;

    use super::{collect_gpu, init_gpu, DeviceReading, NvmlProbe, ProbeFailure};

    struct RealNvml {
        nvml: Nvml,
    }

    impl NvmlProbe for RealNvml {
        fn device_count(&mut self) -> Result<u32, ProbeFailure> {
            self.nvml.device_count().map_err(|_| ProbeFailure)
        }

        fn read_device(&mut self, index: u32) -> Result<DeviceReading, ProbeFailure> {
            let device = self.nvml.device_by_index(index).map_err(|_| ProbeFailure)?;
            Ok(DeviceReading {
                name: device.name().map_err(|_| ProbeFailure),
                memory_bytes: device
                    .memory_info()
                    .map(|info| (info.total, info.used))
                    .map_err(|_| ProbeFailure),
                utilization_percent: device
                    .utilization_rates()
                    .map(|rates| rates.gpu)
                    .map_err(|_| ProbeFailure),
                power_milliwatts: device.power_usage().map_err(|_| ProbeFailure),
                temperature_celsius: device
                    .temperature(TemperatureSensor::Gpu)
                    .map_err(|_| ProbeFailure),
            })
        }
    }

    static NVML_INSTANCE: OnceLock<Mutex<Option<RealNvml>>> = OnceLock::new();

    fn nvml_store() -> &'static Mutex<Option<RealNvml>> {
        NVML_INSTANCE.get_or_init(|| Mutex::new(None))
    }

    pub fn init_nvml(gpu: &mut GpuStats) -> AuraResult<()> {
        let probe = Nvml::init()
            .map(|nvml| RealNvml { nvml })
            .map_err(|_| ProbeFailure);
        let stored = init_gpu(gpu, probe);
        if let Ok(mut guard) = nvml_store().lock() {
            *guard = stored;
        }
        Ok(())
    }

    pub fn collect_nvml(gpu: &mut GpuStats) -> AuraResult<()> {
        if gpu.nvml_available == 0 {
            return Ok(());
        }
        let Ok(mut guard) = nvml_store().lock() else {
            *gpu = super::ZERO_STATS;
            return Ok(());
        };
        let Some(real) = guard.as_mut() else {
            *gpu = super::ZERO_STATS;
            return Ok(());
        };
        collect_gpu(gpu, real);
        Ok(())
    }
}

#[cfg(not(all(feature = "gpu-nvml", target_os = "linux")))]
mod imp {
    use aura_common::{AuraResult, GpuStats};

    use super::ZERO_STATS;

    pub fn init_nvml(gpu: &mut GpuStats) -> AuraResult<()> {
        *gpu = ZERO_STATS;
        Ok(())
    }

    pub fn collect_nvml(gpu: &mut GpuStats) -> AuraResult<()> {
        *gpu = ZERO_STATS;
        Ok(())
    }
}

pub fn init_nvml(gpu: &mut GpuStats) -> AuraResult<()> {
    imp::init_nvml(gpu)
}

pub fn collect_nvml(gpu: &mut GpuStats) -> AuraResult<()> {
    imp::collect_nvml(gpu)
}
