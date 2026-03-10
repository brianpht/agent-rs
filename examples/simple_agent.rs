//! Simple agent example demonstrating basic lifecycle.
//!
//! Run with: cargo run --example simple_agent

use agent_rs::{
    Agent, AgentRunner, BackoffIdleStrategy, RoleName, WorkResult,
    AtomicErrorCounter, default_error_handler,
};

/// A simple agent that counts iterations.
struct SimpleAgent {
    iterations: u64,
    max_iterations: u64,
}

impl SimpleAgent {
    fn new(max_iterations: u64) -> Self {
        Self {
            iterations: 0,
            max_iterations,
        }
    }
}

impl Agent for SimpleAgent {
    fn on_start(&mut self) -> agent_rs::LifecycleResult {
        println!("[{}] Starting...", self.role_name());
        Ok(())
    }

    fn do_work(&mut self) -> WorkResult {
        self.iterations = self.iterations.wrapping_add(1);

        if self.iterations >= self.max_iterations {
            println!("[{}] Completed {} iterations", self.role_name(), self.iterations);
            return Err(agent_rs::AgentTermination::new(
                agent_rs::TerminationReason::Requested,
            ));
        }

        // Simulate some work - return 1 if we did work
        if self.iterations % 1000 == 0 {
            Ok(1) // Did work
        } else {
            Ok(0) // No work (will trigger idle)
        }
    }

    fn on_close(&mut self) -> agent_rs::LifecycleResult {
        println!("[{}] Closing after {} iterations", self.role_name(), self.iterations);
        Ok(())
    }

    fn role_name(&self) -> RoleName {
        RoleName::from_static("simple-agent")
    }
}

fn main() {
    println!("=== Simple Agent Example ===\n");

    // Create error counter for monitoring
    let error_counter = AtomicErrorCounter::new();

    // Create agent and idle strategy
    let agent = SimpleAgent::new(10_000);
    let idle_strategy = BackoffIdleStrategy::new();

    // Create and start runner
    let runner = AgentRunner::new(
        agent,
        idle_strategy,
        default_error_handler,
        Some(&error_counter),
    );

    let (handle, runner_handle) = runner.start();

    println!("Agent started, waiting for completion...\n");

    // Wait for agent to finish
    handle.join().expect("Agent thread panicked");

    println!("\n=== Results ===");
    println!("Errors encountered: {}", error_counter.get());
    println!("Runner closed: {}", runner_handle.is_closed());
}