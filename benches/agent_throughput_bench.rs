//! Agent throughput benchmarks.
//!
//! Run with: cargo bench --bench agent_throughput_bench

use std::hint::black_box;
use criterion::{criterion_group, criterion_main, BatchSize, Criterion, Throughput};
use agent_rs::{
    Agent, AgentInvoker, RoleName, WorkResult,
    idle_strategy::{IdleStrategy, NoOpIdleStrategy, BackoffIdleStrategy},
};

/// Minimal agent for benchmarking.
struct BenchAgent {
    counter: u64,
}

impl BenchAgent {
    fn new() -> Self {
        Self { counter: 0 }
    }
}

impl Agent for BenchAgent {
    #[inline]
    fn do_work(&mut self) -> WorkResult {
        self.counter = self.counter.wrapping_add(1);
        Ok(1)
    }

    fn role_name(&self) -> RoleName {
        RoleName::from_static("bench-agent")
    }
}

fn bench_invoker_throughput(c: &mut Criterion) {
    let mut group = c.benchmark_group("invoker_throughput");

    // Single invocation - prevent optimization by observing both result and state
    group.throughput(Throughput::Elements(1));
    group.bench_function("invoke_single", |b| {
        let mut invoker = AgentInvoker::with_defaults(BenchAgent::new());
        invoker.start();
        b.iter(|| {
            let result = invoker.invoke();
            // Force both result and internal state to be observed
            black_box((result, invoker.agent().counter));
        })
    });

    // Batch invocation with correct throughput
    group.throughput(Throughput::Elements(1000));
    group.bench_function("invoke_batch_1000", |b| {
        let mut invoker = AgentInvoker::with_defaults(BenchAgent::new());
        invoker.start();
        b.iter(|| {
            let mut sum = 0u32;
            for _ in 0..1000 {
                sum = sum.wrapping_add(invoker.invoke());
            }
            black_box((sum, invoker.agent().counter));
        })
    });

    group.finish();
}

fn bench_agent_with_idle(c: &mut Criterion) {
    let mut group = c.benchmark_group("agent_with_idle");

    group.bench_function("noop_idle", |b| {
        let mut agent = BenchAgent::new();
        let mut idle = NoOpIdleStrategy::new();
        b.iter(|| {
            let work = agent.do_work().unwrap();
            idle.idle(work as i32);
            // Observe both work result and agent state
            black_box((work, agent.counter));
        })
    });

    group.bench_function("backoff_idle_with_work", |b| {
        let mut agent = BenchAgent::new();
        let mut idle = BackoffIdleStrategy::new();
        b.iter(|| {
            let work = agent.do_work().unwrap();
            idle.idle(work as i32);
            // Observe both work result and agent state
            black_box((work, agent.counter));
        })
    });

    group.finish();
}

fn bench_lifecycle(c: &mut Criterion) {
    let mut group = c.benchmark_group("lifecycle");

    // Use iter_batched to measure actual lifecycle cost
    group.bench_function("invoker_create_start_close", |b| {
        b.iter_batched(
            || BenchAgent::new(),
            |agent| {
                let mut invoker = AgentInvoker::with_defaults(agent);
                invoker.start();
                let result = invoker.invoke();
                invoker.close();
                black_box((result, invoker.agent().counter))
            },
            BatchSize::SmallInput,
        )
    });

    // Separate measurements for each phase
    group.bench_function("invoker_create_only", |b| {
        b.iter_batched(
            || BenchAgent::new(),
            |agent| {
                let invoker = AgentInvoker::with_defaults(agent);
                black_box(invoker)
            },
            BatchSize::SmallInput,
        )
    });

    group.bench_function("invoker_start_only", |b| {
        b.iter_batched(
            || AgentInvoker::with_defaults(BenchAgent::new()),
            |mut invoker| {
                invoker.start();
                black_box(invoker)
            },
            BatchSize::SmallInput,
        )
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_invoker_throughput,
    bench_agent_with_idle,
    bench_lifecycle,
);

criterion_main!(benches);