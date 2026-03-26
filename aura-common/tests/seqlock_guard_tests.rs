use std::panic;
use std::sync::atomic::{AtomicUsize, Ordering};

use aura_common::SeqLockWriterGuard;

#[test]
fn test_guard_completes_version_on_scope_exit() {
    let version = AtomicUsize::new(0);
    {
        let _guard = SeqLockWriterGuard::begin(&version);
        assert_eq!(version.load(Ordering::SeqCst), 1);
    }
    assert_eq!(version.load(Ordering::SeqCst), 2);
}

#[test]
fn test_guard_complete_method_works() {
    let version = AtomicUsize::new(0);
    {
        let guard = SeqLockWriterGuard::begin(&version);
        assert_eq!(version.load(Ordering::SeqCst), 1);
        guard.complete();
        assert_eq!(version.load(Ordering::SeqCst), 2);
    }
    assert_eq!(version.load(Ordering::SeqCst), 2);
}

#[test]
fn test_guard_panics_force_version_to_even() {
    let version = AtomicUsize::new(0);

    let result = panic::catch_unwind(panic::AssertUnwindSafe(|| {
        let _guard = SeqLockWriterGuard::begin(&version);
        assert_eq!(version.load(Ordering::SeqCst), 1);
        panic!("simulated crash");
    }));

    assert!(result.is_err());
    assert_eq!(version.load(Ordering::SeqCst), 2);
}
