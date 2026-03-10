//! Demonstrates ControllableIdleStrategy with runtime mode switching.
//!
//! Run with: cargo run --example controllable_idle

use agent_rs::{
    Agent, RoleName, WorkResult,
    idle_strategy::{ControllableIdleStrategy, IdleMode, IdleStrategy},
};
use std::sync::atomic::{AtomicI32, Ordering};
use std::time::{Duration, Instant};

/// Agent that tracks idle time.
struct TimingAgent {
    work_count: u64,
    idle_count: u64,
}

impl TimingAgent {
    fn new() -> Self {
        Self {
            work_count: 0,
            idle_count: 0,
        }
    }
}

impl Agent for TimingAgent {
    fn do_work(&mut self) -> WorkResult {
        self.work_count = self.work_count.wrapping_add(1);

        // Simulate sporadic work
        if self.work_count % 100 == 0 {
            Ok(1)
        } else {
            self.idle_count = self.idle_count.wrapping_add(1);
            Ok(0)
        }
    }

    fn role_name(&self) -> RoleName {
        RoleName::from_static("timing-agent")
    }
}

fn benchmark_mode(mode: IdleMode, indicator: &AtomicI32, iterations: u64) -> Duration {
    indicator.store(mode as i32, Ordering::Relaxed);

    let mut agent = TimingAgent::new();
    let mut strategy = ControllableIdleStrategy::new(indicator);

    let start = Instant::now();

    for _ in 0..iterations {
        let work = match agent.do_work() {
            Ok(w) => w as i32,
            Err(_) => break,
        };
        strategy.idle(work);
    }

    start.elapsed()
}

fn main() {
    println!("=== Controllable Idle Strategy Example ===\n");

    let indicator = AtomicI32::new(IdleMode::Park as i32);
    let iterations = 100_000u64;

    println!("Benchmarking {} iterations per mode:\n", iterations);

    // Test each mode
    let modes = [
        (IdleMode::Noop, "Noop (busy wait)"),
        (IdleMode::BusySpin, "BusySpin (with hint)"),
        (IdleMode::Yield, "Yield"),
        (IdleMode::Park, "Park (1µs sleep)"),
    ];

    for (mode, name) in modes {
        let elapsed = benchmark_mode(mode, &indicator, iterations);
        let rate = iterations as f64 / elapsed.as_secs_f64();
        println!(
            "{:25} {:>10.2}ms  ({:.2}M ops/sec)",
            name,
            elapsed.as_secs_f64() * 1000.0,
            rate / 1_000_000.0
        );
    }

    println!("\n=== Dynamic Mode Switching Demo ===\n");

    // Demonstrate runtime switching
    let mut agent = TimingAgent::new();
    let mut strategy = ControllableIdleStrategy::new(&indicator);

    println!("Starting with Park mode...");
    indicator.store(IdleMode::Park as i32, Ordering::Relaxed);

    for i in 0..10 {
        let work = agent.do_work().unwrap_or(0) as i32;
        strategy.idle(work);

        if i == 4 {
            println!("Switching to BusySpin mode...");
            indicator.store(IdleMode::BusySpin as i32, Ordering::Relaxed);
        }
    }

    println!("\nCurrent mode: {:?}", strategy.current_mode());
}