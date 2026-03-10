# AGENT-RS

Agrona-style Agent framework for Rust - deterministic, allocation-free agent lifecycle management.

Inspired by [Agrona](https://github.com/real-logic/agrona)'s Agent abstraction, providing a simple yet powerful model for building high-performance, low-latency concurrent applications.

## Features

- **Allocation-Free Hot Path**: Zero heap allocations during `do_work()` cycles
- **Lock-Free State Management**: Atomic state transitions without mutex contention
- **Progressive Backoff**: Spinning → Yielding → Parking idle strategy
- **Single-Writer Principle**: Thread-safe by design, no data races
- **Cache-Optimized**: 128-byte padding prevents false sharing
- **Pure Rust**: Minimal dependencies, no unsafe in public API
- **High Performance**: Sub-nanosecond invoke latency, 2+ Gelem/s throughput

## Installation

Add to your `Cargo.toml`:

```toml
[dependencies]
agent-rs = "0.1"
```

## Quick Start

```rust
use agent_rs::{Agent, AgentRunner, BackoffIdleStrategy, RoleName, WorkResult};

struct CounterAgent {
    counter: u64,
    max: u64,
}

impl Agent for CounterAgent {
    fn do_work(&mut self) -> WorkResult {
        if self.counter >= self.max {
            return Err(agent_rs::AgentTermination::new(
                agent_rs::TerminationReason::Requested,
            ));
        }
        
        self.counter = self.counter.wrapping_add(1);
        Ok(1) // 1 unit of work done
    }

    fn role_name(&self) -> RoleName {
        RoleName::from_static("counter-agent")
    }
}

fn main() {
    let agent = CounterAgent { counter: 0, max: 1_000_000 };
    let idle = BackoffIdleStrategy::new();
    
    let (handle, runner) = AgentRunner::with_defaults(agent, idle).start();
    
    // Agent runs on dedicated thread...
    handle.join().unwrap();
    
    println!("Agent completed!");
}
```

## How It Works

### Agent Lifecycle

```text
┌─────────────────────────────────────────────────────────┐
│                    Agent Lifecycle                       │
├─────────────────────────────────────────────────────────┤
│                                                         │
│   on_start() ──► [ do_work() ]* ──► on_close()         │
│       │              │                   │              │
│       │         (duty cycle)             │              │
│       │              │                   │              │
│    once at       repeated until       once at           │
│    startup       termination          shutdown          │
│                                                         │
└─────────────────────────────────────────────────────────┘
```

### Idle Strategy State Machine (BackoffIdleStrategy)

```text
┌──────────────────────────────────────────────────────────┐
│                                                          │
│   NOT_IDLE ──► SPINNING ──► YIELDING ──► PARKING        │
│       ▲           │            │            │            │
│       │           │            │            │            │
│       └───────────┴────────────┴────────────┘            │
│                    reset()                               │
│                                                          │
└──────────────────────────────────────────────────────────┘

Phase         │ Action              │ Latency │ CPU Usage
──────────────┼─────────────────────┼─────────┼───────────
SPINNING      │ spin_loop() hint    │ ~10ns   │ 100%
YIELDING      │ thread::yield_now() │ ~1µs    │ Low
PARKING       │ park_timeout()      │ ~1µs+   │ Minimal
```

## Execution Models

### AgentRunner - Dedicated Thread

Run an agent on its own thread with automatic lifecycle management:

```rust
use agent_rs::{Agent, AgentRunner, BackoffIdleStrategy, RoleName, WorkResult};

struct MyAgent { /* ... */ }
impl Agent for MyAgent { /* ... */ }

let agent = MyAgent { /* ... */ };
let idle = BackoffIdleStrategy::new();

// Start on dedicated thread
let (handle, runner) = AgentRunner::with_defaults(agent, idle).start();

// Signal stop from any thread
runner.signal_stop();

// Wait for completion
handle.join().unwrap();
```

### AgentInvoker - Manual Control

Invoke agent duty cycles manually for integration with existing event loops:

```rust
use agent_rs::{Agent, AgentInvoker, BackoffIdleStrategy, IdleStrategy};

let mut invoker = AgentInvoker::with_defaults(MyAgent::new());
let mut idle = BackoffIdleStrategy::new();

invoker.start();

while !invoker.is_closed() {
    let work_count = invoker.invoke();
    idle.idle(work_count as i32);
}
```

## Idle Strategies

| Strategy | Description | Use Case |
|----------|-------------|----------|
| `NoOpIdleStrategy` | Does nothing | External idle handling |
| `BusySpinIdleStrategy` | `spin_loop()` hint | Ultra-low latency |
| `YieldingIdleStrategy` | `thread::yield_now()` | Balanced CPU/latency |
| `SleepingIdleStrategy` | `park_timeout(ns)` | Power-efficient |
| `BackoffIdleStrategy` | Progressive phases | General purpose ✓ |
| `ControllableIdleStrategy` | Runtime switchable | Dynamic tuning |

### Custom Backoff Configuration

```rust
let idle = BackoffIdleStrategy::with_params(
    20,        // max_spins: spin iterations before yielding
    10,        // max_yields: yield iterations before parking  
    1_000,     // min_park_period_ns: initial park duration (1µs)
    1_000_000, // max_park_period_ns: maximum park duration (1ms)
);
```

## Error Handling

```rust
use agent_rs::{AgentRunner, AtomicErrorCounter, default_error_handler};

let error_counter = AtomicErrorCounter::new();

let runner = AgentRunner::new(
    agent,
    BackoffIdleStrategy::new(),
    default_error_handler,       // or custom: fn(AgentTermination)
    Some(&error_counter),        // optional error counting
);

// After running...
println!("Errors: {}", error_counter.get());
```

## Performance

Benchmarks on a modern x86_64 system:

| Operation | Time | Throughput |
|-----------|------|------------|
| `invoke()` single call | ~430 ps | 2.3 Gelem/s |
| `idle(work=1)` reset | ~220 ps | - |
| Single spin iteration | ~23 ns | - |
| 10 spin iterations | ~106 ns | - |
| Backoff `idle(work=1)` | ~700 ps | - |

## Performance Design

This library is built with deterministic, low-latency performance as a core design principle:

- **Allocation-free hot path**: Zero heap allocations during `do_work()` cycles
- **Lock-free state management**: Atomic state transitions, no mutex
- **Single-writer principle**: Each agent owned by one thread
- **Cache-line padding**: 128 bytes for `BackoffIdleStrategy` prevents false sharing
- **Branch-predictable code**: Fast path first, error handlers marked `#[cold]`
- **Inline optimization**: Hot functions marked `#[inline(always)]`
- **Wrapping arithmetic**: Safe sequence number handling

### Performance Targets

| Metric                | Target | Achieved |
|-----------------------|--------|----------|
| Invoke latency        | < 1 ns | ✅ ~430 ps |
| Hot-path allocations  | Zero   | ✅ Zero |
| Cache misses (steady) | None   | ✅ None |

## API Reference

### Core Traits

```rust
pub trait Agent {
    fn on_start(&mut self) -> LifecycleResult { Ok(()) }
    fn do_work(&mut self) -> WorkResult;  // MUST NOT allocate
    fn on_close(&mut self) -> LifecycleResult { Ok(()) }
    fn role_name(&self) -> RoleName;
}

pub trait IdleStrategy {
    fn idle(&mut self, work_count: i32);
    fn idle_unconditional(&mut self);
    fn reset(&mut self);
    fn alias(&self) -> Alias;
}
```

### Result Types

```rust
pub type WorkResult = Result<u32, AgentTermination>;
pub type LifecycleResult = Result<(), AgentTermination>;

pub enum TerminationReason {
    Requested,    // Normal termination
    Error,        // Error condition
    Interrupted,  // External interrupt
    Timeout,      // Timeout exceeded
}
```

## Examples

Run the examples:

```bash
# Simple counter agent
cargo run --example simple_agent

# Multiple concurrent agents
cargo run --example multi_agent

# Controllable idle strategy demo
cargo run --example controllable_idle
```

## Benchmarks

Run performance benchmarks:

```bash
# All benchmarks
cargo bench

# Specific benchmark
cargo bench --bench agent_throughput_bench
cargo bench --bench idle_strategy_bench
cargo bench --bench backoff_bench
```

## License

BSD 3-Clause License. See [LICENSE](LICENSE) for details.

## Credits

Inspired by:
- [Agrona](https://github.com/real-logic/agrona) - High-performance Java library by Real Logic
- [Aeron](https://github.com/real-logic/aeron) - Efficient reliable UDP unicast, multicast, and IPC transport

Related Rust libraries:
- [crossbeam](https://github.com/crossbeam-rs/crossbeam) - Concurrent programming tools
- [parking_lot](https://github.com/Amanieu/parking_lot) - Efficient synchronization primitives

