use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use log::{debug, error, info, warn};

use aura_common::AuraResult;

use crate::collectors::{self, CollectorState};
use crate::state::ShmHandle;

pub fn run(
    mut shm: ShmHandle,
    mut collector_state: CollectorState,
    heartbeat: Duration,
    shutdown_flag: &AtomicBool,
) -> AuraResult<()> {
    info!("entering heartbeat loop ({:?})", heartbeat);

    let mut next_wake = Instant::now();

    loop {
        if shutdown_flag.load(Ordering::Relaxed) {
            break;
        }

        let cycle_start = Instant::now();

        if let Err(e) = collectors::collect_all(&mut collector_state) {
            error!("collector error: {e}");
        }

        if let Err(e) = shm.write(&mut collector_state.telemetry) {
            error!("seqlock write error: {e}");
        }

        #[cfg(target_os = "linux")]
        send_watchdog_heartbeat();

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

    Ok(())
}

#[cfg(target_os = "linux")]
fn send_watchdog_heartbeat() {
    let _ = std::fs::write("/proc/sys/kernel/watchdog", "1");
}
