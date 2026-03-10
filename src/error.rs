//! Agent termination error types.
//!
//! Zero-allocation error handling for agent lifecycle.

use core::fmt;

/// Termination reason for an agent.
///
/// Stored inline, no heap allocation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum TerminationReason {
    /// Normal termination requested by agent.
    Requested = 0,
    /// Termination due to an error condition.
    Error = 1,
    /// Termination due to interrupt.
    Interrupted = 2,
    /// Termination due to timeout.
    Timeout = 3,
}

/// Agent termination signal.
///
/// Fixed-size, no allocation, Copy semantics.
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct AgentTermination {
    reason: TerminationReason,
    /// Error code (application-defined)
    error_code: u32,
}

impl AgentTermination {
    /// Create a new termination signal.
    #[inline]
    pub const fn new(reason: TerminationReason) -> Self {
        Self {
            reason,
            error_code: 0,
        }
    }

    /// Create termination with error code.
    #[inline]
    pub const fn with_error(reason: TerminationReason, error_code: u32) -> Self {
        Self { reason, error_code }
    }

    /// Get termination reason.
    #[inline]
    pub const fn reason(&self) -> TerminationReason {
        self.reason
    }

    /// Get error code.
    #[inline]
    pub const fn error_code(&self) -> u32 {
        self.error_code
    }
}

impl fmt::Display for AgentTermination {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.reason {
            TerminationReason::Requested => write!(f, "termination requested"),
            TerminationReason::Error => write!(f, "error (code: {})", self.error_code),
            TerminationReason::Interrupted => write!(f, "interrupted"),
            TerminationReason::Timeout => write!(f, "timeout"),
        }
    }
}

/// Result type for agent work cycle.
pub type WorkResult = Result<u32, AgentTermination>;

/// Result type for agent lifecycle methods.
pub type LifecycleResult = Result<(), AgentTermination>;