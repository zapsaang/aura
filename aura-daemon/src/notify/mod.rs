use std::ffi::OsStr;
use std::time::{Duration, Instant};

#[cfg(target_os = "linux")]
use aura_common::AuraError;
use aura_common::AuraResult;

use crate::lifecycle::{Heartbeat, Notification, Notifier};

#[cfg(target_os = "linux")]
mod linux;

#[cfg(target_os = "linux")]
const READY_MESSAGE: &[u8] = b"READY=1\n";
#[cfg(target_os = "linux")]
const WATCHDOG_MESSAGE: &[u8] = b"WATCHDOG=1\n";
#[cfg(target_os = "linux")]
const STOPPING_MESSAGE: &[u8] = b"STOPPING=1\n";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NotifierMode {
    Disabled,
    ReadyOnly,
    Watchdog { half_interval: Duration },
    Unavailable,
}

#[derive(Clone, Copy)]
pub struct NotifyEnvironment<'a> {
    pub notify_socket: Option<&'a OsStr>,
    pub watchdog_pid: Option<&'a OsStr>,
    pub watchdog_usec: Option<&'a OsStr>,
}

pub struct SystemdNotifier {
    mode: NotifierMode,
    epoch: Instant,
    #[cfg(target_os = "linux")]
    transport: Option<linux::LinuxTransport>,
    #[cfg(target_os = "linux")]
    next_watchdog: Option<Duration>,
}

impl SystemdNotifier {
    pub fn from_process_environment(heartbeat: Heartbeat) -> AuraResult<Self> {
        #[cfg(target_os = "linux")]
        {
            let notify_socket = std::env::var_os("NOTIFY_SOCKET");
            let watchdog_pid = std::env::var_os("WATCHDOG_PID");
            let watchdog_usec = std::env::var_os("WATCHDOG_USEC");
            Self::negotiate(
                NotifyEnvironment {
                    notify_socket: notify_socket.as_deref(),
                    watchdog_pid: watchdog_pid.as_deref(),
                    watchdog_usec: watchdog_usec.as_deref(),
                },
                heartbeat,
                std::process::id(),
            )
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = heartbeat;
            Ok(Self {
                mode: NotifierMode::Unavailable,
                epoch: Instant::now(),
            })
        }
    }

    pub fn negotiate(
        environment: NotifyEnvironment<'_>,
        heartbeat: Heartbeat,
        current_pid: u32,
    ) -> AuraResult<Self> {
        #[cfg(target_os = "linux")]
        {
            let Some(notify_socket) = environment.notify_socket else {
                return Ok(Self {
                    mode: NotifierMode::Disabled,
                    epoch: Instant::now(),
                    transport: None,
                    next_watchdog: None,
                });
            };
            let watchdog_pid = environment
                .watchdog_pid
                .map(|value| linux::parse_decimal("WATCHDOG_PID", value))
                .transpose()?;
            let watchdog_usec = environment
                .watchdog_usec
                .map(|value| linux::parse_decimal("WATCHDOG_USEC", value))
                .transpose()?;
            let mode = match (watchdog_pid, watchdog_usec) {
                (_, None | Some(0)) => NotifierMode::ReadyOnly,
                (Some(pid), Some(_)) if pid != u64::from(current_pid) => NotifierMode::ReadyOnly,
                (None | Some(_), Some(microseconds)) => {
                    if heartbeat.duration().as_millis() > u128::from(microseconds / 2_000) {
                        return Err(fatal(
                            "heartbeat exceeds half of the negotiated WATCHDOG_USEC interval",
                        ));
                    }
                    NotifierMode::Watchdog {
                        half_interval: Duration::from_micros(microseconds / 2),
                    }
                }
            };
            Ok(Self {
                mode,
                epoch: Instant::now(),
                transport: Some(linux::LinuxTransport::new(notify_socket)?),
                next_watchdog: None,
            })
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = (environment, heartbeat, current_pid);
            Ok(Self {
                mode: NotifierMode::Unavailable,
                epoch: Instant::now(),
            })
        }
    }

    pub const fn mode(&self) -> NotifierMode {
        self.mode
    }

    pub fn notify_at(&mut self, notification: Notification, now: Duration) -> AuraResult<()> {
        #[cfg(target_os = "linux")]
        {
            let Some(transport) = &self.transport else {
                return Ok(());
            };
            match notification {
                Notification::Ready => {
                    transport.send(READY_MESSAGE, "READY=1")?;
                    if let NotifierMode::Watchdog { half_interval } = self.mode {
                        self.next_watchdog =
                            Some(now.checked_add(half_interval).ok_or_else(|| {
                                fatal("systemd watchdog deadline overflowed monotonic duration")
                            })?);
                    }
                    Ok(())
                }
                Notification::Watchdog => {
                    let (NotifierMode::Watchdog { half_interval }, Some(deadline)) =
                        (self.mode, self.next_watchdog)
                    else {
                        return Ok(());
                    };
                    if now < deadline {
                        return Ok(());
                    }
                    transport.send(WATCHDOG_MESSAGE, "WATCHDOG=1")?;
                    self.next_watchdog = Some(now.checked_add(half_interval).ok_or_else(|| {
                        fatal("systemd watchdog deadline overflowed monotonic duration")
                    })?);
                    Ok(())
                }
                Notification::Stopping => transport.send(STOPPING_MESSAGE, "STOPPING=1"),
            }
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = (notification, now);
            Ok(())
        }
    }
}

impl Notifier for SystemdNotifier {
    fn notify(&mut self, notification: Notification) -> AuraResult<()> {
        self.notify_at(notification, self.epoch.elapsed())
    }
}

#[cfg(target_os = "linux")]
fn fatal(message: &str) -> AuraError {
    AuraError::Fatal(message.to_string())
}
