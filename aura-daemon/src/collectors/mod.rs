pub mod cpu;
pub mod gpu;
pub mod heap;
pub mod memory;
pub mod meta;
pub mod network;
pub mod parsing;
pub mod process;
mod sources;
mod state;
pub mod storage;

use aura_common::{
    AuraResult, CAP_MEMORY_BUFFERS, CAP_MEMORY_CACHED, CAP_MEMORY_PAGE_FAULTS, CAP_MEMORY_RAM_FREE,
    CAP_MEMORY_RAM_TOTAL, CAP_MEMORY_RAM_USED, CAP_MEMORY_SWAP,
};

use crate::finalize::SystemFinalizer;
use crate::lifecycle::Finalizer;

pub use sources::{CollectorSources, MetaGpuAvailability, NetworkAvailability, PlatformSources};
pub use state::{
    CollectorBaselines, CollectorScratch, CollectorState, CpuCoreSnapshot, CpuTickSnapshot,
    CycleCollector, FixedCollectorState, NetByteSnapshot, NetIfKey, NetIfSlot, ProviderOutcome,
    NET_KEY_LEN, NET_MAP_CAPACITY,
};
pub use storage::StorageAvailability;

pub struct SystemCollector<S = PlatformSources> {
    sources: S,
}

impl<S> SystemCollector<S> {
    pub const fn with_sources(sources: S) -> Self {
        Self { sources }
    }

    pub const fn sources(&self) -> &S {
        &self.sources
    }
}

impl Default for SystemCollector<PlatformSources> {
    fn default() -> Self {
        Self::with_sources(PlatformSources)
    }
}

impl<S: CollectorSources> CycleCollector for SystemCollector<S> {
    fn collect(
        &mut self,
        state: &mut FixedCollectorState,
        scratch: &mut CollectorScratch,
    ) -> ProviderOutcome<()> {
        match collect_fixed(&mut self.sources, state, scratch) {
            Ok(()) => ProviderOutcome::Available(()),
            Err(error) => ProviderOutcome::Fatal(error),
        }
    }
}

pub fn init(state: &mut CollectorState) -> AuraResult<()> {
    let fixed = state.staging_mut();
    #[cfg(target_os = "linux")]
    {
        meta::cache_os_fingerprint(&mut fixed.archive.meta)?;
        gpu::init_nvml(&mut fixed.archive.gpu)?;
        fixed.baselines.process_page_size = process::cache_page_size()?;
    }
    #[cfg(target_os = "macos")]
    {
        crate::platform::macos::init()?;
        crate::platform::macos::cache_os_fingerprint(&mut fixed.archive.meta)?;
    }
    state.commit_staging();
    Ok(())
}

pub fn collect_sample(state: &mut CollectorState) -> AuraResult<&FixedCollectorState> {
    state.prepare_staging();
    let mut sources = PlatformSources;
    let result = {
        let (fixed, scratch) = state.split_staging();
        collect_fixed(&mut sources, fixed, scratch)
    };
    result?;
    SystemFinalizer::default().finalize(state.staging_mut())?;
    Ok(state.staging())
}

fn collect_fixed<S: CollectorSources>(
    sources: &mut S,
    state: &mut FixedCollectorState,
    scratch: &mut CollectorScratch,
) -> AuraResult<()> {
    state.archive.capabilities = 0;

    let cpu_availability = sources.collect_cpu(state, scratch)?;
    state.cpu_over_capacity = cpu_availability.over_capacity;
    state.archive.capabilities |= cpu_availability.capability_mask();

    let memory_availability = sources.collect_memory(state, scratch)?;
    state.archive.capabilities |= CAP_MEMORY_RAM_TOTAL | CAP_MEMORY_RAM_FREE | CAP_MEMORY_RAM_USED;
    if memory_availability.buffers {
        state.archive.capabilities |= CAP_MEMORY_BUFFERS;
    }
    if memory_availability.cached {
        state.archive.capabilities |= CAP_MEMORY_CACHED;
    }
    if memory_availability.swap {
        state.archive.capabilities |= CAP_MEMORY_SWAP;
    }
    if memory_availability.page_faults {
        state.archive.capabilities |= CAP_MEMORY_PAGE_FAULTS;
    }

    let network_availability = sources.collect_network(state, scratch)?;
    state.archive.capabilities |= network_availability.capability_mask();

    let storage_availability = sources.collect_storage(state, scratch)?;
    state.archive.capabilities |= storage_availability.capability_mask();

    let process_availability = sources.collect_process(state, scratch)?;
    state.archive.capabilities |= process_availability.capability_mask();

    let meta_gpu_availability = sources.collect_meta_and_gpu(state)?;
    state.archive.capabilities |= meta_gpu_availability.capability_mask();
    Ok(())
}
