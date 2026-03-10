//! Counter agent with external stop signal.
//!
//! Demonstrates:
//! - External stop via RunnerHandle
//! - Error counting
//! - Multiple idle strategies
//!
//! Run with: cargo run --example counter_agent

use agent_rs::{
    Agent, AgentRunner, RoleName, WorkResult,
    YieldingIdleStrategy, AtomicErrorCounter,
};
use std::time::{Duration, Instant};

/// Agent that counts as fast as possible.
struct CounterAgent {
    count: u64,
    report_interval: u64,
    last_report: Instant,
}

impl CounterAgent {
    fn new(report_interval: u64) -> Self {
        Self {
            count: 0,
            report_interval,
            last_report: Instant::now(),
        }
    }
}

impl Agent for CounterAgent {
    fn on_start(&mut self) -> agent_rs::LifecycleResult {
        self.last_report = Instant::now();
        println!("Counter agent started");
        Ok(())
    }

    fn do_work(&mut self) -> WorkResult {
        self.count = self.count.wrapping_add(1);

        // Report periodically
        if self.count % self.report_interval == 0 {
            let elapsed = self.last_report.elapsed();
            let rate = self.report_interval as f64 / elapsed.as_secs_f64();
            println!(
                "Count: {}, Rate: {:.2}M ops/sec",
                self.count,
                rate / 1_000_000.0
            );
            self.last_report = Instant::now();
        }

        // Always return 1 (work done) to avoid idling
        Ok(1)
    }

    fn on_close(&mut self) -> agent_rs::LifecycleResult {
        println!("Final count: {}", self.count);
        Ok(())
    }

    fn role_name(&self) -> RoleName {
        RoleName::from_static("counter-agent")
    }
}

fn main() {
    println!("=== Counter Agent Example ===\n");
    println!("Will run for 3 seconds...\n");

    let error_counter = AtomicErrorCounter::new();

    // Use yielding strategy for this high-throughput workload
    let agent = CounterAgent::new(10_000_000);
    let idle_strategy = YieldingIdleStrategy::new();

    let runner = AgentRunner::new(
        agent,
        idle_strategy,
        agent_rs::default_error_handler,
        Some(&error_counter),
    );

    let (handle, runner_handle) = runner.start();

    // Let it run for 3 seconds
    std::thread::sleep(Duration::from_secs(3));

    // Signal stop
    println!("\nSignaling stop...");
    runner_handle.signal_stop();

    // Wait for clean shutdown
    handle.join().expect("Thread panicked");

    println!("\nErrors: {}", error_counter.get());
}