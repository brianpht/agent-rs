//! Agent invoker for manual duty cycle control.
//!
//! Not thread-safe. Use from single thread only.

use crate::{
    agent::Agent,
    error::{AgentTermination, TerminationReason},
    error_handler::{AtomicErrorCounter, ErrorHandlerFn, default_error_handler},
};

/// Agent state machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
enum InvokerState {
    Created = 0,
    Started = 1,
    Running = 2,
    Closed = 3,
}

/// Agent invoker - manual duty cycle invocation.
///
/// # Thread Safety
/// **NOT thread-safe**. All calls must be from the same thread.
///
/// # Example
/// ```ignore
/// let mut invoker = AgentInvoker::new(my_agent, error_handler, &counter);
/// invoker.start();
/// loop {
///     let work = invoker.invoke();
///     if invoker.is_closed() { break; }
///     idle_strategy.idle(work);
/// }
/// ```
pub struct AgentInvoker<'a, A: Agent> {
    agent: A,
    error_handler: ErrorHandlerFn,
    error_counter: Option<&'a AtomicErrorCounter>,
    state: InvokerState,
}

impl<'a, A: Agent> AgentInvoker<'a, A> {
    /// Create new invoker.
    #[inline]
    pub fn new(
        agent: A,
        error_handler: ErrorHandlerFn,
        error_counter: Option<&'a AtomicErrorCounter>,
    ) -> Self {
        Self {
            agent,
            error_handler,
            error_counter,
            state: InvokerState::Created,
        }
    }

    /// Create with default error handler.
    #[inline]
    pub fn with_defaults(agent: A) -> Self {
        Self::new(agent, default_error_handler, None)
    }

    /// Has the agent been started?
    #[inline]
    pub const fn is_started(&self) -> bool {
        !matches!(self.state, InvokerState::Created)
    }

    /// Is the agent running?
    #[inline]
    pub const fn is_running(&self) -> bool {
        matches!(self.state, InvokerState::Running)
    }

    /// Has the agent been closed?
    #[inline]
    pub const fn is_closed(&self) -> bool {
        matches!(self.state, InvokerState::Closed)
    }

    /// Get reference to contained agent.
    #[inline]
    pub fn agent(&self) -> &A {
        &self.agent
    }

    /// Get mutable reference to contained agent.
    #[inline]
    pub fn agent_mut(&mut self) -> &mut A {
        &mut self.agent
    }

    /// Start the agent (calls `on_start`).
    ///
    /// Idempotent - multiple calls have no effect.
    pub fn start(&mut self) {
        if self.state != InvokerState::Created {
            return;
        }

        self.state = InvokerState::Started;

        match self.agent.on_start() {
            Ok(()) => {
                self.state = InvokerState::Running;
            }
            Err(termination) => {
                self.handle_error(termination);
                self.close();
            }
        }
    }

    /// Invoke one duty cycle.
    ///
    /// # Returns
    /// Work count (0 if no work or not running).
    /// 
    /// # Hot Path
    /// This is the primary hot path - optimized for the Ok case.
    #[inline(always)]
    pub fn invoke(&mut self) -> u32 {
        if self.state != InvokerState::Running {
            return 0;
        }

        match self.agent.do_work() {
            Ok(work_count) => work_count,
            Err(termination) => {
                self.handle_invoke_error(termination);
                0
            }
        }
    }

    /// Close the agent (calls `on_close`).
    ///
    /// Idempotent - multiple calls have no effect.
    pub fn close(&mut self) {
        if self.state == InvokerState::Closed {
            return;
        }

        self.state = InvokerState::Closed;

        if let Err(termination) = self.agent.on_close() {
            self.handle_error(termination);
        }
    }

    #[cold]
    #[inline(never)]
    fn handle_invoke_error(&mut self, termination: AgentTermination) {
        self.state = InvokerState::Started; // Stop running
        self.handle_error(termination);

        if termination.reason() == TerminationReason::Requested {
            self.close();
        }
    }

    #[cold]
    #[inline(never)]
    fn handle_error(&self, termination: AgentTermination) {
        if let Some(counter) = self.error_counter {
            counter.increment();
        }
        (self.error_handler)(termination);
    }
}

impl<'a, A: Agent> Drop for AgentInvoker<'a, A> {
    fn drop(&mut self) {
        self.close();
    }
}