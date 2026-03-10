//! Detailed backoff strategy benchmarks.
//!
//! Run with: cargo bench --bench backoff_bench

use std::hint::black_box;
use criterion::{criterion_group, criterion_main, BatchSize, BenchmarkId, Criterion};
use agent_rs::idle_strategy::{BackoffIdleStrategy, IdleStrategy};
use std::time::{Duration, Instant};

fn bench_state_transitions(c: &mut Criterion) {
    let mut group = c.benchmark_group("state_transitions");

    // Measure individual state transition costs
    group.bench_function("not_idle_to_spinning", |b| {
        b.iter_batched(
            || BackoffIdleStrategy::new(),
            |mut s| {
                s.idle_unconditional();
                black_box(s)
            },
            BatchSize::SmallInput,
        )
    });

    // Alternative measurement using iter_custom for comparison
    group.bench_function("not_idle_to_spinning_custom", |b| {
        b.iter_custom(|iters| {
            let mut total = Duration::ZERO;
            for _ in 0..iters {
                let mut s = BackoffIdleStrategy::new();
                let start = Instant::now();
                s.idle_unconditional();
                total += start.elapsed();
                black_box(&s);
            }
            total
        })
    });

    group.bench_function("spinning_phase_100", |b| {
        b.iter_batched(
            || BackoffIdleStrategy::with_params(100, 0, 1_000_000, 1_000_000),
            |mut s| {
                // Stay in spinning phase - 100 iterations
                for _ in 0..100 {
                    s.idle_unconditional();
                }
                black_box(s)
            },
            BatchSize::SmallInput,
        )
    });

    // Measure single spin cost - use iter_batched to ensure fresh state each time
    group.bench_function("single_spin", |b| {
        b.iter_batched(
            || {
                // Large max_spins and max_park to ensure we stay in spinning phase
                let mut s = BackoffIdleStrategy::with_params(1_000_000, 0, 1_000_000_000, 1_000_000_000);
                // Pre-transition to spinning state
                s.idle_unconditional();
                s
            },
            |mut s| {
                s.idle_unconditional();
                black_box(s)
            },
            BatchSize::SmallInput,
        )
    });

    // Measure N spins to get more accurate per-spin cost
    group.bench_function("spinning_phase_10", |b| {
        b.iter_batched(
            || BackoffIdleStrategy::with_params(100, 0, 1_000_000_000, 1_000_000_000),
            |mut s| {
                for _ in 0..10 {
                    s.idle_unconditional();
                }
                black_box(s)
            },
            BatchSize::SmallInput,
        )
    });

    // Measure pure spin_loop hint cost (no state machine overhead) as baseline
    group.bench_function("raw_spin_hint", |b| {
        b.iter(|| {
            std::hint::spin_loop();
            black_box(())
        })
    });

    group.finish();
}

fn bench_full_backoff_cycle(c: &mut Criterion) {
    let mut group = c.benchmark_group("full_cycle");
    group.sample_size(20); // Parking involves actual sleeping

    // Different configurations
    let configs = [
        ("aggressive", 5u64, 3u64, 100u64, 1000u64),
        ("balanced", 10, 5, 1000, 100_000),
        ("conservative", 20, 10, 10_000, 1_000_000),
    ];

    for (name, max_spins, max_yields, min_park, max_park) in configs {
        group.bench_with_input(
            BenchmarkId::new("config", name),
            &(max_spins, max_yields, min_park, max_park),
            |b, &(ms, my, minp, maxp)| {
                // Calculate iterations needed to reach max park period
                let park_doublings = if maxp > minp {
                    (64 - (maxp / minp).leading_zeros()) as u64
                } else {
                    1
                };
                let total_iters = ms + my + park_doublings + 2;

                b.iter_batched(
                    || BackoffIdleStrategy::with_params(ms, my, minp, maxp),
                    |mut s| {
                        // Go through all phases
                        for _ in 0..total_iters {
                            s.idle_unconditional();
                        }
                        black_box(s)
                    },
                    BatchSize::SmallInput,
                )
            },
        );
    }

    group.finish();
}

fn bench_reset_after_work(c: &mut Criterion) {
    let mut group = c.benchmark_group("reset_after_work");

    // Simulate realistic work pattern: work, idle, work, idle...
    group.bench_function("alternating_work_idle", |b| {
        let mut s = BackoffIdleStrategy::new();
        let mut work = true;
        b.iter(|| {
            if work {
                s.idle(black_box(1)); // Work done - resets
            } else {
                s.idle(black_box(0)); // No work - idles
            }
            work = !work;
            black_box(&s);
        })
    });

    // Burst pattern: many idle, then work (stays within spinning phase)
    group.bench_function("burst_pattern_spinning", |b| {
        b.iter_batched(
            // Use params that keep us in spinning phase for 10 idles
            || BackoffIdleStrategy::with_params(20, 10, 1_000_000, 1_000_000),
            |mut s| {
                // 10 idle calls - stays in spinning
                for _ in 0..10 {
                    s.idle(black_box(0));
                }
                // Then work
                s.idle(black_box(1));
                black_box(s)
            },
            BatchSize::SmallInput,
        )
    });

    // Different burst sizes - all staying within spinning/yielding phase
    for burst_size in [5, 10, 15, 20] {
        group.bench_with_input(
            BenchmarkId::new("burst_spinning", burst_size),
            &burst_size,
            |b, &size| {
                b.iter_batched(
                    // Ensure we stay in spinning phase
                    || BackoffIdleStrategy::with_params(100, 0, 1_000_000_000, 1_000_000_000),
                    |mut s| {
                        for _ in 0..size {
                            s.idle(black_box(0));
                        }
                        s.idle(black_box(1));
                        black_box(s)
                    },
                    BatchSize::SmallInput,
                )
            },
        );
    }

    // Burst with parking - measure actual parking cost
    group.sample_size(20);
    for burst_size in [20, 30, 50] {
        group.bench_with_input(
            BenchmarkId::new("burst_with_parking", burst_size),
            &burst_size,
            |b, &size| {
                b.iter_batched(
                    // Small params to quickly enter parking
                    || BackoffIdleStrategy::with_params(5, 5, 1000, 10_000),
                    |mut s| {
                        for _ in 0..size {
                            s.idle(black_box(0));
                        }
                        s.idle(black_box(1));
                        black_box(s)
                    },
                    BatchSize::SmallInput,
                )
            },
        );
    }

    group.finish();
}

fn bench_work_patterns(c: &mut Criterion) {
    let mut group = c.benchmark_group("work_patterns");

    // Steady work - always has work
    group.bench_function("steady_work", |b| {
        let mut s = BackoffIdleStrategy::new();
        b.iter(|| {
            s.idle(black_box(1));
            black_box(&s);
        })
    });

    // Sporadic work - 1 in 10 has work (stays in spinning phase with right params)
    group.bench_function("sporadic_1_in_10_spinning", |b| {
        // With max_spins=20, we reset before leaving spinning phase
        let mut s = BackoffIdleStrategy::with_params(20, 10, 1_000_000, 1_000_000);
        let mut counter = 0u32;
        b.iter(|| {
            counter = counter.wrapping_add(1);
            let work_count = if counter % 10 == 0 { 1 } else { 0 };
            s.idle(black_box(work_count));
            black_box(&s);
        })
    });

    // Sporadic work with default params (will hit yielding)
    group.bench_function("sporadic_1_in_10_default", |b| {
        let mut s = BackoffIdleStrategy::new();
        let mut counter = 0u32;
        b.iter(|| {
            counter = counter.wrapping_add(1);
            let work_count = if counter % 10 == 0 { 1 } else { 0 };
            s.idle(black_box(work_count));
            black_box(&s);
        })
    });

    // Rare work - 1 in 100 (will definitely hit parking with default params)
    group.sample_size(20);
    group.bench_function("rare_1_in_100_default", |b| {
        let mut s = BackoffIdleStrategy::new();
        let mut counter = 0u32;
        b.iter(|| {
            counter = counter.wrapping_add(1);
            let work_count = if counter % 100 == 0 { 1 } else { 0 };
            s.idle(black_box(work_count));
            black_box(&s);
        })
    });

    // Rare work with aggressive params (stays in spinning longer)
    group.bench_function("rare_1_in_100_aggressive", |b| {
        let mut s = BackoffIdleStrategy::with_params(50, 50, 1_000_000, 1_000_000);
        let mut counter = 0u32;
        b.iter(|| {
            counter = counter.wrapping_add(1);
            let work_count = if counter % 100 == 0 { 1 } else { 0 };
            s.idle(black_box(work_count));
            black_box(&s);
        })
    });

    group.finish();
}

fn bench_phase_transitions(c: &mut Criterion) {
    let mut group = c.benchmark_group("phase_transitions");

    // Cost to transition from spinning to yielding
    group.bench_function("spinning_to_yielding", |b| {
        b.iter_batched(
            || {
                let mut s = BackoffIdleStrategy::with_params(5, 10, 1_000_000, 1_000_000);
                // Get to end of spinning phase
                for _ in 0..5 {
                    s.idle_unconditional();
                }
                s
            },
            |mut s| {
                // This call should transition to yielding
                s.idle_unconditional();
                black_box(s)
            },
            BatchSize::SmallInput,
        )
    });

    // Cost to transition from yielding to parking
    group.bench_function("yielding_to_parking", |b| {
        b.iter_batched(
            || {
                let mut s = BackoffIdleStrategy::with_params(5, 5, 100, 1000);
                // Get to end of yielding phase
                for _ in 0..10 {
                    s.idle_unconditional();
                }
                s
            },
            |mut s| {
                // This call should transition to parking
                s.idle_unconditional();
                black_box(s)
            },
            BatchSize::SmallInput,
        )
    });

    // Cost of single yield
    group.bench_function("single_yield", |b| {
        b.iter_batched(
            || {
                let mut s = BackoffIdleStrategy::with_params(1, 1000, 1_000_000_000, 1_000_000_000);
                // Transition past spinning into yielding
                s.idle_unconditional(); // spin
                s.idle_unconditional(); // first yield
                s
            },
            |mut s| {
                s.idle_unconditional();
                black_box(s)
            },
            BatchSize::SmallInput,
        )
    });

    // Cost of single park (minimum sleep)
    group.sample_size(50);
    group.bench_function("single_park_min", |b| {
        b.iter_batched(
            || {
                let mut s = BackoffIdleStrategy::with_params(1, 1, 1000, 1000); // 1µs park
                // Transition to parking
                s.idle_unconditional(); // spin
                s.idle_unconditional(); // yield
                s
            },
            |mut s| {
                s.idle_unconditional(); // park
                black_box(s)
            },
            BatchSize::SmallInput,
        )
    });

    group.finish();
}

fn bench_spinning_cost(c: &mut Criterion) {
    let mut group = c.benchmark_group("spinning_cost");

    // Compare different numbers of spins to derive per-spin cost
    for num_spins in [1, 5, 10, 20, 50, 100] {
        group.bench_with_input(
            BenchmarkId::new("spins", num_spins),
            &num_spins,
            |b, &n| {
                b.iter_batched(
                    || BackoffIdleStrategy::with_params(1000, 0, 1_000_000_000, 1_000_000_000),
                    |mut s| {
                        for _ in 0..n {
                            s.idle_unconditional();
                        }
                        black_box(s)
                    },
                    BatchSize::SmallInput,
                )
            },
        );
    }

    group.finish();
}

criterion_group!(
    benches,
    bench_state_transitions,
    bench_full_backoff_cycle,
    bench_reset_after_work,
    bench_work_patterns,
    bench_phase_transitions,
    bench_spinning_cost,
);

criterion_main!(benches);