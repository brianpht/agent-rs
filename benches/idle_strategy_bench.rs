//! Benchmarks for idle strategies.
//!
//! Run with: cargo bench --bench idle_strategy_bench

use std::hint::black_box;
use criterion::{criterion_group, criterion_main, BatchSize, BenchmarkId, Criterion};
use agent_rs::idle_strategy::{
    BackoffIdleStrategy, BusySpinIdleStrategy, IdleStrategy, NoOpIdleStrategy,
    SleepingIdleStrategy, YieldingIdleStrategy,
};

fn bench_idle_with_work(c: &mut Criterion) {
    let mut group = c.benchmark_group("idle_with_work");

    // When work_count > 0, all strategies should return immediately
    // Use black_box on strategy reference to prevent optimization
    group.bench_function("noop", |b| {
        let mut s = NoOpIdleStrategy::new();
        b.iter(|| {
            s.idle(black_box(1));
            black_box(&s);
        })
    });

    group.bench_function("busy_spin", |b| {
        let mut s = BusySpinIdleStrategy::new();
        b.iter(|| {
            s.idle(black_box(1));
            black_box(&s);
        })
    });

    group.bench_function("yielding", |b| {
        let mut s = YieldingIdleStrategy::new();
        b.iter(|| {
            s.idle(black_box(1));
            black_box(&s);
        })
    });

    group.bench_function("sleeping_ns", |b| {
        let mut s = SleepingIdleStrategy::new();
        b.iter(|| {
            s.idle(black_box(1));
            black_box(&s);
        })
    });

    group.bench_function("backoff", |b| {
        let mut s = BackoffIdleStrategy::new();
        b.iter(|| {
            s.idle(black_box(1));
            black_box(&s);
        })
    });

    group.finish();
}

fn bench_idle_no_work(c: &mut Criterion) {
    let mut group = c.benchmark_group("idle_no_work");

    // Measure actual idle behavior
    group.sample_size(50); // Fewer samples for sleeping strategies

    group.bench_function("noop", |b| {
        let mut s = NoOpIdleStrategy::new();
        b.iter(|| {
            s.idle(black_box(0));
            black_box(&s);
        })
    });

    group.bench_function("busy_spin", |b| {
        let mut s = BusySpinIdleStrategy::new();
        b.iter(|| {
            s.idle(black_box(0));
            black_box(&s);
        })
    });

    group.bench_function("yielding", |b| {
        let mut s = YieldingIdleStrategy::new();
        b.iter(|| {
            s.idle(black_box(0));
            black_box(&s);
        })
    });

    // Add sleeping benchmark with small sleep time
    group.bench_function("sleeping_1us", |b| {
        let mut s = SleepingIdleStrategy::with_duration(std::time::Duration::from_micros(1));
        b.iter(|| {
            s.idle(black_box(0));
            black_box(&s);
        })
    });

    group.finish();
}

fn bench_backoff_progression(c: &mut Criterion) {
    let mut group = c.benchmark_group("backoff_progression");

    // Measure cost of state transitions with iter_batched for accurate setup
    for spins in [0u64, 5, 10, 20] {
        group.bench_with_input(
            BenchmarkId::new("max_spins", spins),
            &spins,
            |b, &max_spins| {
                b.iter_batched(
                    || BackoffIdleStrategy::with_params(max_spins, 0, 1000, 1000),
                    |mut s| {
                        // Progress through spinning phase
                        for _ in 0..=max_spins {
                            s.idle_unconditional();
                        }
                        s.reset();
                        black_box(s)
                    },
                    BatchSize::SmallInput,
                )
            },
        );
    }

    group.finish();
}

fn bench_reset_cost(c: &mut Criterion) {
    let mut group = c.benchmark_group("reset_cost");

    // Reset from NOT_IDLE state (minimal cost)
    group.bench_function("reset_from_not_idle", |b| {
        b.iter_batched(
            || BackoffIdleStrategy::new(),
            |mut s| {
                s.reset();
                black_box(s)
            },
            BatchSize::SmallInput,
        )
    });

    // Reset from SPINNING state
    group.bench_function("reset_from_spinning", |b| {
        b.iter_batched(
            || {
                let mut s = BackoffIdleStrategy::new();
                for _ in 0..5 {
                    s.idle(0);
                }
                s
            },
            |mut s| {
                s.reset();
                black_box(s)
            },
            BatchSize::SmallInput,
        )
    });

    // Reset from YIELDING state
    group.bench_function("reset_from_yielding", |b| {
        b.iter_batched(
            || {
                let mut s = BackoffIdleStrategy::with_params(5, 10, 1000, 100_000);
                // 5 spins + some yields
                for _ in 0..8 {
                    s.idle(0);
                }
                s
            },
            |mut s| {
                s.reset();
                black_box(s)
            },
            BatchSize::SmallInput,
        )
    });

    // Reset from PARKING state (most expensive setup)
    group.sample_size(20); // Parking involves actual sleeping
    group.bench_function("reset_from_parking", |b| {
        b.iter_batched(
            || {
                let mut s = BackoffIdleStrategy::with_params(5, 5, 100, 1000);
                // 5 spins + 5 yields + into parking
                for _ in 0..12 {
                    s.idle(0);
                }
                s
            },
            |mut s| {
                s.reset();
                black_box(s)
            },
            BatchSize::SmallInput,
        )
    });

    group.finish();
}

fn bench_state_check_overhead(c: &mut Criterion) {
    let mut group = c.benchmark_group("state_check_overhead");

    // Compare idle(1) vs idle(0) to measure early-return overhead
    group.bench_function("backoff_idle_work_1", |b| {
        let mut s = BackoffIdleStrategy::new();
        b.iter(|| {
            s.idle(black_box(1));
            black_box(&s);
        })
    });

    group.bench_function("backoff_idle_work_0_single", |b| {
        b.iter_batched(
            || BackoffIdleStrategy::new(),
            |mut s| {
                s.idle(black_box(0));
                black_box(s)
            },
            BatchSize::SmallInput,
        )
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_idle_with_work,
    bench_idle_no_work,
    bench_backoff_progression,
    bench_reset_cost,
    bench_state_check_overhead,
);

criterion_main!(benches);