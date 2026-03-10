//! Error handling infrastructure.
//!
//! Lock-free, allocation-free error reporting.

use core::sync::atomic::{AtomicU64, Ordering};
use crate::error::AgentTermination;

/// Function pointer type for error handling (no allocation).
pub type ErrorHandlerFn = fn(AgentTermination);

/// Default error handler - logs to stderr.
#[cold]
pub fn default_error_handler(termination: AgentTermination) {
    eprintln!("Agent error: {}", termination);
}

/// Atomic error counter.
///
/// Lock-free, cache-line sized to avoid false sharing.
#[repr(C, align(64))]
pub struct AtomicErrorCounter {
    count: AtomicU64,
    _padding: [u8; 56],
}

impl AtomicErrorCounter {
    /// Create new counter.
    #[inline]
    pub const fn new() -> Self {
        Self {
            count: AtomicU64::new(0),
            _padding: [0u8; 56],
        }
    }

    /// Increment counter (Relaxed ordering - just a counter).
    #[inline]
    pub fn increment(&self) {
        self.count.fetch_add(1, Ordering::Relaxed);
    }

    /// Get current count.
    #[inline]
    pub fn get(&self) -> u64 {
        self.count.load(Ordering::Relaxed)
    }

    /// Reset counter.
    #[inline]
    pub fn reset(&self) {
        self.count.store(0, Ordering::Relaxed);
    }
}

impl Default for AtomicErrorCounter {
    fn default() -> Self {
        Self::new()
    }
}