//! Full integration tests.

use agent_rs::{
    Agent, AgentRunner, AgentInvoker, BackoffIdleStrategy, YieldingIdleStrategy,
    RoleName, WorkResult, AtomicErrorCounter, default_error_handler,
};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

#[test]
fn test_high_throughput_invoker() {
    struct ThroughputAgent {
        counter: u64,
    }

    impl Agent for ThroughputAgent {
        #[inline]
        fn do_work(&mut self) -> WorkResult {
            self.counter = self.counter.wrapping_add(1);
            Ok(1)
        }

        fn role_name(&self) -> RoleName {
            RoleName::from_static("throughput")
        }
    }

    let mut invoker = AgentInvoker::with_defaults(ThroughputAgent { counter: 0 });
    invoker.start();

    let iterations = 1_000_000u64;
    let start = Instant::now();

    for _ in 0..iterations {
        invoker.invoke();
    }

    let elapsed = start.elapsed();
    let rate = iterations as f64 / elapsed.as_secs_f64();

    println!("Throughput: {:.2}M ops/sec", rate / 1_000_000.0);

    // Should achieve at least 10M ops/sec on modern hardware
    assert!(rate > 1_000_000.0, "Rate too low: {}", rate);
}

#[test]
fn test_runner_with_shared_state() {
    let shared = Arc::new(AtomicU64::new(0));
    let shared_clone = Arc::clone(&shared);

    struct SharedAgent {
        shared: Arc<AtomicU64>,
        local: u64,
    }

    impl Agent for SharedAgent {
        fn do_work(&mut self) -> WorkResult {
            self.local += 1;
            self.shared.fetch_add(1, Ordering::Relaxed);

            if self.local >= 10_000 {
                Err(agent_rs::AgentTermination::new(
                    agent_rs::TerminationReason::Requested,
                ))
            } else {
                Ok(1)
            }
        }

        fn role_name(&self) -> RoleName {
            RoleName::from_static("shared")
        }
    }

    let runner = AgentRunner::with_defaults(
        SharedAgent {
            shared: shared_clone,
            local: 0,
        },
        YieldingIdleStrategy::new(),
    );

    let (handle, _) = runner.start();
    handle.join().unwrap();

    assert_eq!(shared.load(Ordering::Relaxed), 10_000);
}

#[test]
fn test_multiple_runners_concurrent() {
    const NUM_AGENTS: usize = 4;
    const WORK_PER_AGENT: u64 = 10_000;

    let total = Arc::new(AtomicU64::new(0));
    let error_counter = AtomicErrorCounter::new();

    struct ConcurrentAgent {
        id: usize,
        count: u64,
        max: u64,
        total: Arc<AtomicU64>,
    }

    impl Agent for ConcurrentAgent {
        fn do_work(&mut self) -> WorkResult {
            if self.count >= self.max {
                return Err(agent_rs::AgentTermination::new(
                    agent_rs::TerminationReason::Requested,
                ));
            }

            self.count += 1;
            self.total.fetch_add(1, Ordering::Relaxed);
            Ok(1)
        }

        fn role_name(&self) -> RoleName {
            match self.id {
                0 => RoleName::from_static("concurrent-0"),
                1 => RoleName::from_static("concurrent-1"),
                2 => RoleName::from_static("concurrent-2"),
                _ => RoleName::from_static("concurrent-N"),
            }
        }
    }

    let mut handles = Vec::new();

    for id in 0..NUM_AGENTS {
        let agent = ConcurrentAgent {
            id,
            count: 0,
            max: WORK_PER_AGENT,
            total: Arc::clone(&total),
        };

        let runner = AgentRunner::new(
            agent,
            BackoffIdleStrategy::new(),
            default_error_handler,
            Some(&error_counter),
        );

        let (handle, _) = runner.start();
        handles.push(handle);
    }

    for handle in handles {
        handle.join().unwrap();
    }

    let final_total = total.load(Ordering::Relaxed);
    assert_eq!(final_total, (NUM_AGENTS as u64) * WORK_PER_AGENT);
    assert_eq!(error_counter.get(), 0);
}

#[test]
fn test_idle_strategy_with_runner() {
    use agent_rs::idle_strategy::SleepingIdleStrategy;

    struct SlowAgent {
        count: u32,
    }

    impl Agent for SlowAgent {
        fn do_work(&mut self) -> WorkResult {
            self.count += 1;
            if self.count >= 100 {
                Err(agent_rs::AgentTermination::new(
                    agent_rs::TerminationReason::Requested,
                ))
            } else {
                // Return 0 to trigger idle
                Ok(0)
            }
        }

        fn role_name(&self) -> RoleName {
            RoleName::from_static("slow")
        }
    }

    // Use sleeping strategy
    let runner = AgentRunner::with_defaults(
        SlowAgent { count: 0 },
        SleepingIdleStrategy::with_period_ns(100), // 100ns sleep
    );

    let start = Instant::now();
    let (handle, _) = runner.start();
    handle.join().unwrap();
    let elapsed = start.elapsed();

    // Should complete relatively quickly even with sleeping
    assert!(elapsed < Duration::from_secs(1));
}