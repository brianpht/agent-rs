//! Idle strategies for agent duty cycles.
//!
//! All strategies are allocation-free and deterministic.
//!
//! # Cache Line Padding
//! Stateful strategies use cache line padding (128 bytes) to prevent
//! false sharing when multiple threads use different strategy instances.
//!
//! # State Machine (BackoffIdleStrategy)
//! ```text
//! NOT_IDLE → SPINNING → YIELDING → PARKING
//!     ↑___________|___________|_________|
//!               reset()
//! ```

use core::fmt;
use std::time::Duration;

/// Alias name buffer size.
pub const ALIAS_MAX_LEN: usize = 16;

/// Fixed-size alias storage (no heap allocation).
#[derive(Clone, Copy)]
#[repr(C)]
pub struct Alias {
    buf: [u8; ALIAS_MAX_LEN],
    len: u8,
}

impl Alias {
    /// Create from static string (compile-time).
    #[inline]
    pub const fn from_static(s: &'static str) -> Self {
        let bytes = s.as_bytes();
        let len = if bytes.len() > ALIAS_MAX_LEN {
            ALIAS_MAX_LEN
        } else {
            bytes.len()
        };

        let mut buf = [0u8; ALIAS_MAX_LEN];
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

impl fmt::Debug for Alias {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self.as_str())
    }
}

impl fmt::Display for Alias {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Idle strategy for use by threads when they do not have work to do.
///
/// # Contract
/// - `idle(work_count)`: Called after each work cycle
///   - `work_count > 0`: Work was done, may reset backoff state
///   - `work_count == 0`: No work, apply idle action
/// - `idle()`: Unconditionally apply idle action
/// - `reset()`: Reset internal state for new idle period
///
/// # Usage Pattern
/// ```ignore
/// while is_running {
///     idle_strategy.idle(do_work());
/// }
/// ```
///
/// # Note on TTSP (Time To Safe Point)
/// Some implementations may affect JVM safepoint behavior.
/// In Rust, this is less of a concern, but spin loops should
/// use `core::hint::spin_loop()` for CPU efficiency.
pub trait IdleStrategy {
    /// Idle based on work count from last duty cycle.
    ///
    /// - `work_count > 0`: Reset backoff, return immediately
    /// - `work_count == 0`: Apply idle action (spin/yield/park)
    fn idle(&mut self, work_count: i32);

    /// Unconditionally apply idle action.
    ///
    /// Use with `reset()` for manual idle control:
    /// ```ignore
    /// idle_strategy.reset();
    /// while !has_work() {
    ///     idle_strategy.idle_unconditional();
    /// }
    /// ```
    fn idle_unconditional(&mut self);

    /// Reset internal state for new idle period.
    fn reset(&mut self);

    /// Simple name for strategy identification.
    fn alias(&self) -> Alias;
}

// ============================================================================
// NoOpIdleStrategy
// ============================================================================

/// No-op idle strategy - does nothing.
///
/// Use when caller handles idle behavior externally.
/// **Warning**: Will busy-spin without any CPU yielding.
#[derive(Debug, Default, Clone, Copy)]
pub struct NoOpIdleStrategy;

impl NoOpIdleStrategy {
    /// Alias name.
    pub const ALIAS: Alias = Alias::from_static("noop");

    /// Singleton instance (stateless, safe to share).
    pub const INSTANCE: Self = Self;

    /// Create new instance.
    #[inline]
    pub const fn new() -> Self {
        Self
    }
}

impl IdleStrategy for NoOpIdleStrategy {
    #[inline(always)]
    fn idle(&mut self, _work_count: i32) {
        // Do nothing
    }

    #[inline(always)]
    fn idle_unconditional(&mut self) {
        // Do nothing
    }

    #[inline(always)]
    fn reset(&mut self) {
        // No state to reset
    }

    #[inline(always)]
    fn alias(&self) -> Alias {
        Self::ALIAS
    }
}

impl fmt::Display for NoOpIdleStrategy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "NoOpIdleStrategy{{alias={}}}", Self::ALIAS)
    }
}

// ============================================================================
// BusySpinIdleStrategy
// ============================================================================

/// Busy spin strategy for lowest possible latency.
///
/// This strategy will monopolize a thread to achieve minimum latency.
/// Uses `core::hint::spin_loop()` (equivalent to `Thread.onSpinWait()`).
///
/// # Use Cases
/// - Ultra-low latency requirements
/// - Dedicated CPU cores
/// - Short expected idle periods
///
/// # Warning
/// Will consume 100% CPU on the thread.
#[derive(Debug, Default, Clone, Copy)]
pub struct BusySpinIdleStrategy;

impl BusySpinIdleStrategy {
    /// Alias name.
    pub const ALIAS: Alias = Alias::from_static("spin");

    /// Singleton instance (stateless, safe to share).
    pub const INSTANCE: Self = Self;

    /// Create new instance.
    #[inline]
    pub const fn new() -> Self {
        Self
    }
}

impl IdleStrategy for BusySpinIdleStrategy {
    #[inline(always)]
    fn idle(&mut self, work_count: i32) {
        if work_count > 0 {
            return;
        }
        core::hint::spin_loop();
    }

    #[inline(always)]
    fn idle_unconditional(&mut self) {
        core::hint::spin_loop();
    }

    #[inline(always)]
    fn reset(&mut self) {
        // No state to reset
    }

    #[inline(always)]
    fn alias(&self) -> Alias {
        Self::ALIAS
    }
}

impl fmt::Display for BusySpinIdleStrategy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "BusySpinIdleStrategy{{alias={}}}", Self::ALIAS)
    }
}

// ============================================================================
// YieldingIdleStrategy
// ============================================================================

/// Yielding strategy - calls `thread::yield_now()` when idle.
///
/// Good balance between latency and CPU usage.
/// Allows other threads to run while waiting for work.
#[derive(Debug, Default, Clone, Copy)]
pub struct YieldingIdleStrategy;

impl YieldingIdleStrategy {
    /// Alias name.
    pub const ALIAS: Alias = Alias::from_static("yield");

    /// Singleton instance (stateless, safe to share).
    pub const INSTANCE: Self = Self;

    /// Create new instance.
    #[inline]
    pub const fn new() -> Self {
        Self
    }
}

impl IdleStrategy for YieldingIdleStrategy {
    #[inline(always)]
    fn idle(&mut self, work_count: i32) {
        if work_count > 0 {
            return;
        }
        std::thread::yield_now();
    }

    #[inline(always)]
    fn idle_unconditional(&mut self) {
        std::thread::yield_now();
    }

    #[inline(always)]
    fn reset(&mut self) {
        // No state to reset
    }

    #[inline(always)]
    fn alias(&self) -> Alias {
        Self::ALIAS
    }
}

impl fmt::Display for YieldingIdleStrategy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "YieldingIdleStrategy{{alias={}}}", Self::ALIAS)
    }
}

// ============================================================================
// SleepingIdleStrategy (nanoseconds)
// ============================================================================

/// Sleeping strategy using nanosecond precision.
///
/// Uses `thread::park_timeout()` (equivalent to `LockSupport.parkNanos()`).
///
/// # Linux Note
/// Timer events may be coalesced in a 50µs window. To improve precision:
/// ```bash
/// echo 10000 > /proc/PID/timerslack_ns
/// ```
#[derive(Debug, Clone, Copy)]
pub struct SleepingIdleStrategy {
    sleep_period_ns: u64,
}

impl SleepingIdleStrategy {
    /// Alias name.
    pub const ALIAS: Alias = Alias::from_static("sleep-ns");

    /// Default sleep period (1µs) - minimum effective on Linux.
    pub const DEFAULT_SLEEP_PERIOD_NS: u64 = 1_000;

    /// Create with default sleep period.
    #[inline]
    pub const fn new() -> Self {
        Self {
            sleep_period_ns: Self::DEFAULT_SLEEP_PERIOD_NS,
        }
    }

    /// Create with custom sleep period in nanoseconds.
    #[inline]
    pub const fn with_period_ns(sleep_period_ns: u64) -> Self {
        Self { sleep_period_ns }
    }

    /// Create with duration.
    #[inline]
    pub const fn with_duration(duration: Duration) -> Self {
        Self {
            sleep_period_ns: duration.as_nanos() as u64,
        }
    }

    /// Get configured sleep period.
    #[inline]
    pub const fn sleep_period_ns(&self) -> u64 {
        self.sleep_period_ns
    }
}

impl Default for SleepingIdleStrategy {
    fn default() -> Self {
        Self::new()
    }
}

impl IdleStrategy for SleepingIdleStrategy {
    #[inline(always)]
    fn idle(&mut self, work_count: i32) {
        if work_count > 0 {
            return;
        }
        std::thread::park_timeout(Duration::from_nanos(self.sleep_period_ns));
    }

    #[inline(always)]
    fn idle_unconditional(&mut self) {
        std::thread::park_timeout(Duration::from_nanos(self.sleep_period_ns));
    }

    #[inline(always)]
    fn reset(&mut self) {
        // No state to reset
    }

    #[inline(always)]
    fn alias(&self) -> Alias {
        Self::ALIAS
    }
}

impl fmt::Display for SleepingIdleStrategy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "SleepingIdleStrategy{{alias={}, sleepPeriodNs={}}}",
            Self::ALIAS,
            self.sleep_period_ns
        )
    }
}

// ============================================================================
// SleepingMillisIdleStrategy
// ============================================================================

/// Sleeping strategy using millisecond precision.
///
/// Uses `thread::sleep()` (equivalent to `Thread.sleep()`).
#[derive(Debug, Clone, Copy)]
pub struct SleepingMillisIdleStrategy {
    sleep_period_ms: u64,
}

impl SleepingMillisIdleStrategy {
    /// Alias name.
    pub const ALIAS: Alias = Alias::from_static("sleep-ms");

    /// Default sleep period (1ms).
    pub const DEFAULT_SLEEP_PERIOD_MS: u64 = 1;

    /// Create with default sleep period.
    #[inline]
    pub const fn new() -> Self {
        Self {
            sleep_period_ms: Self::DEFAULT_SLEEP_PERIOD_MS,
        }
    }

    /// Create with custom sleep period in milliseconds.
    #[inline]
    pub const fn with_period_ms(sleep_period_ms: u64) -> Self {
        Self { sleep_period_ms }
    }

    /// Get configured sleep period.
    #[inline]
    pub const fn sleep_period_ms(&self) -> u64 {
        self.sleep_period_ms
    }
}

impl Default for SleepingMillisIdleStrategy {
    fn default() -> Self {
        Self::new()
    }
}

impl IdleStrategy for SleepingMillisIdleStrategy {
    #[inline(always)]
    fn idle(&mut self, work_count: i32) {
        if work_count > 0 {
            return;
        }
        std::thread::sleep(Duration::from_millis(self.sleep_period_ms));
    }

    #[inline(always)]
    fn idle_unconditional(&mut self) {
        std::thread::sleep(Duration::from_millis(self.sleep_period_ms));
    }

    #[inline(always)]
    fn reset(&mut self) {
        // No state to reset
    }

    #[inline(always)]
    fn alias(&self) -> Alias {
        Self::ALIAS
    }
}

impl fmt::Display for SleepingMillisIdleStrategy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "SleepingMillisIdleStrategy{{alias={}, sleepPeriodMs={}}}",
            Self::ALIAS,
            self.sleep_period_ms
        )
    }
}

// ============================================================================
// BackoffIdleStrategy
// ============================================================================

/// Backoff state machine states.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
enum BackoffState {
    /// Not idle - initial state.
    NotIdle = 0,
    /// Spinning phase.
    Spinning = 1,
    /// Yielding phase.
    Yielding = 2,
    /// Parking phase with exponential backoff.
    Parking = 3,
}

/// Pre-padding for cache line isolation (64 bytes).
#[repr(C)]
struct BackoffPrePad {
    _pad: [u8; 64],
}

/// Post-padding for cache line isolation (64 bytes).
#[repr(C)]
struct BackoffPostPad {
    _pad: [u8; 64],
}

/// Backoff idle strategy with progressive phases.
///
/// # Phases
/// 1. **Spin**: Call `spin_loop()` for `max_spins` iterations
/// 2. **Yield**: Call `thread::yield_now()` for `max_yields` iterations
/// 3. **Park**: Call `thread::park_timeout()` with exponential backoff
///
/// # State Machine
/// ```text
/// NOT_IDLE → SPINNING → YIELDING → PARKING
///     ↑___________|___________|_________|
///               reset()
/// ```
///
/// # Exponential Backoff
/// Park period doubles each iteration: `min_park_ns → 2x → 4x → ... → max_park_ns`
///
/// # Cache Line Padding
/// Uses 128 bytes total padding to prevent false sharing.
///
/// # Linux Timer Slack
/// Timer events may be coalesced. Adjust via:
/// ```bash
/// echo 10000 > /proc/PID/timerslack_ns
/// ```
#[repr(C)]
pub struct BackoffIdleStrategy {
    // Pre-padding (64 bytes)
    _pre_pad: BackoffPrePad,

    // Configuration (immutable after construction)
    max_spins: u64,
    max_yields: u64,
    min_park_period_ns: u64,
    max_park_period_ns: u64,

    // Mutable state
    state: BackoffState,
    spins: u64,
    yields: u64,
    park_period_ns: u64,

    // Post-padding (64 bytes)
    _post_pad: BackoffPostPad,
}

impl BackoffIdleStrategy {
    /// Alias name.
    pub const ALIAS: Alias = Alias::from_static("backoff");

    /// Default max spins before yielding.
    pub const DEFAULT_MAX_SPINS: u64 = 10;

    /// Default max yields before parking.
    pub const DEFAULT_MAX_YIELDS: u64 = 5;

    /// Default minimum park period (1µs).
    pub const DEFAULT_MIN_PARK_PERIOD_NS: u64 = 1_000;

    /// Default maximum park period (1ms).
    pub const DEFAULT_MAX_PARK_PERIOD_NS: u64 = 1_000_000;

    /// Create with default parameters.
    #[inline]
    pub const fn new() -> Self {
        Self::with_params(
            Self::DEFAULT_MAX_SPINS,
            Self::DEFAULT_MAX_YIELDS,
            Self::DEFAULT_MIN_PARK_PERIOD_NS,
            Self::DEFAULT_MAX_PARK_PERIOD_NS,
        )
    }

    /// Create with custom parameters.
    ///
    /// # Arguments
    /// - `max_spins`: Spins before moving to yield phase
    /// - `max_yields`: Yields before moving to park phase
    /// - `min_park_period_ns`: Initial park duration
    /// - `max_park_period_ns`: Maximum park duration (exponential cap)
    #[inline]
    pub const fn with_params(
        max_spins: u64,
        max_yields: u64,
        min_park_period_ns: u64,
        max_park_period_ns: u64,
    ) -> Self {
        Self {
            _pre_pad: BackoffPrePad { _pad: [0u8; 64] },
            max_spins,
            max_yields,
            min_park_period_ns,
            max_park_period_ns,
            state: BackoffState::NotIdle,
            spins: 0,
            yields: 0,
            park_period_ns: min_park_period_ns,
            _post_pad: BackoffPostPad { _pad: [0u8; 64] },
        }
    }

    /// Get max spins configuration.
    #[inline]
    pub const fn max_spins(&self) -> u64 {
        self.max_spins
    }

    /// Get max yields configuration.
    #[inline]
    pub const fn max_yields(&self) -> u64 {
        self.max_yields
    }

    /// Get min park period configuration.
    #[inline]
    pub const fn min_park_period_ns(&self) -> u64 {
        self.min_park_period_ns
    }

    /// Get max park period configuration.
    #[inline]
    pub const fn max_park_period_ns(&self) -> u64 {
        self.max_park_period_ns
    }
}

impl Default for BackoffIdleStrategy {
    fn default() -> Self {
        Self::new()
    }
}

impl IdleStrategy for BackoffIdleStrategy {
    /// Idle based on work count.
    /// 
    /// # Hot Path
    /// When work_count > 0, resets immediately (most common case).
    #[inline(always)]
    fn idle(&mut self, work_count: i32) {
        if work_count > 0 {
            self.reset();
        } else {
            self.idle_unconditional();
        }
    }

    /// Apply idle action unconditionally.
    /// 
    /// # State Machine
    /// Match arms ordered by expected frequency in typical workloads:
    /// 1. Spinning - most time spent here in low-latency scenarios
    /// 2. NotIdle - initial transition
    /// 3. Yielding - moderate idle
    /// 4. Parking - long idle (cold path)
    #[inline(always)]
    fn idle_unconditional(&mut self) {
        match self.state {
            // Fast path: spinning is most common in active workloads
            BackoffState::Spinning => {
                core::hint::spin_loop();
                self.spins = self.spins.wrapping_add(1);
                if self.spins > self.max_spins {
                    self.state = BackoffState::Yielding;
                    self.yields = 0;
                }
            }

            BackoffState::NotIdle => {
                self.state = BackoffState::Spinning;
                self.spins = self.spins.wrapping_add(1);
            }

            BackoffState::Yielding => {
                self.yields = self.yields.wrapping_add(1);
                if self.yields > self.max_yields {
                    self.state = BackoffState::Parking;
                    self.park_period_ns = self.min_park_period_ns;
                } else {
                    std::thread::yield_now();
                }
            }

            // Cold path: parking involves syscall
            BackoffState::Parking => {
                std::thread::park_timeout(Duration::from_nanos(self.park_period_ns));
                // Exponential backoff: double period, cap at max
                // Using saturating to avoid overflow, then min for cap
                let doubled = self.park_period_ns.saturating_mul(2);
                self.park_period_ns = if doubled > self.max_park_period_ns {
                    self.max_park_period_ns
                } else {
                    doubled
                };
            }
        }
    }

    #[inline(always)]
    fn reset(&mut self) {
        self.spins = 0;
        self.yields = 0;
        self.park_period_ns = self.min_park_period_ns;
        self.state = BackoffState::NotIdle;
    }

    #[inline]
    fn alias(&self) -> Alias {
        Self::ALIAS
    }
}

impl fmt::Display for BackoffIdleStrategy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "BackoffIdleStrategy{{alias={}, maxSpins={}, maxYields={}, minParkPeriodNs={}, maxParkPeriodNs={}}}",
            Self::ALIAS,
            self.max_spins,
            self.max_yields,
            self.min_park_period_ns,
            self.max_park_period_ns
        )
    }
}

impl fmt::Debug for BackoffIdleStrategy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BackoffIdleStrategy")
            .field("max_spins", &self.max_spins)
            .field("max_yields", &self.max_yields)
            .field("min_park_period_ns", &self.min_park_period_ns)
            .field("max_park_period_ns", &self.max_park_period_ns)
            .field("state", &self.state)
            .field("spins", &self.spins)
            .field("yields", &self.yields)
            .field("park_period_ns", &self.park_period_ns)
            .finish()
    }
}

// ============================================================================
// ControllableIdleStrategy
// ============================================================================

use core::sync::atomic::{AtomicI32, Ordering};

/// Control modes for ControllableIdleStrategy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub enum IdleMode {
    /// Not controlled - defaults to Park.
    NotControlled = 0,
    /// No-op idle (busy loop without hint).
    Noop = 1,
    /// Busy spin with spin_loop hint.
    BusySpin = 2,
    /// Yield to scheduler.
    Yield = 3,
    /// Park/sleep for minimum period.
    Park = 4,
}

impl From<i32> for IdleMode {
    #[inline]
    fn from(v: i32) -> Self {
        match v {
            1 => Self::Noop,
            2 => Self::BusySpin,
            3 => Self::Yield,
            4 => Self::Park,
            _ => Self::NotControlled,
        }
    }
}

/// Controllable idle strategy - mode switchable at runtime.
///
/// Uses an atomic status indicator to switch between idle modes
/// without requiring synchronization.
///
/// # Modes
/// - `NotControlled` / `Park`: Sleep for 1µs
/// - `Noop`: Do nothing (pure busy wait)
/// - `BusySpin`: Spin with CPU hint
/// - `Yield`: Yield to scheduler
///
/// # Thread Safety
/// The status indicator can be modified from any thread.
/// Changes take effect on the next `idle()` call.
pub struct ControllableIdleStrategy<'a> {
    status_indicator: &'a AtomicI32,
}

impl<'a> ControllableIdleStrategy<'a> {
    /// Alias name.
    pub const ALIAS: Alias = Alias::from_static("controllable");

    /// Park period when in Park mode (1µs).
    const PARK_PERIOD_NS: u64 = 1_000;

    /// Create with status indicator reference.
    #[inline]
    pub const fn new(status_indicator: &'a AtomicI32) -> Self {
        Self { status_indicator }
    }

    /// Get current idle mode.
    #[inline]
    pub fn current_mode(&self) -> IdleMode {
        IdleMode::from(self.status_indicator.load(Ordering::Relaxed))
    }
}

impl<'a> IdleStrategy for ControllableIdleStrategy<'a> {
    #[inline(always)]
    fn idle(&mut self, work_count: i32) {
        if work_count > 0 {
            return;
        }
        self.idle_unconditional();
    }

    #[inline(always)]
    fn idle_unconditional(&mut self) {
        // Relaxed ordering - eventual consistency is fine for mode switching
        let status = self.status_indicator.load(Ordering::Relaxed);

        match IdleMode::from(status) {
            // Fast paths first
            IdleMode::BusySpin => {
                core::hint::spin_loop();
            }
            IdleMode::Noop => {
                // Do nothing
            }
            IdleMode::Yield => {
                std::thread::yield_now();
            }
            // Cold path: parking involves syscall
            IdleMode::NotControlled | IdleMode::Park => {
                std::thread::park_timeout(Duration::from_nanos(Self::PARK_PERIOD_NS));
            }
        }
    }

    #[inline(always)]
    fn reset(&mut self) {
        // No state to reset
    }

    #[inline(always)]
    fn alias(&self) -> Alias {
        Self::ALIAS
    }
}

impl<'a> fmt::Debug for ControllableIdleStrategy<'a> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ControllableIdleStrategy")
            .field("current_mode", &self.current_mode())
            .finish()
    }
}

impl<'a> fmt::Display for ControllableIdleStrategy<'a> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "ControllableIdleStrategy{{alias={}, mode={:?}}}",
            Self::ALIAS,
            self.current_mode()
        )
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_noop_strategy() {
        let mut strategy = NoOpIdleStrategy::new();
        strategy.idle(0);
        strategy.idle(1);
        strategy.idle_unconditional();
        strategy.reset();
        assert_eq!(strategy.alias().as_str(), "noop");
    }

    #[test]
    fn test_busy_spin_strategy() {
        let mut strategy = BusySpinIdleStrategy::new();
        strategy.idle(1); // Should return immediately
        strategy.idle(0); // Should spin once
        assert_eq!(strategy.alias().as_str(), "spin");
    }

    #[test]
    fn test_yielding_strategy() {
        let mut strategy = YieldingIdleStrategy::new();
        strategy.idle(1); // Should return immediately
        strategy.idle(0); // Should yield
        assert_eq!(strategy.alias().as_str(), "yield");
    }

    #[test]
    fn test_sleeping_strategy() {
        let strategy = SleepingIdleStrategy::new();
        assert_eq!(strategy.sleep_period_ns(), 1_000);
        assert_eq!(strategy.alias().as_str(), "sleep-ns");

        let custom = SleepingIdleStrategy::with_period_ns(5_000);
        assert_eq!(custom.sleep_period_ns(), 5_000);
    }

    #[test]
    fn test_sleeping_millis_strategy() {
        let strategy = SleepingMillisIdleStrategy::new();
        assert_eq!(strategy.sleep_period_ms(), 1);
        assert_eq!(strategy.alias().as_str(), "sleep-ms");
    }

    #[test]
    fn test_backoff_strategy_state_machine() {
        let mut strategy = BackoffIdleStrategy::with_params(2, 2, 1_000, 10_000);

        // Initial state
        assert_eq!(strategy.state, BackoffState::NotIdle);

        // First idle -> Spinning
        strategy.idle_unconditional();
        assert_eq!(strategy.state, BackoffState::Spinning);

        // Continue spinning
        strategy.idle_unconditional();
        strategy.idle_unconditional();

        // Should transition to Yielding after max_spins
        assert_eq!(strategy.state, BackoffState::Yielding);

        // Continue yielding
        strategy.idle_unconditional();
        strategy.idle_unconditional();
        strategy.idle_unconditional();

        // Should transition to Parking after max_yields
        assert_eq!(strategy.state, BackoffState::Parking);

        // Reset
        strategy.reset();
        assert_eq!(strategy.state, BackoffState::NotIdle);
        assert_eq!(strategy.spins, 0);
        assert_eq!(strategy.yields, 0);
    }

    #[test]
    fn test_backoff_exponential_backoff() {
        let mut strategy = BackoffIdleStrategy::with_params(0, 0, 1_000, 8_000);

        // Move to parking state
        strategy.idle_unconditional(); // NotIdle -> Spinning
        strategy.idle_unconditional(); // Spinning -> Yielding (max_spins=0)
        strategy.idle_unconditional(); // Yielding -> Parking (max_yields=0)

        assert_eq!(strategy.state, BackoffState::Parking);
        assert_eq!(strategy.park_period_ns, 1_000);

        // Each park should double the period
        strategy.idle_unconditional();
        assert_eq!(strategy.park_period_ns, 2_000);

        strategy.idle_unconditional();
        assert_eq!(strategy.park_period_ns, 4_000);

        strategy.idle_unconditional();
        assert_eq!(strategy.park_period_ns, 8_000);

        // Should cap at max
        strategy.idle_unconditional();
        assert_eq!(strategy.park_period_ns, 8_000);
    }

    #[test]
    fn test_backoff_reset_on_work() {
        let mut strategy = BackoffIdleStrategy::new();

        // Get into spinning state
        strategy.idle(0);
        strategy.idle(0);
        assert_eq!(strategy.state, BackoffState::Spinning);

        // Work done should reset
        strategy.idle(1);
        assert_eq!(strategy.state, BackoffState::NotIdle);
        assert_eq!(strategy.spins, 0);
    }

    #[test]
    fn test_controllable_strategy() {
        let indicator = AtomicI32::new(IdleMode::Noop as i32);
        let mut strategy = ControllableIdleStrategy::new(&indicator);

        assert_eq!(strategy.current_mode(), IdleMode::Noop);
        strategy.idle(0); // Should do nothing

        indicator.store(IdleMode::BusySpin as i32, Ordering::Relaxed);
        assert_eq!(strategy.current_mode(), IdleMode::BusySpin);

        indicator.store(IdleMode::Yield as i32, Ordering::Relaxed);
        assert_eq!(strategy.current_mode(), IdleMode::Yield);

        indicator.store(IdleMode::Park as i32, Ordering::Relaxed);
        assert_eq!(strategy.current_mode(), IdleMode::Park);

        assert_eq!(strategy.alias().as_str(), "controllable");
    }

    #[test]
    fn test_backoff_size() {
        // Verify cache line padding
        let size = std::mem::size_of::<BackoffIdleStrategy>();
        // Should be at least 128 bytes (pre-pad + data + post-pad)
        assert!(size >= 128, "BackoffIdleStrategy should be >= 128 bytes, got {}", size);
    }
}