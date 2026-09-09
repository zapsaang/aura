use aura_common::{
    AuraResult, CAP_GPU_ENUMERATION, CAP_META_LOAD_AVERAGE, CAP_META_OS_IDENTITY,
    CAP_META_OS_VERSION_ID, CAP_META_TIMEZONE, CAP_META_UPTIME, CAP_NETWORK_BYTES,
    CAP_NETWORK_RATES,
};

use super::{cpu, memory, network, CollectorScratch, FixedCollectorState};
#[cfg(target_os = "linux")]
use super::{gpu, meta};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NetworkAvailability {
    pub bytes: bool,
    pub rates: bool,
}

impl NetworkAvailability {
    pub(super) const fn capability_mask(self) -> u64 {
        let mut capabilities = 0;
        if self.bytes {
            capabilities |= CAP_NETWORK_BYTES;
        }
        if self.rates {
            capabilities |= CAP_NETWORK_RATES;
        }
        capabilities
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MetaGpuAvailability {
    pub uptime: bool,
    pub load_average: bool,
    pub timezone: bool,
    pub os_identity: bool,
    pub os_version_id: bool,
    pub gpu_enumeration: bool,
}

impl MetaGpuAvailability {
    pub(super) const fn capability_mask(self) -> u64 {
        let mut capabilities = 0;
        if self.uptime {
            capabilities |= CAP_META_UPTIME;
        }
        if self.load_average {
            capabilities |= CAP_META_LOAD_AVERAGE;
        }
        if self.timezone {
            capabilities |= CAP_META_TIMEZONE;
        }
        if self.os_identity {
            capabilities |= CAP_META_OS_IDENTITY;
        }
        if self.os_version_id {
            capabilities |= CAP_META_OS_VERSION_ID;
        }
        if self.gpu_enumeration {
            capabilities |= CAP_GPU_ENUMERATION;
        }
        capabilities
    }
}

pub trait CollectorSources {
    fn collect_cpu(
        &mut self,
        state: &mut FixedCollectorState,
        scratch: &mut CollectorScratch,
    ) -> AuraResult<cpu::CpuAvailability>;

    fn collect_memory(
        &mut self,
        state: &mut FixedCollectorState,
        scratch: &mut CollectorScratch,
    ) -> AuraResult<memory::MemoryAvailability>;

    fn collect_network(
        &mut self,
        state: &mut FixedCollectorState,
        scratch: &mut CollectorScratch,
    ) -> AuraResult<NetworkAvailability>;

    fn collect_meta_and_gpu(
        &mut self,
        state: &mut FixedCollectorState,
    ) -> AuraResult<MetaGpuAvailability>;
}

#[derive(Default)]
pub struct PlatformSources;

impl CollectorSources for PlatformSources {
    fn collect_cpu(
        &mut self,
        state: &mut FixedCollectorState,
        scratch: &mut CollectorScratch,
    ) -> AuraResult<cpu::CpuAvailability> {
        cpu::collect(&mut scratch.proc_buffer, &mut state.archive.cpu)
    }

    fn collect_memory(
        &mut self,
        state: &mut FixedCollectorState,
        scratch: &mut CollectorScratch,
    ) -> AuraResult<memory::MemoryAvailability> {
        memory::collect(
            &mut scratch.proc_buffer,
            &mut scratch.aux_buffer,
            &mut state.archive.memory,
        )
    }

    fn collect_network(
        &mut self,
        state: &mut FixedCollectorState,
        scratch: &mut CollectorScratch,
    ) -> AuraResult<NetworkAvailability> {
        network::collect(&mut scratch.proc_buffer, &mut state.archive.network)?;
        Ok(NetworkAvailability {
            bytes: cfg!(target_os = "linux"),
            rates: cfg!(target_os = "linux"),
        })
    }

    fn collect_meta_and_gpu(
        &mut self,
        state: &mut FixedCollectorState,
    ) -> AuraResult<MetaGpuAvailability> {
        #[cfg(target_os = "linux")]
        {
            meta::collect(&mut state.archive.meta)?;
            gpu::collect_nvml(&mut state.archive.gpu)?;
            Ok(MetaGpuAvailability {
                uptime: true,
                load_average: true,
                timezone: true,
                os_identity: state.archive.meta.os.os_type.bytes[0] != 0
                    && state.archive.meta.os.os_id.bytes[0] != 0,
                os_version_id: state.archive.meta.os.os_version_id.bytes[0] != 0,
                gpu_enumeration: state.archive.gpu.nvml_available != 0,
            })
        }
        #[cfg(target_os = "macos")]
        {
            state.archive.meta.uptime_secs = crate::platform::macos::boot_time()?;
            Ok(MetaGpuAvailability {
                uptime: true,
                load_average: false,
                timezone: false,
                os_identity: state.archive.meta.os.os_type.bytes[0] != 0
                    && state.archive.meta.os.os_id.bytes[0] != 0,
                os_version_id: state.archive.meta.os.os_version_id.bytes[0] != 0,
                gpu_enumeration: false,
            })
        }
    }
}
