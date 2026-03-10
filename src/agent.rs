//! Agent trait definition.
//!
//! Agents perform work in a duty cycle without allocation.

use crate::error::{LifecycleResult, WorkResult};

/// Agent role name buffer size.
pub const ROLE_NAME_MAX_LEN: usize = 64;

/// Fixed-size role name storage (no heap allocation).
#[derive(Clone, Copy)]
#[repr(C)]
pub struct RoleName {
    buf: [u8; ROLE_NAME_MAX_LEN],
    len: u8,
}

impl RoleName {
    /// Create empty role name.
    #[inline]
    pub const fn empty() -> Self {
        Self {
            buf: [0u8; ROLE_NAME_MAX_LEN],
            len: 0,
        }
    }

    /// Create from static string (compile-time).
    #[inline]
    pub const fn from_static(s: &'static str) -> Self {
        let bytes = s.as_bytes();
        let len = if bytes.len() > ROLE_NAME_MAX_LEN {
            ROLE_NAME_MAX_LEN
        } else {
            bytes.len()
        };

        let mut buf = [0u8; ROLE_NAME_MAX_LEN];
        let mut i = 0;
        while i < len {
            buf[i] = bytes[i];
            i += 1;
        }

        Self { buf, len: len as u8 }
    }

    /// Get as string slice.
    #[inline]
    pub fn as_str(&self) -> &str {
        // SAFETY: We only store valid UTF-8 from &str inputs
        unsafe { core::str::from_utf8_unchecked(&self.buf[..self.len as usize]) }
    }
}

impl core::fmt::Debug for RoleName {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "RoleName({:?})", self.as_str())
    }
}

impl core::fmt::Display for RoleName {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// An Agent performs work on a duty cycle.
///
/// # Safety Contract
/// - `do_work()` MUST NOT allocate
/// - `do_work()` MUST be O(1) or O(n) with bounded n
/// - All methods called from same thread (single-writer)
///
/// # Lifecycle
/// ```text
/// on_start() → [do_work()]* → on_close()
/// ```
pub trait Agent {
    /// Called once when agent starts.
    ///
    /// May allocate here for initialization.
    #[inline]
    fn on_start(&mut self) -> LifecycleResult {
        Ok(())
    }

    /// Perform one unit of work.
    ///
    /// # Returns
    /// - `Ok(0)` - No work available (triggers idle strategy)
    /// - `Ok(n)` - n units of work completed
    /// - `Err(termination)` - Agent requests termination
    ///
    /// # Contract
    /// - **MUST NOT** allocate
    /// - **MUST** be bounded time
    fn do_work(&mut self) -> WorkResult;

    /// Called once when agent closes.
    ///
    /// May deallocate resources here.
    #[inline]
    fn on_close(&mut self) -> LifecycleResult {
        Ok(())
    }

    /// Get the role name of this agent.
    fn role_name(&self) -> RoleName;
}