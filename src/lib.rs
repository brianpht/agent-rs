//! Agrona-style Agent framework for Rust.
//!
//! Deterministic, allocation-free agent lifecycle management.
//!
//! # Design Principles
//! - No allocation in hot path (`do_work`)
//! - Lock-free state management
//! - Single-writer principle
//! - Bounded latency
//!
//! # Example
//! ```ignore
//! use agent_rs::{Agent, AgentRunner, BackoffIdleStrategy, RoleName};
//!
//! struct MyAgent {
//!     counter: u64,
//! }
//!
//! impl Agent for MyAgent {
//!     fn do_work(&mut self) -> WorkResult {
//!         // Hot path - no allocation!
//!         self.counter = self.counter.wrapping_add(1);
//!         Ok(1)
//!     }
//!
//!     fn role_name(&self) -> RoleName {
//!         RoleName::from_static("my-agent")
//!     }
//! }
//!
//! let agent = MyAgent { counter: 0 };
//! let idle = BackoffIdleStrategy::default_backoff();
//! let (handle, runner) = AgentRunner::with_defaults(agent, idle).start();
//!
//! // ... later ...
//! runner.signal_stop();
//! handle.join().unwrap();
//! ```

#![warn(missing_docs)]
#![warn(clippy::all)]

pub mod agent;
pub mod agent_invoker;
pub mod agent_runner;
pub mod error;
pub mod error_handler;
pub mod idle_strategy;

// Re-exports
pub use agent::{Agent, RoleName, ROLE_NAME_MAX_LEN};
pub use agent_invoker::AgentInvoker;
pub use agent_runner::{AgentRunner, RunnerConfig, RunnerHandle, DEFAULT_CLOSE_TIMEOUT_MS};
pub use error::{AgentTermination, LifecycleResult, TerminationReason, WorkResult};
pub use error_handler::{AtomicErrorCounter, ErrorHandlerFn, default_error_handler};
pub use idle_strategy::{
    BackoffIdleStrategy, IdleStrategy, NoOpIdleStrategy, SleepingIdleStrategy, YieldingIdleStrategy,
};

