use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use log::{debug, error, info};

use aura_common::AuraResult;

use crate::collectors::{self, CollectorState};
use crate::state::ShmHandle;

pub fn run(
    mut shm: ShmHandle,
    mut collector_state: CollectorState,
    heartbeat: Duration,
    _foreground: bool,
) -> AuraResult<()> {
    let shutdown_flag = AtomicBool::new(false);
    info!("entering heartbeat loop ({:?})", heartbeat);

    loop {
        if shutdown_flag.load(Ordering::Relaxed) {
            break;
        }

        let cycle_start = Instant::now();

        if let Err(e) = collectors::collect_all(&mut collector_state) {
            error!("collector error: {e}");
        }

        if let Err(e) = shm.write(&collector_state.telemetry) {
            error!("seqlock write error: {e}");
        }

        #[cfg(target_os = "linux")]
        crate::platform::linux::send_watchdog_heartbeat();

        let elapsed = cycle_start.elapsed();
        if elapsed < heartbeat {
            let sleep_time = heartbeat - elapsed;
            debug!("cycle {:?}, sleeping {:?}", elapsed, sleep_time);
            std::thread::sleep(sleep_time);
        } else {
            debug!("cycle overran {:?}", elapsed - heartbeat);
        }
    }

    Ok(())
}
