use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use aura_common::{AuraError, AuraResult, TelemetryArchive};

use crate::collectors::{CollectorState, CycleCollector, FixedCollectorState, ProviderOutcome};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Heartbeat(Duration);

impl Heartbeat {
    pub fn from_millis(milliseconds: u64) -> AuraResult<Self> {
        if milliseconds == 0 {
            return Err(AuraError::Fatal("heartbeat must be positive".to_string()));
        }
        Ok(Self(Duration::from_millis(milliseconds)))
    }

    pub const fn duration(self) -> Duration {
        self.0
    }

    pub fn from_duration(duration: Duration) -> AuraResult<Self> {
        if duration.is_zero() {
            return Err(AuraError::Fatal("heartbeat must be positive".to_string()));
        }
        Ok(Self(duration))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Notification {
    Ready,
    Watchdog,
    Stopping,
}

pub trait Finalizer {
    fn finalize(&mut self, state: &mut FixedCollectorState) -> AuraResult<()>;
}

pub trait Publisher {
    fn publish(&mut self, archive: &TelemetryArchive) -> AuraResult<()>;
}

pub trait Notifier {
    fn notify(&mut self, notification: Notification) -> AuraResult<()>;
}

pub trait Sleeper {
    fn sleep(&mut self, duration: Duration);
}

pub struct ThreadSleeper;

impl Sleeper for ThreadSleeper {
    fn sleep(&mut self, duration: Duration) {
        std::thread::sleep(duration);
    }
}

pub struct Lifecycle<C, F, P, N, S> {
    state: CollectorState,
    collector: C,
    finalizer: F,
    publisher: P,
    notifier: N,
    sleeper: S,
    next_notification: Notification,
}

pub struct LifecycleParts<C, F, P, N, S> {
    pub collector: C,
    pub finalizer: F,
    pub publisher: P,
    pub notifier: N,
    pub sleeper: S,
}

impl<C, F, P, N, S> Lifecycle<C, F, P, N, S>
where
    C: CycleCollector,
    F: Finalizer,
    P: Publisher,
    N: Notifier,
    S: Sleeper,
{
    pub fn new(state: CollectorState, parts: LifecycleParts<C, F, P, N, S>) -> Self {
        Self {
            state,
            collector: parts.collector,
            finalizer: parts.finalizer,
            publisher: parts.publisher,
            notifier: parts.notifier,
            sleeper: parts.sleeper,
            next_notification: Notification::Ready,
        }
    }

    pub fn warm_up(&mut self, heartbeat: Heartbeat) -> AuraResult<()> {
        self.collect_and_finalize()?;
        self.state.commit_staging();
        self.sleeper.sleep(heartbeat.duration());
        Ok(())
    }

    pub fn cycle(&mut self) -> AuraResult<()> {
        self.collect_and_finalize()?;
        self.publisher
            .publish(&self.state.staging().archive)
            .map_err(fatal_boundary)?;
        self.state.commit_staging();
        self.notifier
            .notify(self.next_notification)
            .map_err(fatal_boundary)?;
        self.next_notification = Notification::Watchdog;
        Ok(())
    }

    pub fn run(&mut self, heartbeat: Heartbeat, shutdown: &AtomicBool) -> AuraResult<()> {
        let result = (|| {
            if shutdown.load(Ordering::Acquire) {
                return Ok(());
            }
            self.warm_up(heartbeat)?;
            while !shutdown.load(Ordering::Acquire) {
                self.cycle()?;
                self.sleeper.sleep(heartbeat.duration());
            }
            Ok(())
        })();
        let _ = self.notifier.notify(Notification::Stopping);
        result
    }

    pub fn state(&self) -> &CollectorState {
        &self.state
    }

    pub fn collector(&self) -> &C {
        &self.collector
    }

    pub fn collector_mut(&mut self) -> &mut C {
        &mut self.collector
    }

    pub fn finalizer(&self) -> &F {
        &self.finalizer
    }

    pub fn publisher(&self) -> &P {
        &self.publisher
    }

    pub fn notifier(&self) -> &N {
        &self.notifier
    }

    pub fn sleeper(&self) -> &S {
        &self.sleeper
    }

    fn collect_and_finalize(&mut self) -> AuraResult<()> {
        self.state.prepare_staging();
        let outcome = {
            let (staging, scratch) = self.state.split_staging();
            self.collector.collect(staging, scratch)
        };
        match outcome {
            ProviderOutcome::Available(()) | ProviderOutcome::Unavailable => {}
            ProviderOutcome::Fatal(error) => return Err(fatal_boundary(error)),
        }
        self.finalizer
            .finalize(self.state.staging_mut())
            .map_err(fatal_boundary)
    }
}

fn fatal_boundary(error: AuraError) -> AuraError {
    match error {
        AuraError::Fatal(_) => error,
        other => AuraError::Fatal(other.to_string()),
    }
}

impl Publisher for crate::state::ShmHandle {
    fn publish(&mut self, archive: &TelemetryArchive) -> AuraResult<()> {
        self.write(archive)
    }
}
