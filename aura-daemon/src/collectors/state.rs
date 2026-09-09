use aura_common::{AuraError, TelemetryArchive, MAX_NETIFS, PROC_BUFFER_SIZE};

#[derive(Debug)]
pub enum ProviderOutcome<T> {
    Available(T),
    Unavailable,
    Fatal(AuraError),
}

#[derive(Clone, Copy, Debug, Default)]
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

#[derive(Clone, Copy, Debug)]
pub struct NetByteSnapshot {
    pub interfaces: [(u64, u64); MAX_NETIFS],
    pub count: usize,
}

impl Default for NetByteSnapshot {
    fn default() -> Self {
        Self {
            interfaces: [(0, 0); MAX_NETIFS],
            count: 0,
        }
    }
}

impl NetByteSnapshot {
    pub const fn zero() -> Self {
        Self {
            interfaces: [(0, 0); MAX_NETIFS],
            count: 0,
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct CollectorBaselines {
    pub cpu_ticks: CpuTickSnapshot,
    pub net_bytes: NetByteSnapshot,
    pub prev_page_faults: u64,
    pub prev_timestamp_ns: u64,
}

#[derive(Clone, Copy, Debug)]
pub struct FixedCollectorState {
    pub archive: TelemetryArchive,
    pub baselines: CollectorBaselines,
}

impl Default for FixedCollectorState {
    fn default() -> Self {
        Self {
            archive: TelemetryArchive::zeroed(),
            baselines: CollectorBaselines::default(),
        }
    }
}

pub struct CollectorScratch {
    pub proc_buffer: Vec<u8>,
    pub aux_buffer: Vec<u8>,
}

impl Default for CollectorScratch {
    fn default() -> Self {
        Self {
            proc_buffer: Vec::with_capacity(PROC_BUFFER_SIZE),
            aux_buffer: Vec::with_capacity(PROC_BUFFER_SIZE),
        }
    }
}

pub struct CollectorState {
    committed: FixedCollectorState,
    staging: FixedCollectorState,
    scratch: CollectorScratch,
}

impl CollectorState {
    pub fn new() -> Self {
        Self::with_committed(FixedCollectorState::default())
    }

    pub fn with_committed(committed: FixedCollectorState) -> Self {
        Self {
            committed,
            staging: committed,
            scratch: CollectorScratch::default(),
        }
    }

    pub fn committed(&self) -> &FixedCollectorState {
        &self.committed
    }

    pub fn staging(&self) -> &FixedCollectorState {
        &self.staging
    }

    pub fn staging_mut(&mut self) -> &mut FixedCollectorState {
        &mut self.staging
    }

    pub fn scratch_capacities(&self) -> (usize, usize) {
        (
            self.scratch.proc_buffer.capacity(),
            self.scratch.aux_buffer.capacity(),
        )
    }

    pub(crate) fn prepare_staging(&mut self) {
        self.staging = self.committed;
    }

    pub(crate) fn split_staging(&mut self) -> (&mut FixedCollectorState, &mut CollectorScratch) {
        (&mut self.staging, &mut self.scratch)
    }

    pub(crate) fn commit_staging(&mut self) {
        std::mem::swap(&mut self.committed, &mut self.staging);
    }
}

impl Default for CollectorState {
    fn default() -> Self {
        Self::new()
    }
}

pub trait CycleCollector {
    fn collect(
        &mut self,
        state: &mut FixedCollectorState,
        scratch: &mut CollectorScratch,
    ) -> ProviderOutcome<()>;
}
