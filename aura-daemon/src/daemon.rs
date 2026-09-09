use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};

use aura_common::{AuraError, AuraResult};
use env_logger::{Builder, Env};
use log::info;

use crate::collectors::{self, CollectorState, SystemCollector};
use crate::finalize::SystemFinalizer;
use crate::lifecycle::{
    Heartbeat, Lifecycle, LifecycleParts, Notification, Notifier, ThreadSleeper,
};
use crate::notify::SystemdNotifier;
use crate::state::ShmHandle;

static SHUTDOWN: AtomicBool = AtomicBool::new(false);

#[derive(Clone, Debug)]
pub struct DaemonConfig {
    pub shm_path: Option<PathBuf>,
    pub heartbeat_ms: u64,
    pub verbose: bool,
    pub foreground: bool,
}

pub trait SignalInstaller {
    fn install(&mut self) -> AuraResult<()>;
}

pub struct PosixSignalInstaller;

impl SignalInstaller for PosixSignalInstaller {
    fn install(&mut self) -> AuraResult<()> {
        install_signal(libc::SIGINT)?;
        install_signal(libc::SIGTERM)
    }
}

pub fn install_signals(installer: &mut impl SignalInstaller) -> AuraResult<()> {
    installer.install().map_err(as_fatal)
}

pub fn logging_filter(verbose: bool, rust_log: Option<&str>) -> &str {
    rust_log.unwrap_or(if verbose { "debug" } else { "info" })
}

pub fn run(config: DaemonConfig) -> AuraResult<()> {
    init_logging(config.verbose);
    let heartbeat = Heartbeat::from_millis(config.heartbeat_ms)?;
    let notifier = SystemdNotifier::from_process_environment(heartbeat)?;
    SHUTDOWN.store(false, Ordering::Release);
    install_signals(&mut PosixSignalInstaller)?;
    info!("AURA daemon starting");
    info!("heartbeat: {}ms", config.heartbeat_ms);

    let publisher = match config.shm_path {
        Some(path) => ShmHandle::new(&path)?,
        None => ShmHandle::new_default()?,
    };
    let mut state = CollectorState::new();
    collectors::init(&mut state)?;
    let parts = LifecycleParts {
        collector: SystemCollector::default(),
        finalizer: SystemFinalizer::default(),
        publisher,
        notifier,
        sleeper: ThreadSleeper,
    };
    let mut lifecycle = Lifecycle::new(state, parts);
    lifecycle.run(heartbeat, &SHUTDOWN)
}

pub(crate) struct NoopNotifier;

impl Notifier for NoopNotifier {
    fn notify(&mut self, _notification: Notification) -> AuraResult<()> {
        Ok(())
    }
}

fn init_logging(verbose: bool) {
    let default = logging_filter(verbose, None);
    let environment = Env::default().default_filter_or(default);
    let _ = Builder::from_env(environment)
        .format_timestamp_millis()
        .try_init();
}

extern "C" fn shutdown_handler(_signal: libc::c_int) {
    SHUTDOWN.store(true, Ordering::Release);
}

fn install_signal(signal: libc::c_int) -> AuraResult<()> {
    // SAFETY: zero is a valid initial state for sigaction before its fields and mask are initialized.
    let mut action = unsafe { std::mem::zeroed::<libc::sigaction>() };
    action.sa_sigaction = shutdown_handler as *const () as usize;
    action.sa_flags = 0;
    // SAFETY: action has valid storage and sigemptyset receives its writable mask pointer.
    if unsafe { libc::sigemptyset(&mut action.sa_mask) } != 0 {
        return Err(AuraError::Fatal(format!(
            "sigemptyset failed: {}",
            std::io::Error::last_os_error()
        )));
    }
    // SAFETY: the initialized action contains a C-ABI handler and sigaction receives valid pointers.
    if unsafe { libc::sigaction(signal, &action, std::ptr::null_mut()) } != 0 {
        return Err(AuraError::Fatal(format!(
            "sigaction({signal}) failed: {}",
            std::io::Error::last_os_error()
        )));
    }
    Ok(())
}

fn as_fatal(error: AuraError) -> AuraError {
    match error {
        AuraError::Fatal(_) => error,
        other => AuraError::Fatal(other.to_string()),
    }
}
