use std::cell::Cell;
use std::ffi::{OsStr, OsString};
use std::rc::Rc;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use aura_common::{AuraError, AuraResult, TelemetryArchive};
use aura_daemon::collectors::{
    CollectorScratch, CollectorState, CycleCollector, FixedCollectorState, ProviderOutcome,
};
use aura_daemon::lifecycle::{
    Finalizer, Heartbeat, Lifecycle, LifecycleParts, Notification, Notifier, Publisher, Sleeper,
};
use aura_daemon::notify::{NotifierMode, NotifyEnvironment, SystemdNotifier};

#[cfg(target_os = "linux")]
use std::os::linux::net::SocketAddrExt;
#[cfg(target_os = "linux")]
use std::os::unix::ffi::OsStringExt;
#[cfg(target_os = "linux")]
use std::os::unix::net::{SocketAddr, UnixDatagram};
#[cfg(target_os = "linux")]
use std::path::PathBuf;

const CURRENT_PID: u32 = 4242;

fn heartbeat(milliseconds: u64) -> Heartbeat {
    Heartbeat::from_millis(milliseconds).expect("positive heartbeat")
}

fn environment<'a>(
    socket: Option<&'a OsStr>,
    watchdog_pid: Option<&'a OsStr>,
    watchdog_usec: Option<&'a OsStr>,
) -> NotifyEnvironment<'a> {
    NotifyEnvironment {
        notify_socket: socket,
        watchdog_pid,
        watchdog_usec,
    }
}

#[cfg(target_os = "linux")]
struct FilesystemReceiver {
    _directory: tempfile::TempDir,
    path: PathBuf,
    socket: UnixDatagram,
}

#[cfg(target_os = "linux")]
impl FilesystemReceiver {
    fn new() -> Self {
        let directory = tempfile::tempdir().expect("notification directory");
        let path = directory.path().join("notify.sock");
        let socket = UnixDatagram::bind(&path).expect("bind notification socket");
        socket
            .set_read_timeout(Some(Duration::from_secs(1)))
            .expect("set notification timeout");
        Self {
            _directory: directory,
            path,
            socket,
        }
    }

    fn receive(&self) -> Vec<u8> {
        let mut message = [0_u8; 32];
        let length = self
            .socket
            .recv(&mut message)
            .expect("receive notification");
        message[..length].to_vec()
    }

    fn assert_empty(&self) {
        self.socket
            .set_nonblocking(true)
            .expect("set nonblocking receiver");
        let mut message = [0_u8; 32];
        let error = self
            .socket
            .recv(&mut message)
            .expect_err("notification socket must be empty");
        assert_eq!(error.kind(), std::io::ErrorKind::WouldBlock);
        self.socket
            .set_nonblocking(false)
            .expect("restore blocking receiver");
    }
}

#[test]
#[cfg(target_os = "linux")]
fn notifier_is_disabled_when_notify_socket_is_absent() {
    // Given: watchdog variables without a systemd notification socket.
    let env = environment(None, Some(OsStr::new("bad")), Some(OsStr::new("bad")));

    // When: notification is negotiated.
    let notifier = SystemdNotifier::negotiate(env, heartbeat(500), CURRENT_PID)
        .expect("absent socket disables notification");

    // Then: malformed watchdog-only residue is ignored because notification is disabled.
    assert_eq!(notifier.mode(), NotifierMode::Disabled);
}

#[test]
#[cfg(target_os = "linux")]
fn malformed_watchdog_pid_is_fatal() {
    let env = environment(
        Some(OsStr::new("@aura-pid")),
        Some(OsStr::new("nope")),
        Some(OsStr::new("3000000")),
    );
    let result = SystemdNotifier::negotiate(env, heartbeat(500), CURRENT_PID);
    assert!(matches!(result, Err(AuraError::Fatal(message)) if message.contains("WATCHDOG_PID")));
}

#[test]
#[cfg(target_os = "linux")]
fn malformed_watchdog_usec_is_fatal() {
    let env = environment(Some(OsStr::new("@aura-usec")), None, Some(OsStr::new("-1")));
    let result = SystemdNotifier::negotiate(env, heartbeat(500), CURRENT_PID);
    assert!(matches!(result, Err(AuraError::Fatal(message)) if message.contains("WATCHDOG_USEC")));
}

#[test]
#[cfg(target_os = "linux")]
fn malformed_usec_is_fatal_even_when_numeric_pid_mismatches() {
    let env = environment(
        Some(OsStr::new("@aura-mismatch-malformed")),
        Some(OsStr::new("7")),
        Some(OsStr::new("invalid")),
    );
    let result = SystemdNotifier::negotiate(env, heartbeat(500), CURRENT_PID);
    assert!(matches!(result, Err(AuraError::Fatal(message)) if message.contains("WATCHDOG_USEC")));
}

#[test]
#[cfg(target_os = "linux")]
fn numeric_pid_mismatch_negotiates_ready_only() {
    let env = environment(
        Some(OsStr::new("@aura-other-pid")),
        Some(OsStr::new("7")),
        Some(OsStr::new("3000000")),
    );
    let notifier = SystemdNotifier::negotiate(env, heartbeat(500), CURRENT_PID)
        .expect("PID mismatch is not fatal");
    assert_eq!(notifier.mode(), NotifierMode::ReadyOnly);
}

#[test]
#[cfg(target_os = "linux")]
fn zero_pid_mismatch_negotiates_ready_only() {
    // Given: a numeric PID of zero that does not match the current process.
    let env = environment(
        Some(OsStr::new("@aura-zero-pid")),
        Some(OsStr::new("0")),
        Some(OsStr::new("3000000")),
    );

    // When: watchdog notification is negotiated.
    let notifier = SystemdNotifier::negotiate(env, heartbeat(500), CURRENT_PID)
        .expect("zero PID is a numeric mismatch");

    // Then: the notifier remains READY-only.
    assert_eq!(notifier.mode(), NotifierMode::ReadyOnly);
}

#[test]
#[cfg(target_os = "linux")]
fn leading_zero_pid_matches_its_numeric_value() {
    // Given: a watchdog PID with leading zeros whose numeric value is current.
    let env = environment(
        Some(OsStr::new("@aura-leading-zero-pid")),
        Some(OsStr::new("0042")),
        Some(OsStr::new("3000000")),
    );

    // When: notification is negotiated for PID 42.
    let notifier =
        SystemdNotifier::negotiate(env, heartbeat(500), 42).expect("leading zeros are valid");

    // Then: the numeric match enables watchdog mode.
    assert!(matches!(notifier.mode(), NotifierMode::Watchdog { .. }));
}

#[test]
#[cfg(target_os = "linux")]
fn pid_above_u32_max_is_a_ready_only_mismatch() {
    // Given: a valid u64 PID one greater than u32::MAX.
    let env = environment(
        Some(OsStr::new("@aura-u32-overflow-pid")),
        Some(OsStr::new("4294967296")),
        Some(OsStr::new("3000000")),
    );

    // When: notification is negotiated for the current u32 process PID.
    let notifier = SystemdNotifier::negotiate(env, heartbeat(500), CURRENT_PID)
        .expect("valid u64 PID is not malformed");

    // Then: the numeric mismatch is READY-only rather than Fatal.
    assert_eq!(notifier.mode(), NotifierMode::ReadyOnly);
}

#[test]
#[cfg(target_os = "linux")]
fn u64_max_pid_is_a_ready_only_mismatch() {
    // Given: the largest successfully parseable WATCHDOG_PID.
    let env = environment(
        Some(OsStr::new("@aura-u64-max-pid")),
        Some(OsStr::new("18446744073709551615")),
        Some(OsStr::new("3000000")),
    );

    // When: notification is negotiated.
    let notifier = SystemdNotifier::negotiate(env, heartbeat(500), CURRENT_PID)
        .expect("u64::MAX is a valid numeric PID");

    // Then: the numeric mismatch is READY-only.
    assert_eq!(notifier.mode(), NotifierMode::ReadyOnly);
}

#[test]
#[cfg(target_os = "linux")]
fn pid_above_u64_max_is_fatal() {
    // Given: a decimal WATCHDOG_PID that exceeds u64::MAX.
    let env = environment(
        Some(OsStr::new("@aura-u64-overflow-pid")),
        Some(OsStr::new("18446744073709551616")),
        Some(OsStr::new("3000000")),
    );

    // When: notification is negotiated.
    let result = SystemdNotifier::negotiate(env, heartbeat(500), CURRENT_PID);

    // Then: decimal overflow is malformed and therefore Fatal.
    assert!(matches!(result, Err(AuraError::Fatal(message)) if message.contains("WATCHDOG_PID")));
}

#[test]
#[cfg(target_os = "linux")]
fn unset_pid_with_missing_usec_negotiates_ready_only() {
    let env = environment(Some(OsStr::new("@aura-missing-usec")), None, None);
    let notifier = SystemdNotifier::negotiate(env, heartbeat(500), CURRENT_PID)
        .expect("missing interval is ready-only");
    assert_eq!(notifier.mode(), NotifierMode::ReadyOnly);
}

#[test]
#[cfg(target_os = "linux")]
fn matching_pid_with_zero_usec_negotiates_ready_only() {
    let pid = CURRENT_PID.to_string();
    let env = environment(
        Some(OsStr::new("@aura-zero-usec")),
        Some(OsStr::new(&pid)),
        Some(OsStr::new("0")),
    );
    let notifier = SystemdNotifier::negotiate(env, heartbeat(500), CURRENT_PID)
        .expect("zero interval is ready-only");
    assert_eq!(notifier.mode(), NotifierMode::ReadyOnly);
}

#[test]
#[cfg(target_os = "linux")]
fn unset_pid_with_positive_usec_enables_watchdog() {
    let env = environment(
        Some(OsStr::new("@aura-unset-pid")),
        None,
        Some(OsStr::new("3000000")),
    );
    let notifier = SystemdNotifier::negotiate(env, heartbeat(500), CURRENT_PID)
        .expect("unset PID applies to current process");
    assert_eq!(
        notifier.mode(),
        NotifierMode::Watchdog {
            half_interval: Duration::from_micros(1_500_000),
        }
    );
}

#[test]
#[cfg(target_os = "linux")]
fn matching_pid_with_positive_usec_enables_watchdog() {
    let pid = CURRENT_PID.to_string();
    let env = environment(
        Some(OsStr::new("@aura-matching-pid")),
        Some(OsStr::new(&pid)),
        Some(OsStr::new("3000000")),
    );
    let notifier = SystemdNotifier::negotiate(env, heartbeat(500), CURRENT_PID)
        .expect("matching PID enables watchdog");
    assert!(matches!(notifier.mode(), NotifierMode::Watchdog { .. }));
}

#[test]
#[cfg(target_os = "linux")]
fn heartbeat_at_half_interval_boundary_is_accepted() {
    let env = environment(
        Some(OsStr::new("@aura-boundary")),
        None,
        Some(OsStr::new("3000000")),
    );
    let notifier = SystemdNotifier::negotiate(env, heartbeat(1500), CURRENT_PID)
        .expect("heartbeat equals half interval");
    assert!(matches!(notifier.mode(), NotifierMode::Watchdog { .. }));
}

#[test]
#[cfg(target_os = "linux")]
fn heartbeat_above_half_interval_boundary_is_fatal() {
    let env = environment(
        Some(OsStr::new("@aura-slow-heartbeat")),
        None,
        Some(OsStr::new("3000000")),
    );
    let result = SystemdNotifier::negotiate(env, heartbeat(1501), CURRENT_PID);
    assert!(matches!(result, Err(AuraError::Fatal(message)) if message.contains("heartbeat")));
}

#[test]
#[cfg(target_os = "linux")]
fn filesystem_socket_receives_exact_ready_message() {
    let receiver = FilesystemReceiver::new();
    let env = environment(Some(receiver.path.as_os_str()), None, None);
    let mut notifier =
        SystemdNotifier::negotiate(env, heartbeat(500), CURRENT_PID).expect("notifier");

    notifier
        .notify_at(Notification::Ready, Duration::ZERO)
        .expect("send READY");

    assert_eq!(receiver.receive(), b"READY=1\n");
}

#[test]
#[cfg(target_os = "linux")]
fn abstract_socket_receives_exact_ready_message() {
    let name = format!("aura-notify-{}", std::process::id());
    let address = SocketAddr::from_abstract_name(name.as_bytes()).expect("abstract address");
    let receiver = UnixDatagram::bind_addr(&address).expect("bind abstract socket");
    receiver
        .set_read_timeout(Some(Duration::from_secs(1)))
        .expect("set timeout");
    let notify_socket = OsString::from(format!("@{name}"));
    let env = environment(Some(notify_socket.as_os_str()), None, None);
    let mut notifier =
        SystemdNotifier::negotiate(env, heartbeat(500), CURRENT_PID).expect("notifier");

    notifier
        .notify_at(Notification::Ready, Duration::ZERO)
        .expect("send READY");

    let mut message = [0_u8; 32];
    let length = receiver.recv(&mut message).expect("receive READY");
    assert_eq!(&message[..length], b"READY=1\n");
}

#[test]
#[cfg(target_os = "linux")]
fn maximum_abstract_address_uses_no_trailing_nul() {
    let name = vec![b'x'; 107];
    let address = SocketAddr::from_abstract_name(&name).expect("maximum abstract address");
    let receiver = UnixDatagram::bind_addr(&address).expect("bind maximum abstract socket");
    receiver
        .set_read_timeout(Some(Duration::from_secs(1)))
        .expect("set timeout");
    let mut notify_socket = Vec::with_capacity(108);
    notify_socket.push(b'@');
    notify_socket.extend_from_slice(&name);
    let notify_socket = OsString::from_vec(notify_socket);
    let env = environment(Some(notify_socket.as_os_str()), None, None);
    let mut notifier =
        SystemdNotifier::negotiate(env, heartbeat(500), CURRENT_PID).expect("notifier");

    notifier
        .notify_at(Notification::Ready, Duration::ZERO)
        .expect("send maximum abstract address");

    let mut message = [0_u8; 32];
    let length = receiver.recv(&mut message).expect("receive READY");
    assert_eq!(&message[..length], b"READY=1\n");
}

#[test]
#[cfg(target_os = "linux")]
fn oversized_abstract_address_is_fatal() {
    let notify_socket = OsString::from(format!("@{}", "x".repeat(108)));
    let env = environment(Some(notify_socket.as_os_str()), None, None);
    let result = SystemdNotifier::negotiate(env, heartbeat(500), CURRENT_PID);
    assert!(matches!(result, Err(AuraError::Fatal(message)) if message.contains("NOTIFY_SOCKET")));
}

#[test]
#[cfg(target_os = "linux")]
fn maximum_filesystem_path_is_accepted() {
    let notify_socket = OsString::from(format!("/{}", "x".repeat(106)));
    let env = environment(Some(notify_socket.as_os_str()), None, None);
    let notifier = SystemdNotifier::negotiate(env, heartbeat(500), CURRENT_PID)
        .expect("107-byte pathname plus NUL fits sun_path");
    assert_eq!(notifier.mode(), NotifierMode::ReadyOnly);
}

#[test]
#[cfg(target_os = "linux")]
fn oversized_filesystem_path_is_fatal() {
    let notify_socket = OsString::from(format!("/{}", "x".repeat(107)));
    let env = environment(Some(notify_socket.as_os_str()), None, None);
    let result = SystemdNotifier::negotiate(env, heartbeat(500), CURRENT_PID);
    assert!(matches!(result, Err(AuraError::Fatal(message)) if message.contains("NOTIFY_SOCKET")));
}

#[test]
#[cfg(target_os = "linux")]
fn relative_notify_socket_is_fatal() {
    let env = environment(Some(OsStr::new("relative.sock")), None, None);
    let result = SystemdNotifier::negotiate(env, heartbeat(500), CURRENT_PID);
    assert!(matches!(result, Err(AuraError::Fatal(message)) if message.contains("NOTIFY_SOCKET")));
}

#[test]
#[cfg(target_os = "linux")]
fn filesystem_notify_socket_with_embedded_nul_is_fatal() {
    // Given: an absolute filesystem socket containing an embedded NUL.
    let notify_socket = OsString::from_vec(b"/tmp/foo\0bar".to_vec());
    let env = environment(Some(notify_socket.as_os_str()), None, None);

    // When: notification is negotiated.
    let result = SystemdNotifier::negotiate(env, heartbeat(500), CURRENT_PID);

    // Then: the malformed address is rejected.
    assert!(matches!(result, Err(AuraError::Fatal(message)) if message.contains("embedded NUL")));
}

#[test]
#[cfg(target_os = "linux")]
fn abstract_notify_socket_with_embedded_nul_is_fatal() {
    // Given: an abstract socket name containing an embedded NUL.
    let notify_socket = OsString::from_vec(b"@foo\0bar".to_vec());
    let env = environment(Some(notify_socket.as_os_str()), None, None);

    // When: notification is negotiated.
    let result = SystemdNotifier::negotiate(env, heartbeat(500), CURRENT_PID);

    // Then: the malformed address is rejected.
    assert!(matches!(result, Err(AuraError::Fatal(message)) if message.contains("embedded NUL")));
}

#[test]
#[cfg(target_os = "linux")]
fn ready_only_mode_never_sends_watchdog() {
    let receiver = FilesystemReceiver::new();
    let env = environment(Some(receiver.path.as_os_str()), None, None);
    let mut notifier =
        SystemdNotifier::negotiate(env, heartbeat(500), CURRENT_PID).expect("notifier");

    notifier
        .notify_at(Notification::Ready, Duration::ZERO)
        .expect("send READY");
    assert_eq!(receiver.receive(), b"READY=1\n");
    notifier
        .notify_at(Notification::Watchdog, Duration::from_secs(30))
        .expect("ignore disabled watchdog");

    receiver.assert_empty();
}

#[test]
#[cfg(target_os = "linux")]
fn watchdog_deadline_starts_after_ready() {
    let receiver = FilesystemReceiver::new();
    let env = environment(
        Some(receiver.path.as_os_str()),
        None,
        Some(OsStr::new("2000000")),
    );
    let mut notifier =
        SystemdNotifier::negotiate(env, heartbeat(500), CURRENT_PID).expect("notifier");

    notifier
        .notify_at(Notification::Watchdog, Duration::from_secs(9))
        .expect("ignore pre-READY watchdog");
    receiver.assert_empty();
    notifier
        .notify_at(Notification::Ready, Duration::from_secs(10))
        .expect("send READY");
    assert_eq!(receiver.receive(), b"READY=1\n");
    notifier
        .notify_at(Notification::Watchdog, Duration::from_millis(10_999))
        .expect("not due");
    receiver.assert_empty();
    notifier
        .notify_at(Notification::Watchdog, Duration::from_secs(11))
        .expect("due watchdog");

    assert_eq!(receiver.receive(), b"WATCHDOG=1\n");
}

#[test]
#[cfg(target_os = "linux")]
fn successful_watchdog_resets_the_half_interval_deadline() {
    let receiver = FilesystemReceiver::new();
    let env = environment(
        Some(receiver.path.as_os_str()),
        None,
        Some(OsStr::new("2000000")),
    );
    let mut notifier =
        SystemdNotifier::negotiate(env, heartbeat(500), CURRENT_PID).expect("notifier");
    notifier
        .notify_at(Notification::Ready, Duration::ZERO)
        .expect("send READY");
    receiver.receive();
    notifier
        .notify_at(Notification::Watchdog, Duration::from_secs(1))
        .expect("first watchdog");
    assert_eq!(receiver.receive(), b"WATCHDOG=1\n");

    notifier
        .notify_at(Notification::Watchdog, Duration::from_millis(1999))
        .expect("second watchdog not due");
    receiver.assert_empty();
    notifier
        .notify_at(Notification::Watchdog, Duration::from_secs(2))
        .expect("second watchdog due");

    assert_eq!(receiver.receive(), b"WATCHDOG=1\n");
}

#[test]
#[cfg(target_os = "linux")]
fn due_watchdog_send_failure_is_fatal() {
    // Given: watchdog mode whose receiver disappears after accepting READY.
    let receiver = FilesystemReceiver::new();
    let env = environment(
        Some(receiver.path.as_os_str()),
        None,
        Some(OsStr::new("2000000")),
    );
    let mut notifier =
        SystemdNotifier::negotiate(env, heartbeat(500), CURRENT_PID).expect("notifier");
    notifier
        .notify_at(Notification::Ready, Duration::ZERO)
        .expect("send READY");
    assert_eq!(receiver.receive(), b"READY=1\n");
    std::fs::remove_file(&receiver.path).expect("remove receiver pathname");

    // When: a due watchdog notification is sent to the missing pathname.
    let result = notifier.notify_at(Notification::Watchdog, Duration::from_secs(1));

    // Then: the send failure is Fatal and identifies WATCHDOG.
    assert!(matches!(result, Err(AuraError::Fatal(message)) if message.contains("WATCHDOG=1")));
}

#[test]
#[cfg(target_os = "linux")]
fn ready_deadline_overflow_is_fatal() {
    // Given: watchdog mode and a monotonic time less than one interval below Duration::MAX.
    let receiver = FilesystemReceiver::new();
    let env = environment(
        Some(receiver.path.as_os_str()),
        None,
        Some(OsStr::new("2000000")),
    );
    let mut notifier =
        SystemdNotifier::negotiate(env, heartbeat(500), CURRENT_PID).expect("notifier");
    let near_maximum = Duration::MAX - Duration::from_millis(500);

    // When: READY establishes its first watchdog deadline.
    let result = notifier.notify_at(Notification::Ready, near_maximum);

    // Then: checked deadline overflow is Fatal.
    assert!(
        matches!(result, Err(AuraError::Fatal(message)) if message.contains("deadline overflowed"))
    );
}

#[test]
#[cfg(target_os = "linux")]
fn stopping_uses_exact_protocol_message() {
    let receiver = FilesystemReceiver::new();
    let env = environment(Some(receiver.path.as_os_str()), None, None);
    let mut notifier =
        SystemdNotifier::negotiate(env, heartbeat(500), CURRENT_PID).expect("notifier");

    notifier
        .notify_at(Notification::Stopping, Duration::ZERO)
        .expect("send STOPPING");

    assert_eq!(receiver.receive(), b"STOPPING=1\n");
}

#[test]
#[cfg(target_os = "linux")]
fn ready_send_failure_is_fatal() {
    let directory = tempfile::tempdir().expect("notification directory");
    let missing = directory.path().join("missing.sock");
    let env = environment(Some(missing.as_os_str()), None, None);
    let mut notifier =
        SystemdNotifier::negotiate(env, heartbeat(500), CURRENT_PID).expect("notifier");

    let result = notifier.notify_at(Notification::Ready, Duration::ZERO);

    assert!(matches!(result, Err(AuraError::Fatal(message)) if message.contains("READY=1")));
}

#[derive(Clone, Copy)]
enum CollectorBehavior {
    Succeed,
    Fail,
}

struct TestCollector(CollectorBehavior);

impl CycleCollector for TestCollector {
    fn collect(
        &mut self,
        state: &mut FixedCollectorState,
        _scratch: &mut CollectorScratch,
    ) -> ProviderOutcome<()> {
        match self.0 {
            CollectorBehavior::Succeed => {
                state.archive.meta.uptime_secs += 1;
                ProviderOutcome::Available(())
            }
            CollectorBehavior::Fail => {
                ProviderOutcome::Fatal(AuraError::Fatal("collector failed".to_string()))
            }
        }
    }
}

struct TestFinalizer;

impl Finalizer for TestFinalizer {
    fn finalize(&mut self, state: &mut FixedCollectorState) -> AuraResult<()> {
        state.archive.checksum = state.archive.calculate_checksum();
        Ok(())
    }
}

struct FlagPublisher {
    published: Rc<Cell<bool>>,
    fail: bool,
}

impl Publisher for FlagPublisher {
    fn publish(&mut self, _archive: &TelemetryArchive) -> AuraResult<()> {
        if self.fail {
            return Err(AuraError::Fatal("publication failed".to_string()));
        }
        self.published.set(true);
        Ok(())
    }
}

struct OrderingNotifier {
    published: Rc<Cell<bool>>,
    calls: Rc<Cell<usize>>,
    fail_stopping: bool,
    notifications: Vec<Notification>,
}

impl Notifier for OrderingNotifier {
    fn notify(&mut self, notification: Notification) -> AuraResult<()> {
        if notification != Notification::Stopping {
            assert!(
                self.published.get(),
                "publication must precede notification"
            );
        }
        self.calls.set(self.calls.get() + 1);
        self.notifications.push(notification);
        if notification == Notification::Stopping && self.fail_stopping {
            return Err(AuraError::Fatal("STOPPING failed".to_string()));
        }
        Ok(())
    }
}

struct NoSleep;

impl Sleeper for NoSleep {
    fn sleep(&mut self, _duration: Duration) {}
}

type TestLifecycle =
    Lifecycle<TestCollector, TestFinalizer, FlagPublisher, OrderingNotifier, NoSleep>;
type LifecycleFixture = (TestLifecycle, Rc<Cell<bool>>, Rc<Cell<usize>>);

fn lifecycle(
    collector: CollectorBehavior,
    publication_fails: bool,
    stopping_fails: bool,
) -> LifecycleFixture {
    let published = Rc::new(Cell::new(false));
    let calls = Rc::new(Cell::new(0));
    let parts = LifecycleParts {
        collector: TestCollector(collector),
        finalizer: TestFinalizer,
        publisher: FlagPublisher {
            published: Rc::clone(&published),
            fail: publication_fails,
        },
        notifier: OrderingNotifier {
            published: Rc::clone(&published),
            calls: Rc::clone(&calls),
            fail_stopping: stopping_fails,
            notifications: Vec::new(),
        },
        sleeper: NoSleep,
    };
    (
        Lifecycle::new(CollectorState::new(), parts),
        published,
        calls,
    )
}

#[test]
fn successful_publication_precedes_ready_notification() {
    let (mut lifecycle, published, calls) = lifecycle(CollectorBehavior::Succeed, false, false);

    lifecycle.cycle().expect("successful cycle");

    assert!(published.get());
    assert_eq!(calls.get(), 1);
}

#[test]
fn failed_publication_sends_no_notification() {
    let (mut lifecycle, published, calls) = lifecycle(CollectorBehavior::Succeed, true, false);

    let result = lifecycle.cycle();

    assert!(matches!(result, Err(AuraError::Fatal(_))));
    assert!(!published.get());
    assert_eq!(calls.get(), 0);
}

#[test]
fn failed_later_cycle_after_ready_sends_no_watchdog() {
    // Given: a lifecycle that completed one published READY cycle.
    let (mut lifecycle, _published, calls) = lifecycle(CollectorBehavior::Succeed, false, false);
    lifecycle.cycle().expect("successful READY cycle");
    lifecycle.collector_mut().0 = CollectorBehavior::Fail;

    // When: the following collection cycle fails before publication.
    let result = lifecycle.cycle();

    // Then: only READY was sent and no WATCHDOG notification escaped the failed cycle.
    assert!(matches!(result, Err(AuraError::Fatal(message)) if message == "collector failed"));
    assert_eq!(calls.get(), 1);
    assert_eq!(
        lifecycle.notifier().notifications.as_slice(),
        [Notification::Ready]
    );
}

#[test]
fn stopping_failure_does_not_turn_clean_shutdown_into_failure() {
    let (mut lifecycle, _published, calls) = lifecycle(CollectorBehavior::Succeed, false, true);
    let shutdown = AtomicBool::new(true);

    let result = lifecycle.run(heartbeat(1), &shutdown);

    assert!(result.is_ok());
    assert_eq!(calls.get(), 1);
}

#[test]
fn stopping_failure_does_not_mask_prior_fatal_error() {
    let (mut lifecycle, _published, calls) = lifecycle(CollectorBehavior::Fail, false, true);
    let shutdown = AtomicBool::new(false);

    let result = lifecycle.run(heartbeat(1), &shutdown);

    assert!(matches!(result, Err(AuraError::Fatal(message)) if message == "collector failed"));
    assert_eq!(calls.get(), 1);
}

#[test]
fn checked_in_service_is_a_systemd_user_notify_unit() {
    let service = include_str!("../../deployment/systemd/aura-daemon.service");
    assert!(service.contains("Type=notify\n"));
    assert!(service.contains("NotifyAccess=main\n"));
    assert!(service.contains("WatchdogSec=3s\n"));
    assert!(service.contains("RuntimeDirectory=aura\n"));
    assert!(service.contains("RuntimeDirectoryMode=0700\n"));
    assert!(service.contains("WantedBy=default.target\n"));
    assert!(!service.contains("User="));
    assert!(!service.contains("AURA_SHM_PATH"));
    assert!(!service.contains("--shm-path"));
}

#[test]
fn home_manager_uses_user_notify_service_and_optional_shm_override() {
    let module = include_str!("../../deployment/home-manager/default.nix");
    assert!(module.contains("systemd.user.services.aura-daemon"));
    assert!(module.contains("Type = \"notify\""));
    assert!(module.contains("NotifyAccess = \"main\""));
    assert!(module.contains("WatchdogSec = \"3s\""));
    assert!(module.contains("RuntimeDirectory = \"aura\""));
    assert!(module.contains("RuntimeDirectoryMode = \"0700\""));
    assert!(module.contains("lib.optionalString (cfg.shmPath != null)"));
    assert!(module.contains("type = lib.types.nullOr lib.types.str"));
}

#[test]
fn daemon_source_has_no_hardware_watchdog_interaction() {
    let daemon = include_str!("../src/daemon.rs");
    assert!(!daemon.contains("/dev/watchdog"));
    assert!(!daemon.contains("WatchdogDevice"));
    assert!(!daemon.contains("SystemNotifier"));
}
