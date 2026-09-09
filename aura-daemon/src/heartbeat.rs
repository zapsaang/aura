use std::sync::atomic::AtomicBool;
use std::time::Duration;

use aura_common::AuraResult;

use crate::collectors::{CollectorState, SystemCollector};
use crate::daemon::NoopNotifier;
use crate::finalize::SystemFinalizer;
use crate::lifecycle::{Heartbeat, Lifecycle, LifecycleParts, ThreadSleeper};
use crate::state::ShmHandle;

pub fn run(
    shm: ShmHandle,
    collector_state: CollectorState,
    heartbeat: Duration,
    shutdown_flag: &AtomicBool,
) -> AuraResult<()> {
    let heartbeat = Heartbeat::from_duration(heartbeat)?;
    let parts = LifecycleParts {
        collector: SystemCollector::default(),
        finalizer: SystemFinalizer::default(),
        publisher: shm,
        notifier: NoopNotifier,
        sleeper: ThreadSleeper,
    };
    let mut lifecycle = Lifecycle::new(collector_state, parts);
    lifecycle.run(heartbeat, shutdown_flag)
}
