//! Multiple agents running concurrently.
//!
//! Run with: cargo run --example multi_agent

use agent_rs::{
    Agent, AgentRunner, BackoffIdleStrategy, RoleName, WorkResult,
    AtomicErrorCounter, default_error_handler,
};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

/// Shared counter across agents.
struct SharedCounter {
    value: AtomicU64,
}

impl SharedCounter {
    fn new() -> Self {
        Self {
            value: AtomicU64::new(0),
        }
    }

    fn increment(&self) -> u64 {
        self.value.fetch_add(1, Ordering::Relaxed)
    }

    fn get(&self) -> u64 {
        self.value.load(Ordering::Relaxed)
    }
}

/// Worker agent that increments shared counter.
struct WorkerAgent {
    id: u32,
    local_count: u64,
    shared: Arc<SharedCounter>,
    role: RoleName,
}

impl WorkerAgent {
    fn new(id: u32, shared: Arc<SharedCounter>) -> Self {
        // Build role name at construction time
        let role = match id {
            0 => RoleName::from_static("worker-0"),
            1 => RoleName::from_static("worker-1"),
            2 => RoleName::from_static("worker-2"),
            3 => RoleName::from_static("worker-3"),
            _ => RoleName::from_static("worker-N"),
        };

        Self {
            id,
            local_count: 0,
            shared,
            role,
        }
    }
}

impl Agent for WorkerAgent {
    fn on_start(&mut self) -> agent_rs::LifecycleResult {
        println!("Worker {} started", self.id);
        Ok(())
    }

    fn do_work(&mut self) -> WorkResult {
        // Increment shared counter
        self.shared.increment();
        self.local_count = self.local_count.wrapping_add(1);

        // Simulate variable workload
        if self.local_count % 1000 == 0 {
            Ok(1)
        } else {
            Ok(0)
        }
    }

    fn on_close(&mut self) -> agent_rs::LifecycleResult {
        println!("Worker {} closed, local count: {}", self.id, self.local_count);
        Ok(())
    }

    fn role_name(&self) -> RoleName {
        self.role
    }
}

fn main() {
    println!("=== Multi-Agent Example ===\n");

    const NUM_WORKERS: u32 = 4;
    let shared_counter = Arc::new(SharedCounter::new());
    let error_counter = AtomicErrorCounter::new();

    // Start multiple workers
    let mut handles = Vec::with_capacity(NUM_WORKERS as usize);
    let mut runner_handles = Vec::with_capacity(NUM_WORKERS as usize);

    for id in 0..NUM_WORKERS {
        let agent = WorkerAgent::new(id, Arc::clone(&shared_counter));
        let idle_strategy = BackoffIdleStrategy::new();

        let runner = AgentRunner::new(
            agent,
            idle_strategy,
            default_error_handler,
            Some(&error_counter),
        );

        let (handle, runner_handle) = runner.start();
        handles.push(handle);
        runner_handles.push(runner_handle);
    }

    println!("All {} workers started\n", NUM_WORKERS);

    // Let them run
    std::thread::sleep(Duration::from_secs(2));

    // Report progress
    println!("Shared counter: {}\n", shared_counter.get());

    // Stop all workers
    println!("Stopping all workers...");
    for runner_handle in &runner_handles {
        runner_handle.signal_stop();
    }

    // Wait for all to finish
    for handle in handles {
        handle.join().expect("Worker panicked");
    }

    println!("\n=== Final Results ===");
    println!("Total shared count: {}", shared_counter.get());
    println!("Errors: {}", error_counter.get());
}