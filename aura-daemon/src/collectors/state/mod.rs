use aura_common::{
    AuraError, TelemetryArchive, MAX_CORES, MAX_DISKS, MAX_NETIFS, PROC_BUFFER_SIZE,
};

use super::process::state::ProcessBaseline;
use super::storage::state::{DiskBaselineMap, DiskRawSnapshot};

mod network;

pub use network::{NetByteSnapshot, NetIfKey, NetIfSlot, NET_KEY_LEN, NET_MAP_CAPACITY};

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

#[derive(Clone, Copy, Debug, Default)]
pub struct CpuCoreSnapshot {
    pub user: u64,
    pub system: u64,
    pub idle: u64,
    pub total: u64,
}

impl CpuCoreSnapshot {
    pub const fn zero() -> Self {
        Self {
            user: 0,
            system: 0,
            idle: 0,
            total: 0,
        }
    }
}

#[derive(Clone, Debug)]
pub struct CollectorBaselines {
    pub cpu_ticks: CpuTickSnapshot,
    pub cores: [CpuCoreSnapshot; MAX_CORES],
    pub core_count: u8,
    pub net_bytes: NetByteSnapshot,
    pub disk: DiskBaselineMap,
    pub process: Box<ProcessBaseline>,
    pub process_page_size: u64,
    pub prev_page_faults: u64,
    pub prev_timestamp_ns: u64,
}

impl CollectorBaselines {
    /// Copies committed baselines into staging; the boxed process table is
    /// memcpy'd in place so no allocation happens per cycle.
    fn copy_from(&mut self, src: &Self) {
        self.cpu_ticks = src.cpu_ticks;
        self.cores = src.cores;
        self.core_count = src.core_count;
        self.net_bytes = src.net_bytes;
        self.disk = src.disk;
        *self.process = *src.process;
        self.process_page_size = src.process_page_size;
        self.prev_page_faults = src.prev_page_faults;
        self.prev_timestamp_ns = src.prev_timestamp_ns;
    }
}

impl Default for CollectorBaselines {
    fn default() -> Self {
        Self {
            cpu_ticks: CpuTickSnapshot::default(),
            cores: [CpuCoreSnapshot::default(); MAX_CORES],
            core_count: 0,
            net_bytes: NetByteSnapshot::zero(),
            disk: DiskBaselineMap::zero(),
            process: Box::new(ProcessBaseline::default()),
            process_page_size: 0,
            prev_page_faults: 0,
            prev_timestamp_ns: 0,
        }
    }
}

#[derive(Clone, Debug)]
pub struct FixedCollectorState {
    pub archive: TelemetryArchive,
    pub baselines: CollectorBaselines,
    pub cpu_over_capacity: bool,
    /// Raw disk counters for the current cycle, aligned with
    /// `archive.storage.disks`; consumed by the finalize rate pass.
    pub disk_raw: [DiskRawSnapshot; MAX_DISKS],
    /// Per-cycle network identity keys aligned with
    /// `archive.network.interfaces`; macOS fills (sdl_index, name) keys,
    /// other producers leave them empty so the finalize pass derives the
    /// legacy name-only key.
    pub net_keys: [NetIfKey; MAX_NETIFS],
}

impl Default for FixedCollectorState {
    fn default() -> Self {
        Self {
            archive: TelemetryArchive::zeroed(),
            baselines: CollectorBaselines::default(),
            cpu_over_capacity: false,
            disk_raw: [DiskRawSnapshot::zero(); MAX_DISKS],
            net_keys: [NetIfKey::empty(); MAX_NETIFS],
        }
    }
}

/// Storage scratch covers both `/proc/diskstats` and `/proc/self/mountinfo`
/// sequentially; it is sized once so mount-heavy systems never trigger a
/// mid-cycle allocation.
pub const STORAGE_BUFFER_SIZE: usize = 1024 * 1024;

pub struct CollectorScratch {
    pub proc_buffer: Vec<u8>,
    pub aux_buffer: Vec<u8>,
    pub process_path_buffer: Vec<u8>,
    pub storage_buffer: Vec<u8>,
}

impl Default for CollectorScratch {
    fn default() -> Self {
        Self {
            proc_buffer: Vec::with_capacity(PROC_BUFFER_SIZE),
            aux_buffer: Vec::with_capacity(PROC_BUFFER_SIZE),
            process_path_buffer: Vec::with_capacity(PROC_BUFFER_SIZE),
            storage_buffer: Vec::with_capacity(STORAGE_BUFFER_SIZE),
        }
    }
}

pub struct CollectorState {
    committed: Box<FixedCollectorState>,
    staging: Box<FixedCollectorState>,
    scratch: CollectorScratch,
}

impl CollectorState {
    pub fn new() -> Self {
        Self::with_committed(FixedCollectorState::default())
    }

    pub fn with_committed(committed: FixedCollectorState) -> Self {
        Self {
            staging: Box::new(committed.clone()),
            committed: Box::new(committed),
            scratch: CollectorScratch::default(),
        }
    }

    pub fn committed(&self) -> &FixedCollectorState {
        self.committed.as_ref()
    }

    pub fn staging(&self) -> &FixedCollectorState {
        self.staging.as_ref()
    }

    pub fn staging_mut(&mut self) -> &mut FixedCollectorState {
        self.staging.as_mut()
    }

    pub fn scratch_capacities(&self) -> (usize, usize) {
        (
            self.scratch.proc_buffer.capacity(),
            self.scratch.aux_buffer.capacity(),
        )
    }

    pub(crate) fn prepare_staging(&mut self) {
        self.staging.archive = self.committed.archive;
        self.staging.baselines.copy_from(&self.committed.baselines);
        self.staging.cpu_over_capacity = false;
    }

    pub(crate) fn split_staging(&mut self) -> (&mut FixedCollectorState, &mut CollectorScratch) {
        (self.staging.as_mut(), &mut self.scratch)
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
