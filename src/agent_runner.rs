//! Agent runner for dedicated thread execution.
//!
//! Lock-free state management using atomics.

use crate::{
    agent::Agent,
    error::{AgentTermination, TerminationReason},
    error_handler::{AtomicErrorCounter, ErrorHandlerFn, default_error_handler},
    idle_strategy::IdleStrategy,
};
use core::sync::atomic::{AtomicU8, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

/// Wrapper to make raw pointer Send-safe.
/// SAFETY: The AtomicErrorCounter is only accessed atomically.
#[derive(Clone, Copy)]
struct SendPtr(*const AtomicErrorCounter);

// SAFETY: AtomicErrorCounter uses only atomic operations, safe across threads.
unsafe impl Send for SendPtr {}

/// Runner state (atomic).
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunnerState {
    /// Runner created but not started.
    Created = 0,
    /// Runner starting (initializing).
    Starting = 1,
    /// Runner actively running work loop.
    Running = 2,
    /// Runner stopping (cleanup in progress).
    Stopping = 3,
    /// Runner fully closed.
    Closed = 4,
}

impl From<u8> for RunnerState {
    #[inline]
    fn from(v: u8) -> Self {
        match v {
            0 => Self::Created,
            1 => Self::Starting,
            2 => Self::Running,
            3 => Self::Stopping,
            4 => Self::Closed,
            _ => Self::Closed,
        }
    }
}

/// Default close retry timeout.
pub const DEFAULT_CLOSE_TIMEOUT_MS: u64 = 5000;

/// Configuration for agent runner.
#[derive(Debug, Clone)]
pub struct RunnerConfig {
    /// Timeout for close retry in milliseconds.
    pub close_timeout_ms: u64,
}

impl Default for RunnerConfig {
    fn default() -> Self {
        Self {
            close_timeout_ms: DEFAULT_CLOSE_TIMEOUT_MS,
        }
    }
}

/// Agent runner - executes agent on dedicated thread.
///
/// # Thread Safety
/// - `close()` can be called from any thread
/// - Agent methods called from runner thread only
///
/// # Example
/// ```ignore
/// let runner = AgentRunner::new(agent, idle_strategy, error_handler, &counter);
/// let handle = runner.start();
/// // ... later ...
/// runner.close();
/// handle.join().unwrap();
/// ```
pub struct AgentRunner<A: Agent, I: IdleStrategy> {
    state: Arc<AtomicU8>,
    config: RunnerConfig,
    // These are moved into the thread, using Option for take()
    agent: Option<A>,
    idle_strategy: Option<I>,
    error_handler: ErrorHandlerFn,
    error_counter: Option<*const AtomicErrorCounter>,
}

// SAFETY: Agent and IdleStrategy are only accessed from runner thread.
// AtomicU8 state is the only cross-thread communication.
unsafe impl<A: Agent + Send, I: IdleStrategy + Send> Send for AgentRunner<A, I> {}
unsafe impl<A: Agent, I: IdleStrategy> Sync for AgentRunner<A, I> {}

impl<A: Agent, I: IdleStrategy> AgentRunner<A, I> {
    /// Create new runner.
    pub fn new(
        agent: A,
        idle_strategy: I,
        error_handler: ErrorHandlerFn,
        error_counter: Option<&AtomicErrorCounter>,
    ) -> Self {
        Self {
            state: Arc::new(AtomicU8::new(RunnerState::Created as u8)),
            config: RunnerConfig::default(),
            agent: Some(agent),
            idle_strategy: Some(idle_strategy),
            error_handler,
            error_counter: error_counter.map(|c| c as *const _),
        }
    }

    /// Create with configuration.
    pub fn with_config(
        agent: A,
        idle_strategy: I,
        error_handler: ErrorHandlerFn,
        error_counter: Option<&AtomicErrorCounter>,
        config: RunnerConfig,
    ) -> Self {
        Self {
            state: Arc::new(AtomicU8::new(RunnerState::Created as u8)),
            config,
            agent: Some(agent),
            idle_strategy: Some(idle_strategy),
            error_handler,
            error_counter: error_counter.map(|c| c as *const _),
        }
    }

    /// Create with defaults.
    pub fn with_defaults(agent: A, idle_strategy: I) -> Self {
        Self::new(agent, idle_strategy, default_error_handler, None)
    }

    /// Get current state.
    #[inline]
    pub fn state(&self) -> RunnerState {
        RunnerState::from(self.state.load(Ordering::Acquire))
    }

    /// Is the runner closed?
    #[inline]
    pub fn is_closed(&self) -> bool {
        self.state() == RunnerState::Closed
    }

    /// Is the runner running?
    #[inline]
    pub fn is_running(&self) -> bool {
        self.state() == RunnerState::Running
    }

    /// Signal the runner to stop.
    ///
    /// This is non-blocking. Use with `JoinHandle::join()` to wait.
    #[inline]
    pub fn signal_stop(&self) {
        // CAS from Running to Stopping
        let _ = self.state.compare_exchange(
            RunnerState::Running as u8,
            RunnerState::Stopping as u8,
            Ordering::Release,
            Ordering::Relaxed,
        );
    }
}

impl<A: Agent + Send + 'static, I: IdleStrategy + Send + 'static> AgentRunner<A, I> {
    /// Start the runner on a new thread.
    ///
    /// # Returns
    /// Tuple of (JoinHandle, shared state reference for signaling stop)
    pub fn start(mut self) -> (JoinHandle<()>, RunnerHandle) {
        // Clone Arc for thread and handle
        let state_for_thread = Arc::clone(&self.state);
        let state_for_handle = Arc::clone(&self.state);

        // Take ownership of agent and idle strategy
        let mut agent = self.agent.take().expect("agent already taken");
        let mut idle_strategy = self.idle_strategy.take().expect("idle strategy already taken");
        let error_handler = self.error_handler;
        // Wrap in SendPtr to make it Send-safe
        let error_counter = self.error_counter.map(SendPtr);

        // Transition to Starting
        self.state.store(RunnerState::Starting as u8, Ordering::Release);

        let role_name = agent.role_name();

        let handle = thread::Builder::new()
            .name(role_name.as_str().to_string())
            .spawn(move || {
                // Run the agent
                run_agent_loop(
                    &mut agent,
                    &mut idle_strategy,
                    &state_for_thread,
                    error_handler,
                    // Unwrap SendPtr back to raw pointer
                    error_counter.map(|p| p.0),
                );
            })
            .expect("failed to spawn thread");

        let runner_handle = RunnerHandle {
            state: state_for_handle,
            close_timeout_ms: self.config.close_timeout_ms,
        };

        (handle, runner_handle)
    }
}

/// Handle for controlling a running agent.
pub struct RunnerHandle {
    state: Arc<AtomicU8>,
    close_timeout_ms: u64,
}

// Arc<AtomicU8> is already Send + Sync
impl RunnerHandle {
    /// Get current state.
    #[inline]
    pub fn state(&self) -> RunnerState {
        RunnerState::from(self.state.load(Ordering::Acquire))
    }

    /// Is the runner closed?
    #[inline]
    pub fn is_closed(&self) -> bool {
        self.state() == RunnerState::Closed
    }

    /// Signal stop.
    #[inline]
    pub fn signal_stop(&self) {
        let _ = self.state.compare_exchange(
            RunnerState::Running as u8,
            RunnerState::Stopping as u8,
            Ordering::Release,
            Ordering::Relaxed,
        );
    }

    /// Close and wait for thread to finish.
    pub fn close(&self, handle: JoinHandle<()>) {
        self.signal_stop();

        // Wait with timeout and retry
        let timeout = Duration::from_millis(self.close_timeout_ms);

        loop {
            if self.is_closed() {
                let _ = handle.join();
                return;
            }

            // Park with timeout
            thread::park_timeout(timeout);

            if self.is_closed() {
                let _ = handle.join();
                return;
            }

            // Still running after timeout - log warning
            eprintln!("Agent close timeout, retrying...");
        }
    }
}

/// Internal: Run the agent loop.
fn run_agent_loop<A: Agent, I: IdleStrategy>(
    agent: &mut A,
    idle_strategy: &mut I,
    state: &AtomicU8,
    error_handler: ErrorHandlerFn,
    error_counter: Option<*const AtomicErrorCounter>,
) {
    // on_start
    if let Err(termination) = agent.on_start() {
        handle_error(termination, error_handler, error_counter, state);
        state.store(RunnerState::Closed as u8, Ordering::Release);
        let _ = agent.on_close();
        return;
    }

    // Transition to Running
    state.store(RunnerState::Running as u8, Ordering::Release);

    // Work loop
    work_loop(agent, idle_strategy, state, error_handler, error_counter);

    // on_close
    state.store(RunnerState::Stopping as u8, Ordering::Release);
    if let Err(termination) = agent.on_close() {
        handle_error(termination, error_handler, error_counter, state);
    }

    state.store(RunnerState::Closed as u8, Ordering::Release);
}

/// Hot path work loop - optimized for minimal overhead.
///
/// # Performance
/// - Single atomic load per iteration (Acquire for state check)
/// - Running state value cached to avoid repeated casts
/// - Error path is cold and separated
#[inline(always)]
fn work_loop<A: Agent, I: IdleStrategy>(
    agent: &mut A,
    idle_strategy: &mut I,
    state: &AtomicU8,
    error_handler: ErrorHandlerFn,
    error_counter: Option<*const AtomicErrorCounter>,
) {
    // Cache the running state value to avoid repeated casts
    let running = RunnerState::Running as u8;

    loop {
        // Single atomic load with Acquire ordering
        if state.load(Ordering::Acquire) != running {
            break;
        }
        do_work(agent, idle_strategy, state, error_handler, error_counter);
    }
}

/// Execute one work cycle.
///
/// # Hot Path
/// The Ok path is the hot path - inlined and optimized.
/// Error path is marked cold and separated for branch prediction.
#[inline(always)]
fn do_work<A: Agent, I: IdleStrategy>(
    agent: &mut A,
    idle_strategy: &mut I,
    state: &AtomicU8,
    error_handler: ErrorHandlerFn,
    error_counter: Option<*const AtomicErrorCounter>,
) {
    match agent.do_work() {
        Ok(work_count) => {
            // Hot path: just idle based on work count
            idle_strategy.idle(work_count as i32);
        }
        Err(termination) => {
            // Cold path: handle termination
            handle_termination(termination, state, error_handler, error_counter);
        }
    }
}

/// Handle agent termination (cold path).
///
/// Separated from hot path for better branch prediction.
#[cold]
#[inline(never)]
fn handle_termination(
    termination: AgentTermination,
    state: &AtomicU8,
    error_handler: ErrorHandlerFn,
    error_counter: Option<*const AtomicErrorCounter>,
) {
    handle_error(termination, error_handler, error_counter, state);

    // AgentTermination with Requested reason stops the loop
    if termination.reason() == TerminationReason::Requested {
        state.store(RunnerState::Stopping as u8, Ordering::Release);
    }
}

#[cold]
#[inline(never)]
fn handle_error(
    termination: AgentTermination,
    error_handler: ErrorHandlerFn,
    error_counter: Option<*const AtomicErrorCounter>,
    state: &AtomicU8,
) {
    // Only count actual errors, not normal termination requests
    if termination.reason() != TerminationReason::Requested {
        if let Some(counter_ptr) = error_counter {
            let running = state.load(Ordering::Acquire) == RunnerState::Running as u8;
            if running {
                // SAFETY: Pointer valid for lifetime of runner
                unsafe { &*counter_ptr }.increment();
            }
        }
    }

    error_handler(termination);
}