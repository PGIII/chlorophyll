#![no_std]

use core::sync::atomic::AtomicBool;

/// Shared application state passed as `Arc<State>` across Embassy tasks.
#[derive(Debug, Default)]
pub struct State {
    pub is_fast_mode: AtomicBool,
    pub was_reset_by_watchdog: AtomicBool,
}
