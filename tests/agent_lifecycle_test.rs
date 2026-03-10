//! Agent lifecycle integration tests.

use agent_rs::{
    Agent, AgentTermination, LifecycleResult, RoleName, TerminationReason, WorkResult,
};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

/// Agent that tracks lifecycle calls.
struct LifecycleTracker {
    on_start_count: Arc<AtomicU32>,
    do_work_count: Arc<AtomicU32>,
    on_close_count: Arc<AtomicU32>,
    max_work: u32,
}

impl LifecycleTracker {
    fn new(
        on_start: Arc<AtomicU32>,
        do_work: Arc<AtomicU32>,
        on_close: Arc<AtomicU32>,
        max_work: u32,
    ) -> Self {
        Self {
            on_start_count: on_start,
            do_work_count: do_work,
            on_close_count: on_close,
            max_work,
        }
    }
}

impl Agent for LifecycleTracker {
    fn on_start(&mut self) -> LifecycleResult {
        self.on_start_count.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }

    fn do_work(&mut self) -> WorkResult {
        let count = self.do_work_count.fetch_add(1, Ordering::SeqCst);
        if count >= self.max_work {
            Err(AgentTermination::new(TerminationReason::Requested))
        } else {
            Ok(1)
        }
    }

    fn on_close(&mut self) -> LifecycleResult {
        self.on_close_count.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }

    fn role_name(&self) -> RoleName {
        RoleName::from_static("lifecycle-tracker")
    }
}

#[test]
fn test_lifecycle_order() {

    let on_start = Arc::new(AtomicU32::new(0));
    let do_work = Arc::new(AtomicU32::new(0));
    let on_close = Arc::new(AtomicU32::new(0));

    let agent = LifecycleTracker::new(
        Arc::clone(&on_start),
        Arc::clone(&do_work),
        Arc::clone(&on_close),
        5,
    );

    let mut invoker = agent_rs::AgentInvoker::with_defaults(agent);

    // Before start
    assert_eq!(on_start.load(Ordering::SeqCst), 0);
    assert!(!invoker.is_started());

    // Start
    invoker.start();
    assert_eq!(on_start.load(Ordering::SeqCst), 1);
    assert!(invoker.is_running());

    // Work
    while invoker.is_running() {
        invoker.invoke();
    }

    assert!(do_work.load(Ordering::SeqCst) >= 5);

    // Close happens automatically via Drop or explicit call
    invoker.close();
    assert_eq!(on_close.load(Ordering::SeqCst), 1);
    assert!(invoker.is_closed());
}

#[test]
fn test_on_start_failure() {
    struct FailingStartAgent;

    impl Agent for FailingStartAgent {
        fn on_start(&mut self) -> LifecycleResult {
            Err(AgentTermination::with_error(TerminationReason::Error, 42))
        }

        fn do_work(&mut self) -> WorkResult {
            panic!("should not be called");
        }

        fn role_name(&self) -> RoleName {
            RoleName::from_static("failing-start")
        }
    }

    let mut invoker = agent_rs::AgentInvoker::with_defaults(FailingStartAgent);
    invoker.start();

    // Agent should be closed after on_start failure
    assert!(invoker.is_closed());
    assert!(!invoker.is_running());
}

#[test]
fn test_idempotent_close() {
    let on_close = Arc::new(AtomicU32::new(0));

    struct CloseCounter(Arc<AtomicU32>);

    impl Agent for CloseCounter {
        fn do_work(&mut self) -> WorkResult {
            Ok(0)
        }

        fn on_close(&mut self) -> LifecycleResult {
            self.0.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }

        fn role_name(&self) -> RoleName {
            RoleName::from_static("close-counter")
        }
    }

    let mut invoker = agent_rs::AgentInvoker::with_defaults(CloseCounter(Arc::clone(&on_close)));
    invoker.start();

    // Multiple closes
    invoker.close();
    invoker.close();
    invoker.close();

    assert_eq!(on_close.load(Ordering::SeqCst), 1);
}