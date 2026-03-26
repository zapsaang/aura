use aura_common::{AuraResult, GpuStats};

#[cfg(feature = "gpu-nvml")]
mod imp {
    use std::sync::{Mutex, OnceLock};

    use aura_common::{AuraResult, FixedString16, GpuStat, GpuStats};
    use nvml_wrapper::enum_wrappers::device::TemperatureSensor;
    use nvml_wrapper::NVML;

    static NVML_INSTANCE: OnceLock<Mutex<Option<NVML>>> = OnceLock::new();

    fn nvml_store() -> &'static Mutex<Option<NVML>> {
        NVML_INSTANCE.get_or_init(|| Mutex::new(None))
    }

    pub fn init_nvml(gpu: &mut GpuStats) -> AuraResult<()> {
        gpu.gpu_count = 0;
        gpu.nvml_available = 0;

        let nvml = match NVML::init() {
            Ok(n) => n,
            Err(_) => return Ok(()),
        };

        let count = nvml.device_count().unwrap_or(0).min(8);
        gpu.gpu_count = count as u8;
        gpu.nvml_available = 1;

        if let Ok(mut guard) = nvml_store().lock() {
            *guard = Some(nvml);
        }

        Ok(())
    }

    pub fn collect_nvml(gpu: &mut GpuStats) -> AuraResult<()> {
        if gpu.nvml_available == 0 {
            return Ok(());
        }

        let Ok(guard) = nvml_store().lock() else {
            return Ok(());
        };
        let Some(nvml) = guard.as_ref() else {
            return Ok(());
        };

        let mut i = 0usize;
        let count = gpu.gpu_count as usize;
        while i < count && i < gpu.gpus.len() {
            let mut stat = GpuStat {
                name: FixedString16::new(),
                memory_total: 0,
                memory_used: 0,
                utilization_percent: 0.0,
                power_watts: 0.0,
                temperature_celsius: 0,
                available: 1,
            };

            if let Ok(device) = nvml.device_by_index(i as u32) {
                if let Ok(name) = device.name() {
                    stat.name = FixedString16::from_bytes(name.as_bytes());
                }
                if let Ok(memory) = device.memory_info() {
                    stat.memory_total = memory.total;
                    stat.memory_used = memory.used;
                }
                if let Ok(util) = device.utilization_rates() {
                    stat.utilization_percent = util.gpu as f32;
                }
                if let Ok(power_mw) = device.power_usage() {
                    stat.power_watts = power_mw as f32 / 1000.0;
                }
                if let Ok(temp) = device.temperature(TemperatureSensor::Gpu) {
                    stat.temperature_celsius = temp as i16;
                }
            } else {
                stat.available = 0;
            }

            gpu.gpus[i] = stat;
            i += 1;
        }

        Ok(())
    }
}

#[cfg(not(feature = "gpu-nvml"))]
mod imp {
    use aura_common::{AuraResult, GpuStats};

    pub fn init_nvml(gpu: &mut GpuStats) -> AuraResult<()> {
        gpu.gpu_count = 0;
        gpu.nvml_available = 0;
        Ok(())
    }

    pub fn collect_nvml(_gpu: &mut GpuStats) -> AuraResult<()> {
        Ok(())
    }
}

pub fn init_nvml(gpu: &mut GpuStats) -> AuraResult<()> {
    imp::init_nvml(gpu)
}

pub fn collect_nvml(gpu: &mut GpuStats) -> AuraResult<()> {
    imp::collect_nvml(gpu)
}
