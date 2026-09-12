use std::cell::Cell;

use policy::{Capability, EffectiveCapabilities, ScopeId};
use runner::{enforce_process_operation, IsolationRequirements, RunnerError};
use runner_native::NativeRunner;

fn process_authority() -> EffectiveCapabilities {
    EffectiveCapabilities::new(vec![Capability::process("claude").unwrap()])
}

#[test]
fn trusted_host_runs_with_exact_process_authority() {
    let called = Cell::new(false);
    let result = enforce_process_operation(
        &NativeRunner,
        &process_authority(),
        "ws-1",
        ScopeId::new("claude").unwrap(),
        IsolationRequirements::trusted_host(),
        |_| called.set(true),
    );

    assert!(result.is_ok());
    assert!(called.get());
}

#[test]
fn required_isolation_never_downgrades_to_native_runner() {
    for requirements in [
        IsolationRequirements::new(true, false, false),
        IsolationRequirements::new(false, true, false),
        IsolationRequirements::new(false, false, true),
        IsolationRequirements::new(true, true, true),
    ] {
        let called = Cell::new(false);
        let result = enforce_process_operation(
            &NativeRunner,
            &process_authority(),
            "ws-1",
            ScopeId::new("claude").unwrap(),
            requirements,
            |_| called.set(true),
        );
        assert!(matches!(
            result,
            Err(RunnerError::InsufficientEnforcement { .. })
        ));
        assert!(!called.get());
    }
}

#[test]
fn native_runner_cannot_widen_process_authority() {
    let called = Cell::new(false);
    let result = enforce_process_operation(
        &NativeRunner,
        &process_authority(),
        "ws-1",
        ScopeId::new("bash").unwrap(),
        IsolationRequirements::trusted_host(),
        |_| called.set(true),
    );

    assert!(matches!(
        result,
        Err(RunnerError::ProcessNotAuthorized { .. })
    ));
    assert!(!called.get());
}
