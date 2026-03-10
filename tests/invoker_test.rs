//! AgentInvoker unit tests.

use agent_rs::{
    Agent, AgentInvoker, AgentTermination, LifecycleResult, RoleName,
    TerminationReason, WorkResult, AtomicErrorCounter,
};

#[test]
fn test_invoker_not_started() {
    struct SimpleAgent;

    impl Agent for SimpleAgent {
        fn do_work(&mut self) -> WorkResult {
            Ok(1)
        }

        fn role_name(&self) -> RoleName {
            RoleName::from_static("simple")
        }
    }

    let mut invoker = AgentInvoker::with_defaults(SimpleAgent);

    // Not started - invoke should return 0
    assert!(!invoker.is_started());
    assert_eq!(invoker.invoke(), 0);
}

#[test]
fn test_invoker_work_counting() {
    struct CountingAgent {
        count: u32,
    }

    impl Agent for CountingAgent {
        fn do_work(&mut self) -> WorkResult {
            self.count += 1;
            Ok(self.count)
        }

        fn role_name(&self) -> RoleName {
            RoleName::from_static("counting")
        }
    }

    let mut invoker = AgentInvoker::with_defaults(CountingAgent { count: 0 });
    invoker.start();

    assert_eq!(invoker.invoke(), 1);
    assert_eq!(invoker.invoke(), 2);
    assert_eq!(invoker.invoke(), 3);

    assert_eq!(invoker.agent().count, 3);
}

#[test]
fn test_invoker_error_handling() {
    let error_counter = AtomicErrorCounter::new();

    struct ErrorAgent {
        calls: u32,
    }

    impl Agent for ErrorAgent {
        fn do_work(&mut self) -> WorkResult {
            self.calls += 1;
            if self.calls >= 2 {
                Err(AgentTermination::with_error(TerminationReason::Error, 123))
            } else {
                Ok(1)
            }
        }

        fn role_name(&self) -> RoleName {
            RoleName::from_static("error")
        }
    }

    fn custom_handler(_: AgentTermination) {
        // Custom handling
    }

    let mut invoker = AgentInvoker::new(
        ErrorAgent { calls: 0 },
        custom_handler,
        Some(&error_counter),
    );

    invoker.start();

    assert_eq!(invoker.invoke(), 1); // First call OK
    assert_eq!(invoker.invoke(), 0); // Second call errors

    // Error counter should be incremented
    assert_eq!(error_counter.get(), 1);

    // Agent should still be "started" but not "running"
    assert!(invoker.is_started());
}

#[test]
fn test_invoker_agent_access() {
    struct StatefulAgent {
        value: i32,
    }

    impl Agent for StatefulAgent {
        fn do_work(&mut self) -> WorkResult {
            self.value += 10;
            Ok(1)
        }

        fn role_name(&self) -> RoleName {
            RoleName::from_static("stateful")
        }
    }

    let mut invoker = AgentInvoker::with_defaults(StatefulAgent { value: 5 });

    // Access before start
    assert_eq!(invoker.agent().value, 5);

    invoker.start();
    invoker.invoke();

    // Access after work
    assert_eq!(invoker.agent().value, 15);

    // Mutable access
    invoker.agent_mut().value = 100;
    assert_eq!(invoker.agent().value, 100);
}

#[test]
fn test_invoker_drop_calls_close() {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    let closed = Arc::new(AtomicBool::new(false));
    let closed_clone = Arc::clone(&closed);

    struct DropTracker(Arc<AtomicBool>);

    impl Agent for DropTracker {
        fn do_work(&mut self) -> WorkResult {
            Ok(0)
        }

        fn on_close(&mut self) -> LifecycleResult {
            self.0.store(true, Ordering::SeqCst);
            Ok(())
        }

        fn role_name(&self) -> RoleName {
            RoleName::from_static("drop-tracker")
        }
    }

    {
        let mut invoker = AgentInvoker::with_defaults(DropTracker(closed_clone));
        invoker.start();
        // Invoker dropped here
    }

    assert!(closed.load(Ordering::SeqCst));
}