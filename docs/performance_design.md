# Performance Design

> **High-performance, low-latency agent framework implementation in Rust.**

---

## Table of Contents

- [Project Overview](#project-overview)
- [Governance Model](#governance-model)
- [Deployment Assumptions](#deployment-assumptions)
- [Performance Targets](#performance-targets)
- [Core Design Principles](#core-design-principles)
  - [1. Determinism First](#1-determinism-first)
  - [2. Single-Writer Principle](#2-single-writer-principle)
  - [3. Allocation-Free Hot Path](#3-allocation-free-hot-path)
  - [4. Cache-Oriented Design](#4-cache-oriented-design)
  - [5. Branch Predictability](#5-branch-predictability)
  - [6. Lock-Free State Management](#6-lock-free-state-management)
  - [7. Progressive Backoff Strategy](#7-progressive-backoff-strategy)
  - [8. Ring Buffer Discipline](#8-ring-buffer-discipline)
  - [9. Fixed-Size Strings](#9-fixed-size-strings)
  - [10. Unsafe Policy](#10-unsafe-policy)
  - [11. Performance Budget](#11-performance-budget)
- [Final Principle](#final-principle)

---

## Project Overview

### Target Domains

| Domain                           | Use Case                              |
|----------------------------------|---------------------------------------|
| HFT (High-Frequency Trading)     | Ultra-low latency event processing    |
| Real-time systems                | Deterministic task scheduling         |
| Message-driven architectures     | High-throughput agent pipelines       |
| Embedded/Resource-constrained    | Allocation-free steady state          |

### Inspiration

| Source                             | Contribution                          |
|------------------------------------|---------------------------------------|
| Agrona Agent framework             | Core agent lifecycle model            |
| Aeron transport design             | Lock-free, zero-copy principles       |
| Mechanical Sympathy philosophy     | Hardware-aware optimization           |
| LMAX Disruptor                     | Single-writer, cache-friendly design  |

---

## Governance Model

This project defines **two layers** of performance governance:

| Layer             | Document                           | Purpose                       |
|-------------------|-----------------------------------|-------------------------------|
| **Architecture**  | `performance_design.md`            | Defines intent and reasoning  |
| **Enforcement**   | `.github/copilot-instructions.md`  | Enforces non-negotiable rules |

### Conflict Resolution

```
If enforcement rules conflict with architecture
    → Architecture must be updated first

Benchmarks are the final authority.
```

---

## Deployment Assumptions

| Assumption            | Value                                              |
|-----------------------|---------------------------------------------------|
| Primary target        | x86_64                                            |
| Threading model       | Dedicated thread per agent                        |
| Memory model          | Single-writer, multiple-reader                    |
| Priority              | Deterministic latency > maximum throughput        |

> ⚠️ **Note**: Agent `do_work()` is always called from the same thread. Cross-thread communication uses atomics only for state transitions.

---

## Performance Targets

| Metric                   | Target       | Current      | Status |
|--------------------------|--------------|--------------|--------|
| Agent duty cycle         | < 200 ns     | ~0.5 ns      | ✅      |
| Invoker throughput       | > 1 Gelem/s  | 2.2 Gelem/s  | ✅      |
| Idle strategy overhead   | < 10 ns      | ~1-5 ns      | ✅      |
| Backoff reset            | < 5 ns       | ~1 ns        | ✅      |
| State transition         | < 50 ns      | ~10-30 ns    | ✅      |

### Regression Policy

- **> 10% regression** → requires justification
- **Tail latency** matters more than average latency
- **Latency variance** is a correctness concern

---

## Core Design Principles

### 1. Determinism First

```
Correctness > Determinism > Latency > Throughput
```

> ⚠️ **Unbounded memory or nondeterministic latency is a correctness failure.**

#### Must Be Deterministic Under

| Condition              | Required |
|------------------------|----------|
| High work rate         | ✅        |
| Zero work (idle)       | ✅        |
| Burst patterns         | ✅        |
| Extended idle periods  | ✅        |

**No randomness in agent lifecycle.**

---

### 2. Single-Writer Principle

| Principle                          | Rationale                        |
|------------------------------------|----------------------------------|
| One thread owns agent state        | No synchronization needed        |
| Atomic state for cross-thread      | Only for runner control          |
| No shared mutable data in hot path | Eliminates contention            |

#### Thread Ownership Model

```rust
// ✅ Correct: Single-writer
impl AgentInvoker {
    // All methods called from same thread
    fn invoke(&mut self) -> u32 { ... }
}

// ✅ Correct: Cross-thread via atomics only
impl AgentRunner {
    fn close(&self) {  // &self, not &mut self
        self.state.store(Stopping, Ordering::Release);
    }
}
```

---

### 3. Allocation-Free Hot Path

#### No Heap Allocation During

| Operation              | Allocation Allowed? |
|------------------------|---------------------|
| `do_work()`            | ❌                   |
| `idle()`               | ❌                   |
| `invoke()`             | ❌                   |
| `reset()`              | ❌                   |
| State transitions      | ❌                   |

- All buffers **preallocated at initialization**
- Fixed-size strings (`RoleName`, `Alias`)
- **Reuse everything**

---

### 4. Cache-Oriented Design

#### CPU Memory Latency Reference

| Level | Latency  |
|-------|----------|
| L1    | ~1 ns    |
| L2    | ~3 ns    |
| L3    | ~10 ns   |
| RAM   | ~100 ns  |

#### Rules

| Rule                                  | Priority |
|---------------------------------------|----------|
| Cache line padding (128 bytes)        | Required |
| Hot fields first in struct            | Required |
| Avoid pointer chasing                 | Required |
| Separate hot/cold data                | Required |
| Hot structs ≤ 64 bytes                | Required |

#### Cache Line Padding Example

```rust
#[repr(C)]
pub struct BackoffIdleStrategy {
    // Pre-padding (64 bytes)
    _pre_pad: [u8; 64],

    // Hot data - fits in one cache line
    state: BackoffState,
    spins: u64,
    yields: u64,
    park_period_ns: u64,

    // Post-padding (64 bytes)
    _post_pad: [u8; 64],
}
```

---

### 5. Branch Predictability

**Mispredict penalty**: ~15–20 cycles (~5–7 ns)

| Rule                             | Priority       |
|----------------------------------|----------------|
| Fast path first                  | Required       |
| Error paths marked `#[cold]`     | Required       |
| Match arms by frequency          | Required       |
| Avoid data-dependent divergence  | Required       |

#### Example: Backoff State Machine

```rust
#[inline(always)]
fn idle_unconditional(&mut self) {
    match self.state {
        // Fast path: spinning is most common
        BackoffState::Spinning => { ... }
        
        // Initial transition
        BackoffState::NotIdle => { ... }
        
        // Moderate idle
        BackoffState::Yielding => { ... }
        
        // Cold path: involves syscall
        BackoffState::Parking => { ... }
    }
}
```

---

### 6. Lock-Free State Management

**Default**: Atomic state transitions for cross-thread control.

#### Atomic Ordering

| Ordering   | Use Case                   |
|------------|----------------------------|
| `Relaxed`  | Counters                   |
| `Release`  | Publish state change       |
| `Acquire`  | Read state                 |
| `SeqCst`   | **Avoid in hot path**      |

#### State Transition Model

```rust
// AgentRunner state machine
pub enum RunnerState {
    Created  = 0,
    Starting = 1,
    Running  = 2,
    Stopping = 3,
    Closed   = 4,
}

// Cross-thread: use atomics
fn close(&self) {
    self.state.store(Stopping as u8, Ordering::Release);
}

fn is_running(&self) -> bool {
    self.state.load(Ordering::Acquire) == Running as u8
}
```

---

### 7. Progressive Backoff Strategy

#### Phase Progression

```text
NOT_IDLE → SPINNING → YIELDING → PARKING
    ↑__________|__________|_________|
              reset()
```

#### Phase Characteristics

| Phase    | Action              | Latency | CPU Usage |
|----------|---------------------|---------|-----------|
| Spinning | `spin_loop()`       | ~1 ns   | 100%      |
| Yielding | `yield_now()`       | ~100 ns | Low       |
| Parking  | `park_timeout()`    | ~1 µs+  | Minimal   |

#### Exponential Backoff in Parking

```rust
// Park period doubles each iteration
park_period_ns = min(park_period_ns * 2, max_park_period_ns);
```

| Configuration          | Default    |
|------------------------|------------|
| `max_spins`            | 10         |
| `max_yields`           | 5          |
| `min_park_period_ns`   | 1,000 (1µs)|
| `max_park_period_ns`   | 1,000,000 (1ms)|

---

### 8. Ring Buffer Discipline

#### Capacity

- **MUST** be power-of-two

#### Indexing

```rust
// ✅ Correct
index = seq & (capacity - 1)

// ❌ Forbidden
index = seq % capacity
```

| Rule                              | Status   |
|-----------------------------------|----------|
| Never use `%` in hot path         | Required |
| Power-of-two capacities only      | Required |

---

### 9. Fixed-Size Strings

No heap allocation for names and identifiers.

#### RoleName (64 bytes)

```rust
#[repr(C)]
pub struct RoleName {
    buf: [u8; 64],
    len: u8,
}

impl RoleName {
    pub const fn from_static(s: &'static str) -> Self { ... }
}
```

#### Alias (16 bytes)

```rust
#[repr(C)]
pub struct Alias {
    buf: [u8; 16],
    len: u8,
}

impl Alias {
    pub const fn from_static(s: &'static str) -> Self { ... }
}
```

| Type       | Size     | Use Case                    |
|------------|----------|-----------------------------|
| `RoleName` | 64 bytes | Agent identification        |
| `Alias`    | 16 bytes | Idle strategy identification|

---

### 10. Unsafe Policy

#### Allowed Only If

| Condition                 | Required |
|---------------------------|----------|
| Measurable gain proven    | ✅        |
| Benchmarked before/after  | ✅        |
| Invariants documented     | ✅        |
| Fuzz-tested               | ✅        |

> ❌ **Unsafe without justification → reject.**

#### Current Unsafe Usage

| Location             | Justification                          |
|----------------------|----------------------------------------|
| `RoleName::as_str()` | UTF-8 invariant maintained by API      |
| `Alias::as_str()`    | UTF-8 invariant maintained by API      |
| `SendPtr` wrapper    | Atomic access only, documented safety  |

---

### 11. Performance Budget

#### Agent Duty Cycle

| Metric                   | Target       |
|--------------------------|--------------|
| Allocation               | **None**     |
| Latency                  | **< 200 ns** |
| Steady-state cache miss  | **None**     |

#### Investigation Trigger

```
p99 > p50 × 2 → investigate
```

---

## Final Principle

| Layer         | Role               |
|---------------|-------------------|
| Architecture  | Defines intent     |
| Enforcement   | Ensures invariants |
| Benchmarks    | Validates reality  |

```
Architecture defines intent.
Enforcement ensures invariants.
Benchmarks validate reality.
```

