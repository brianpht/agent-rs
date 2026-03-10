//! AgentRunner state machine tests.

use agent_rs::{
    Agent, AgentRunner, BackoffIdleStrategy, RoleName, WorkResult,
    AtomicErrorCounter,
};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

/// Agent that runs until stopped.
struct InfiniteAgent {
    running: Arc<AtomicBool>,
    iterations: AtomicU64,
}

impl InfiniteAgent {
    fn new(running: Arc<AtomicBool>) -> Self {
        Self {
            running,
            iterations: AtomicU64::new(0),
        }
    }
}

impl Agent for InfiniteAgent {
    fn do_work(&mut self) -> WorkResult {
        self.iterations.fetch_add(1, Ordering::Relaxed);

        // Return work to avoid idle sleeping
        if self.running.load(Ordering::Relaxed) {
            Ok(1)
        } else {
            Err(agent_rs::AgentTermination::new(
                agent_rs::TerminationReason::Requested,
            ))
        }
    }

    fn role_name(&self) -> RoleName {
        RoleName::from_static("infinite-agent")
    }
}

#[test]
fn test_runner_start_stop() {
    let running = Arc::new(AtomicBool::new(true));
    let agent = InfiniteAgent::new(Arc::clone(&running));
    let idle = BackoffIdleStrategy::new();

    let runner = AgentRunner::with_defaults(agent, idle);
    let (handle, runner_handle) = runner.start();

    // Let it run briefly
    std::thread::sleep(Duration::from_millis(50));

    // Stop it
    running.store(false, Ordering::Relaxed);
    runner_handle.signal_stop();

    // Wait for completion
    handle.join().expect("Thread panicked");

    assert!(runner_handle.is_closed());
}

#[test]
fn test_runner_external_stop() {
    struct NeverEndingAgent;

    impl Agent for NeverEndingAgent {
        fn do_work(&mut self) -> WorkResult {
            Ok(0) // Will idle
        }

        fn role_name(&self) -> RoleName {
            RoleName::from_static("never-ending")
        }
    }

    let runner = AgentRunner::with_defaults(NeverEndingAgent, BackoffIdleStrategy::new());
    let (handle, runner_handle) = runner.start();

    // Let it start
    std::thread::sleep(Duration::from_millis(10));

    // External stop
    runner_handle.signal_stop();

    // Should complete within reasonable time
    let result = handle.join();
    assert!(result.is_ok());
    assert!(runner_handle.is_closed());
}

#[test]
fn test_error_counting() {
    let error_counter = AtomicErrorCounter::new();

    struct ErrorAgent {
        error_count: u32,
    }

    impl Agent for ErrorAgent {
        fn do_work(&mut self) -> WorkResult {
            self.error_count += 1;
            if self.error_count >= 3 {
                Err(agent_rs::AgentTermination::new(
                    agent_rs::TerminationReason::Requested,
                ))
            } else {
                Err(agent_rs::AgentTermination::with_error(
                    agent_rs::TerminationReason::Error,
                    self.error_count,
                ))
            }
        }

        fn role_name(&self) -> RoleName {
            RoleName::from_static("error-agent")
        }
    }

    let runner = AgentRunner::new(
        ErrorAgent { error_count: 0 },
        BackoffIdleStrategy::new(),
        agent_rs::default_error_handler,
        Some(&error_counter),
    );

    let (handle, _runner_handle) = runner.start();
    handle.join().expect("Thread panicked");

    // Should have counted errors
    assert!(error_counter.get() >= 2);
}