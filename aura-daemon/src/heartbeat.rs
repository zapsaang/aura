use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use log::{debug, error, info, warn};

use aura_common::AuraResult;

use crate::collectors::{self, CollectorState};
use crate::state::ShmHandle;

#[cfg(target_os = "linux")]
mod watchdog {
    use std::fs::{File, OpenOptions};
    use std::io::Write;
    use std::sync::OnceLock;

    use log::{info, warn};

    static WATCHDOG_FD: OnceLock<Option<File>> = OnceLock::new();

    pub fn init() {
        WATCHDOG_FD.get_or_init(
            || match OpenOptions::new().write(true).open("/dev/watchdog") {
                Ok(fd) => {
                    info!("opened /dev/watchdog for hardware watchdog keepalive");
                    Some(fd)
                }
                Err(e) => {
                    warn!("could not open /dev/watchdog ({e}), watchdog disabled");
                    None
                }
            },
        );
    }

    pub fn pet() {
        if let Some(Some(fd)) = WATCHDOG_FD.get() {
            let mut fd_ref = fd;
            let _ = fd_ref.write(&[0u8]);
        }
    }

    pub fn shutdown() {
        if let Some(Some(fd)) = WATCHDOG_FD.get() {
            let mut fd_ref = fd;
            // Magic close: 'V' (0x56) signals the kernel watchdog driver
            // to stop cleanly instead of triggering a system reset.
            if fd_ref.write(b"V").is_ok() {
                info!("sent magic close to /dev/watchdog");
            }
        }
    }
}

pub fn run(
    mut shm: ShmHandle,
    mut collector_state: CollectorState,
    heartbeat: Duration,
    shutdown_flag: &AtomicBool,
) -> AuraResult<()> {
    info!("entering heartbeat loop ({:?})", heartbeat);

    #[cfg(target_os = "linux")]
    watchdog::init();

    let mut next_wake = Instant::now();

    loop {
        if shutdown_flag.load(Ordering::Relaxed) {
            break;
        }

        let cycle_start = Instant::now();

        if let Err(e) = collectors::collect_all(&mut collector_state) {
            error!("collector error: {e}");
        }

        collector_state.telemetry.meta.timestamp_ns = aura_common::monotonic_ns();

        if let Err(e) = shm.write(&mut collector_state.telemetry) {
            error!("seqlock write error: {e}");
        }

        #[cfg(target_os = "linux")]
        watchdog::pet();

        next_wake += heartbeat;
        let now = Instant::now();

        if now < next_wake {
            let sleep_time = next_wake - now;
            debug!(
                "cycle {:?}, sleeping {:?}",
                cycle_start.elapsed(),
                sleep_time
            );
            std::thread::sleep(sleep_time);
        } else {
            let overrun = now - next_wake;
            debug!("cycle overran by {:?}", overrun);
            if overrun > heartbeat {
                warn!("severe starvation detected, resetting heartbeat anchor");
                next_wake = now;
            }
        }
    }

    #[cfg(target_os = "linux")]
    watchdog::shutdown();

    Ok(())
}
