use std::fs::File;
#[cfg(target_os = "linux")]
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::Duration;

use aura_common::{
    read_double_buffer, AuraError, AuraResult, TelemetryArchive, CAP_META_UPTIME, SHM_SIZE,
};
#[cfg(target_os = "linux")]
use aura_daemon::collectors::process::linux::{
    collect_with_directory, ProcessDirectory, ProcessScan, DIRENT_BUF_LEN,
};
#[cfg(target_os = "linux")]
use aura_daemon::collectors::process::ProcessBaseSnapshot;
use aura_daemon::collectors::{
    CollectorScratch, CollectorState, CycleCollector, FixedCollectorState, NetIfKey,
    ProviderOutcome, SystemCollector,
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
        let key_eth = NetIfKey::from_linux_name(b"eth0");
        let key_wlan = NetIfKey::from_linux_name(b"wlan0");
        if let Some(slot) = state.baselines.net_bytes.get_mut(&key_eth) {
            slot.rx_bytes += 6;
            slot.tx_bytes += 7;
        }
        if let Some(slot) = state.baselines.net_bytes.get_mut(&key_wlan) {
            slot.rx_bytes += 8;
            slot.tx_bytes += 9;
        }
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

#[cfg(target_os = "linux")]
pub type MidScanFailureLifecycle = Lifecycle<
    MidScanFatalCollector,
    ScriptedFinalizer,
    MmapPublisher,
    RecordingNotifier,
    RecordingSleeper,
>;

#[cfg(target_os = "linux")]
pub struct MidScanFatalCollector {
    process_root: TempDir,
}

#[cfg(target_os = "linux")]
#[derive(Default)]
struct OnePidThenFatal {
    reads: u8,
}

#[cfg(target_os = "linux")]
impl ProcessDirectory for OnePidThenFatal {
    fn read(&mut self, buf: &mut [u8; DIRENT_BUF_LEN]) -> AuraResult<usize> {
        if self.reads != 0 {
            return Err(AuraError::Fatal("injected getdents64 failure".to_string()));
        }
        self.reads = 1;
        let record_len = 21u16;
        let bytes = record_len.to_ne_bytes();
        buf[16] = bytes[0];
        buf[17] = bytes[1];
        buf[18] = libc::DT_DIR;
        buf[19] = b'7';
        buf[20] = 0;
        Ok(usize::from(record_len))
    }
}

#[cfg(target_os = "linux")]
impl CycleCollector for MidScanFatalCollector {
    fn collect(
        &mut self,
        state: &mut FixedCollectorState,
        scratch: &mut CollectorScratch,
    ) -> ProviderOutcome<()> {
        state.baselines.process_page_size = 8192;
        let mut reader = OnePidThenFatal::default();
        let mut scan = ProcessScan {
            proc_root: self.process_root.path().as_os_str().as_bytes(),
            page_size: state.baselines.process_page_size,
            online_cores: 1,
            delta_global_ticks: 100,
            stat_buf: &mut scratch.proc_buffer,
            path_buf: &mut scratch.process_path_buffer,
        };
        match collect_with_directory(
            &mut scan,
            &mut state.baselines.process,
            &mut state.archive.process,
            &mut reader,
        ) {
            Ok(()) => ProviderOutcome::Available(()),
            Err(error) => ProviderOutcome::Fatal(error),
        }
    }
}

pub fn lifecycle(failure: Failure, notify_failure: Option<usize>) -> TestLifecycle {
    let committed = initial_committed();
    let publisher = MmapPublisher::new(&committed.archive, failure);
    let state = CollectorState::with_committed(committed);
    let parts = LifecycleParts {
        collector: scripted_collector(failure),
        finalizer: ScriptedFinalizer { calls: 0, failure },
        publisher,
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
    let publisher = MmapPublisher::new(&committed.archive, Failure::None);
    let state = CollectorState::with_committed(committed);
    let parts = LifecycleParts {
        collector: SystemCollector::with_sources(DeterministicSources::default()),
        finalizer: SystemFinalizer::new(SteppingClock {
            next_monotonic_ns: 1_000,
        }),
        publisher,
        notifier: RecordingNotifier {
            calls: 0,
            fail_on: None,
            notifications: [None; 4],
        },
        sleeper: RecordingSleeper::default(),
    };
    Lifecycle::new(state, parts)
}

#[cfg(target_os = "linux")]
pub fn mid_scan_failure_lifecycle() -> MidScanFailureLifecycle {
    let process_root = tempfile::tempdir().expect("temporary proc root");
    let pid_root = process_root.path().join("7");
    std::fs::create_dir(&pid_root).expect("pid directory");
    std::fs::write(
        pid_root.join("stat"),
        b"7 (partial) R 1 7 7 0 -1 4194304 100 0 0 0 1 1 0 0 20 0 1 0 7 200 1 1 1 0 0 0 0 0 0 0 0 0 17 0 0 0 0 0 0 0 0 0",
    )
    .expect("pid stat");

    let mut committed = initial_committed();
    committed.baselines.process_page_size = 4096;
    committed
        .baselines
        .process
        .insert((99, 99), ProcessBaseSnapshot { utime: 3, stime: 4 });
    let publisher = MmapPublisher::new(&committed.archive, Failure::None);
    let state = CollectorState::with_committed(committed);
    let parts = LifecycleParts {
        collector: MidScanFatalCollector { process_root },
        finalizer: ScriptedFinalizer {
            calls: 0,
            failure: Failure::None,
        },
        publisher,
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
    assert_eq!(actual.cpu_over_capacity, expected.cpu_over_capacity);
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
    assert_eq!(actual.net_bytes.slots, expected.net_bytes.slots);
    assert_eq!(actual.net_bytes.represented, expected.net_bytes.represented);
    assert_eq!(actual.disk.slots, expected.disk.slots);
    assert_eq!(actual.disk.generation, expected.disk.generation);
    assert_eq!(actual.core_count, expected.core_count);
    for (actual_core, expected_core) in actual.cores.iter().zip(expected.cores.iter()) {
        assert_eq!(actual_core.user, expected_core.user);
        assert_eq!(actual_core.system, expected_core.system);
        assert_eq!(actual_core.idle, expected_core.idle);
        assert_eq!(actual_core.total, expected_core.total);
    }
    assert_eq!(actual.process_page_size, expected.process_page_size);
    assert_eq!(
        actual.process.current_generation,
        expected.process.current_generation
    );
    for (actual_slot, expected_slot) in actual
        .process
        .slots
        .iter()
        .zip(expected.process.slots.iter())
    {
        assert_eq!(actual_slot.pid, expected_slot.pid);
        assert_eq!(actual_slot.generation, expected_slot.generation);
        assert_eq!(actual_slot.starttime, expected_slot.starttime);
        assert_eq!(actual_slot.stat.utime, expected_slot.stat.utime);
        assert_eq!(actual_slot.stat.stime, expected_slot.stat.stime);
    }
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
    let key_eth = NetIfKey::from_linux_name(b"eth0");
    let key_wlan = NetIfKey::from_linux_name(b"wlan0");
    let prev_eth = previous
        .net_bytes
        .get(&key_eth)
        .copied()
        .expect("eth0 baseline seeded");
    let prev_wlan = previous
        .net_bytes
        .get(&key_wlan)
        .copied()
        .expect("wlan0 baseline seeded");
    let actual_eth = actual
        .net_bytes
        .get(&key_eth)
        .copied()
        .expect("eth0 baseline advanced");
    let actual_wlan = actual
        .net_bytes
        .get(&key_wlan)
        .copied()
        .expect("wlan0 baseline advanced");
    assert_eq!(actual_eth.rx_bytes, prev_eth.rx_bytes + 6);
    assert_eq!(actual_eth.tx_bytes, prev_eth.tx_bytes + 7);
    assert_eq!(actual_wlan.rx_bytes, prev_wlan.rx_bytes + 8);
    assert_eq!(actual_wlan.tx_bytes, prev_wlan.tx_bytes + 9);
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
    let key_eth = NetIfKey::from_linux_name(b"eth0");
    let key_wlan = NetIfKey::from_linux_name(b"wlan0");
    let key_loop = NetIfKey::from_linux_name(b"lo");
    committed.baselines.net_bytes.insert(key_eth, 1, 6, 7);
    committed.baselines.net_bytes.insert(key_wlan, 1, 8, 9);
    committed.baselines.net_bytes.insert(key_loop, 1, 10, 11);
    committed.baselines.net_bytes.represented = 2;
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
