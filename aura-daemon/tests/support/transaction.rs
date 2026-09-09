use std::fs::File;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::Duration;

use aura_common::{
    read_double_buffer, AuraError, AuraResult, TelemetryArchive, CAP_META_UPTIME, MAX_NETIFS,
    SHM_SIZE,
};
use aura_daemon::collectors::{
    CollectorScratch, CollectorState, CycleCollector, FixedCollectorState, ProviderOutcome,
    SystemCollector,
};
use aura_daemon::finalize::{Clock, ClockSample, SystemFinalizer};
use aura_daemon::lifecycle::{
    Finalizer, Lifecycle, LifecycleParts, Notification, Notifier, Publisher, Sleeper,
};
use aura_daemon::state::ShmHandle;
use memmap2::{Mmap, MmapOptions};
use tempfile::TempDir;

use super::system_sources::DeterministicSources;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Failure {
    None,
    Collector,
    Finalizer,
    Publisher,
}

pub struct ScriptedCollector {
    pub calls: usize,
    pub failure: Failure,
    pub unavailable: bool,
    pub seen_baselines: [u64; 4],
}

impl CycleCollector for ScriptedCollector {
    fn collect(
        &mut self,
        state: &mut FixedCollectorState,
        _scratch: &mut CollectorScratch,
    ) -> ProviderOutcome<()> {
        self.seen_baselines[self.calls] = state.baselines.prev_page_faults;
        self.calls += 1;
        state.baselines.cpu_ticks.user += 1;
        state.baselines.cpu_ticks.system += 2;
        state.baselines.cpu_ticks.idle += 3;
        state.baselines.cpu_ticks.total += 4;
        state.baselines.cpu_ticks.context_switches += 5;
        state.baselines.net_bytes.interfaces[0].0 += 6;
        state.baselines.net_bytes.interfaces[0].1 += 7;
        state.baselines.net_bytes.interfaces[1].0 += 8;
        state.baselines.net_bytes.interfaces[1].1 += 9;
        state.baselines.net_bytes.count = 2;
        state.baselines.prev_page_faults += 1;
        state.baselines.prev_timestamp_ns += 10;
        state.archive.meta.uptime_secs += 1;
        state.archive.capabilities |= CAP_META_UPTIME;
        if self.failure == Failure::Collector {
            return ProviderOutcome::Fatal(AuraError::Fatal("collector fault".to_string()));
        }
        if self.unavailable {
            ProviderOutcome::Unavailable
        } else {
            ProviderOutcome::Available(())
        }
    }
}

pub struct ScriptedFinalizer {
    pub calls: usize,
    pub failure: Failure,
}

impl Finalizer for ScriptedFinalizer {
    fn finalize(&mut self, state: &mut FixedCollectorState) -> AuraResult<()> {
        self.calls += 1;
        state.archive.version = aura_common::ARCHIVE_VERSION;
        state.archive.meta.timestamp_ns += 100;
        if self.failure == Failure::Finalizer {
            return Err(AuraError::Fatal("finalizer fault".to_string()));
        }
        state.archive.checksum = state.archive.calculate_checksum();
        Ok(())
    }
}

pub struct MmapPublisher {
    pub calls: usize,
    failure: Failure,
    handle: ShmHandle,
    path: PathBuf,
    _directory: TempDir,
}

impl MmapPublisher {
    fn new(initial: &TelemetryArchive, failure: Failure) -> Self {
        let directory = tempfile::tempdir().expect("temporary SHM directory");
        std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700))
            .expect("private SHM directory");
        let path = directory.path().join("state.dat");
        let mut handle = ShmHandle::new(&path).expect("create real SHM publisher");
        handle.write(initial).expect("publish initial archive");
        Self {
            calls: 0,
            failure,
            handle,
            path,
            _directory: directory,
        }
    }

    pub fn raw_bytes(&self) -> Vec<u8> {
        std::fs::read(&self.path).expect("read exact SHM bytes")
    }

    pub fn published(&self) -> TelemetryArchive {
        read_published(&self.path)
    }
}

impl Publisher for MmapPublisher {
    fn publish(&mut self, archive: &TelemetryArchive) -> AuraResult<()> {
        self.calls += 1;
        if self.failure == Failure::Publisher {
            return Err(AuraError::Fatal("publisher fault".to_string()));
        }
        self.handle.write(archive)
    }
}

pub struct RecordingNotifier {
    pub calls: usize,
    pub fail_on: Option<usize>,
    pub notifications: [Option<Notification>; 4],
}

impl Notifier for RecordingNotifier {
    fn notify(&mut self, notification: Notification) -> AuraResult<()> {
        self.calls += 1;
        self.notifications[self.calls - 1] = Some(notification);
        if self.fail_on == Some(self.calls) {
            return Err(AuraError::Fatal("notifier fault".to_string()));
        }
        Ok(())
    }
}

#[derive(Default)]
pub struct RecordingSleeper {
    pub calls: usize,
    pub durations: [Duration; 4],
}

impl Sleeper for RecordingSleeper {
    fn sleep(&mut self, duration: Duration) {
        self.durations[self.calls] = duration;
        self.calls += 1;
    }
}

pub struct SteppingClock {
    next_monotonic_ns: u64,
}

impl Clock for SteppingClock {
    fn sample(&mut self) -> AuraResult<ClockSample> {
        let monotonic_ns = self.next_monotonic_ns;
        self.next_monotonic_ns += 100;
        Ok(ClockSample {
            monotonic_ns,
            wallclock_ns: monotonic_ns + 1_000,
        })
    }
}

pub type TestLifecycle = Lifecycle<
    ScriptedCollector,
    ScriptedFinalizer,
    MmapPublisher,
    RecordingNotifier,
    RecordingSleeper,
>;

pub type AllocationLifecycle = Lifecycle<
    SystemCollector<DeterministicSources>,
    SystemFinalizer<SteppingClock>,
    MmapPublisher,
    RecordingNotifier,
    RecordingSleeper,
>;

pub fn lifecycle(failure: Failure, notify_failure: Option<usize>) -> TestLifecycle {
    let committed = initial_committed();
    let state = CollectorState::with_committed(committed);
    let parts = LifecycleParts {
        collector: scripted_collector(failure),
        finalizer: ScriptedFinalizer { calls: 0, failure },
        publisher: MmapPublisher::new(&committed.archive, failure),
        notifier: RecordingNotifier {
            calls: 0,
            fail_on: notify_failure,
            notifications: [None; 4],
        },
        sleeper: RecordingSleeper::default(),
    };
    Lifecycle::new(state, parts)
}

pub fn allocation_lifecycle() -> AllocationLifecycle {
    let committed = initial_committed();
    let state = CollectorState::with_committed(committed);
    let parts = LifecycleParts {
        collector: SystemCollector::with_sources(DeterministicSources::default()),
        finalizer: SystemFinalizer::new(SteppingClock {
            next_monotonic_ns: 1_000,
        }),
        publisher: MmapPublisher::new(&committed.archive, Failure::None),
        notifier: RecordingNotifier {
            calls: 0,
            fail_on: None,
            notifications: [None; 4],
        },
        sleeper: RecordingSleeper::default(),
    };
    Lifecycle::new(state, parts)
}

pub fn assert_fixed_state_eq(actual: &FixedCollectorState, expected: &FixedCollectorState) {
    assert_archive_eq(&actual.archive, &expected.archive);
    let actual = &actual.baselines;
    let expected = &expected.baselines;
    assert_eq!(actual.cpu_ticks.user, expected.cpu_ticks.user);
    assert_eq!(actual.cpu_ticks.system, expected.cpu_ticks.system);
    assert_eq!(actual.cpu_ticks.idle, expected.cpu_ticks.idle);
    assert_eq!(actual.cpu_ticks.total, expected.cpu_ticks.total);
    assert_eq!(
        actual.cpu_ticks.context_switches,
        expected.cpu_ticks.context_switches
    );
    assert_eq!(actual.net_bytes.interfaces, expected.net_bytes.interfaces);
    assert_eq!(actual.net_bytes.count, expected.net_bytes.count);
    assert_eq!(actual.prev_page_faults, expected.prev_page_faults);
    assert_eq!(actual.prev_timestamp_ns, expected.prev_timestamp_ns);
}

pub fn assert_archive_eq(actual: &TelemetryArchive, expected: &TelemetryArchive) {
    assert_eq!(archive_bytes(actual), archive_bytes(expected));
}

pub fn assert_baselines_advanced_once(
    actual: &FixedCollectorState,
    previous: &FixedCollectorState,
) {
    let actual = &actual.baselines;
    let previous = &previous.baselines;
    assert_eq!(actual.cpu_ticks.user, previous.cpu_ticks.user + 1);
    assert_eq!(actual.cpu_ticks.system, previous.cpu_ticks.system + 2);
    assert_eq!(actual.cpu_ticks.idle, previous.cpu_ticks.idle + 3);
    assert_eq!(actual.cpu_ticks.total, previous.cpu_ticks.total + 4);
    assert_eq!(
        actual.cpu_ticks.context_switches,
        previous.cpu_ticks.context_switches + 5
    );
    let mut expected_interfaces = previous.net_bytes.interfaces;
    expected_interfaces[0].0 += 6;
    expected_interfaces[0].1 += 7;
    expected_interfaces[1].0 += 8;
    expected_interfaces[1].1 += 9;
    assert_eq!(actual.net_bytes.interfaces, expected_interfaces);
    assert_eq!(actual.net_bytes.count, 2);
    assert_eq!(actual.prev_page_faults, previous.prev_page_faults + 1);
    assert_eq!(actual.prev_timestamp_ns, previous.prev_timestamp_ns + 10);
}

fn initial_committed() -> FixedCollectorState {
    let mut committed = FixedCollectorState::default();
    committed.baselines.cpu_ticks.user = 1;
    committed.baselines.cpu_ticks.system = 2;
    committed.baselines.cpu_ticks.idle = 3;
    committed.baselines.cpu_ticks.total = 4;
    committed.baselines.cpu_ticks.context_switches = 5;
    committed.baselines.net_bytes.interfaces[0] = (6, 7);
    committed.baselines.net_bytes.interfaces[1] = (8, 9);
    for index in 2..MAX_NETIFS {
        committed.baselines.net_bytes.interfaces[index] =
            (index as u64 * 10, index as u64 * 10 + 1);
    }
    committed.baselines.net_bytes.count = 2;
    committed.baselines.prev_page_faults = 10;
    committed.baselines.prev_timestamp_ns = 100;
    committed.archive.meta.uptime_secs = 10;
    committed.archive.capabilities = CAP_META_UPTIME;
    committed.archive.checksum = committed.archive.calculate_checksum();
    committed
}

fn scripted_collector(failure: Failure) -> ScriptedCollector {
    ScriptedCollector {
        calls: 0,
        failure,
        unavailable: false,
        seen_baselines: [0; 4],
    }
}

fn archive_bytes(archive: &TelemetryArchive) -> &[u8] {
    // SAFETY: [Categories 4 and 10 — initialization and bounds]
    // `TelemetryArchive` is fully initialized and `size_of` describes its
    // complete contiguous object representation for this shared borrow.
    unsafe {
        std::slice::from_raw_parts(
            std::ptr::addr_of!(*archive).cast::<u8>(),
            std::mem::size_of::<TelemetryArchive>(),
        )
    }
}

fn read_published(path: &Path) -> TelemetryArchive {
    let file = File::open(path).expect("open real SHM mapping");
    // SAFETY: [Categories 6 and 10 — alignment and bounds] `ShmHandle`
    // created this regular file at exactly `SHM_SIZE`, and `Mmap` owns the
    // naturally page-aligned read-only mapping for the returned lifetime.
    let mmap: Mmap = unsafe {
        MmapOptions::new()
            .len(SHM_SIZE)
            .map(&file)
            .expect("map real SHM file")
    };
    // SAFETY: [Categories 2, 6, and 10 — races, alignment, and bounds]
    // the live exact-sized mapping was initialized by the sole `ShmHandle`
    // writer and this reader uses only the atomic double-buffer protocol.
    unsafe { read_double_buffer(mmap.as_ptr()) }.expect("read published archive")
}
