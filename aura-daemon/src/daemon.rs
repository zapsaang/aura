use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};

#[cfg(target_os = "linux")]
use std::fs::{File, OpenOptions};
#[cfg(target_os = "linux")]
use std::io::{self, Write};

use aura_common::{AuraError, AuraResult};
use env_logger::{Builder, Env};
use log::info;
#[cfg(target_os = "linux")]
use log::warn;

use crate::collectors::{self, CollectorState, SystemCollector};
use crate::finalize::SystemFinalizer;
use crate::lifecycle::{
    Heartbeat, Lifecycle, LifecycleParts, Notification, Notifier, ThreadSleeper,
};
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
        notifier: SystemNotifier::new(),
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

struct SystemNotifier {
    #[cfg(target_os = "linux")]
    watchdog: Option<WatchdogDevice<File>>,
}

impl SystemNotifier {
    fn new() -> Self {
        #[cfg(target_os = "linux")]
        {
            let watchdog = match OpenOptions::new().write(true).open("/dev/watchdog") {
                Ok(file) => {
                    info!("opened /dev/watchdog for hardware watchdog keepalive");
                    Some(WatchdogDevice::new(file))
                }
                Err(error) => {
                    warn!("could not open /dev/watchdog ({error}), watchdog disabled");
                    None
                }
            };
            Self { watchdog }
        }
        #[cfg(not(target_os = "linux"))]
        {
            Self {}
        }
    }
}

impl Notifier for SystemNotifier {
    fn notify(&mut self, _notification: Notification) -> AuraResult<()> {
        #[cfg(target_os = "linux")]
        if let Some(watchdog) = &mut self.watchdog {
            watchdog.pet()?;
        }
        Ok(())
    }
}

#[cfg(target_os = "linux")]
impl Drop for SystemNotifier {
    fn drop(&mut self) {
        if let Some(watchdog) = &mut self.watchdog {
            if watchdog.stop().is_ok() {
                info!("sent magic close to /dev/watchdog");
            }
        }
    }
}

#[cfg(target_os = "linux")]
struct WatchdogDevice<W: Write> {
    writer: W,
}

#[cfg(target_os = "linux")]
impl<W: Write> WatchdogDevice<W> {
    const fn new(writer: W) -> Self {
        Self { writer }
    }

    fn pet(&mut self) -> io::Result<()> {
        self.writer.write_all(&[0])
    }

    fn stop(&mut self) -> io::Result<()> {
        self.writer.write_all(b"V")
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

#[cfg(test)]
mod tests {
    #[cfg(target_os = "linux")]
    use super::WatchdogDevice;

    #[test]
    #[cfg(target_os = "linux")]
    fn watchdog_device_emits_pet_and_magic_close() {
        let mut bytes = Vec::new();
        let mut device = WatchdogDevice::new(&mut bytes);
        device.pet().expect("pet watchdog");
        device.stop().expect("stop watchdog");
        assert_eq!(bytes, [0, b'V']);
    }
}
