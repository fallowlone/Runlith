use std::cell::Cell;
use std::ffi::OsString;
use std::process::Stdio;
use std::sync::Arc;

use policy::{Capability, EffectiveCapabilities, ScopeId};
use runner::{
    enforce_process_operation, prepare_process_launch, ChildEnvironment, CommandSpec,
    IsolationRequirements, RunnerError,
};
use runner_native::NativeRunner;

fn child_environment() -> ChildEnvironment {
    ChildEnvironment::from_explicit([
        (OsString::from("PATH"), OsString::from("/usr/bin:/bin")),
        (OsString::from("HOME"), OsString::from("/home/tester")),
    ])
    .unwrap()
}

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
        child_environment(),
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
            child_environment(),
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
        child_environment(),
        |_| called.set(true),
    );

    assert!(matches!(
        result,
        Err(RunnerError::ProcessNotAuthorized { .. })
    ));
    assert!(!called.get());
}

/// TASK-SEC-R1 R1-H2 (adversarial, real child): ambient credentials must not
/// reach a governed child. A poisoned parent environment is observed by a real
/// `/usr/bin/env` child, which must see only the plan's allowlist.
#[cfg(unix)]
#[test]
fn r1_h2_child_env_cannot_see_poisoned_vars() {
    // Only a non-allowlisted name is poisoned, so allowlist resolution in
    // concurrently running tests is unaffected.
    const CANARY: &str = "RUNLITH_SEC_CANARY_AWS_SECRET_ACCESS_KEY";
    std::env::set_var(CANARY, "poison-value");
    assert_eq!(std::env::var(CANARY).unwrap(), "poison-value");

    let environment = ChildEnvironment::from_explicit([
        (OsString::from("PATH"), OsString::from("/usr/bin:/bin")),
        (OsString::from("HOME"), OsString::from("/home/tester")),
    ])
    .unwrap();
    let env_authority =
        EffectiveCapabilities::new(vec![Capability::process("/usr/bin/env").unwrap()]);
    let launch = prepare_process_launch(
        Arc::new(NativeRunner),
        &env_authority,
        "ws-1",
        ScopeId::new("/usr/bin/env").unwrap(),
        IsolationRequirements::trusted_host(),
        environment,
    )
    .expect("trusted-host plan is verifiable");

    let sandboxed = launch
        .bind(CommandSpec::new(
            OsString::from("/usr/bin/env"),
            std::iter::empty(),
        ))
        .expect("bind sandboxed recipe");
    let child = sandboxed
        .spawn(|wire| {
            wire.stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped());
        })
        .expect("spawn sandboxed child");
    let output = child.wait_with_output().expect("run /usr/bin/env child");
    assert!(output.status.success());

    let observed = String::from_utf8_lossy(&output.stdout);
    let names: Vec<&str> = observed
        .lines()
        .map(|line| line.split('=').next().unwrap_or_default())
        .collect();
    assert!(
        !names.contains(&CANARY),
        "governed child inherited a poisoned ambient variable: {observed}"
    );
    assert!(
        names.contains(&"PATH"),
        "allowlisted PATH missing from child env: {observed}"
    );
    assert!(
        names.contains(&"HOME"),
        "allowlisted HOME missing from child env: {observed}"
    );
    assert!(
        names
            .iter()
            .all(|name| runner::CHILD_ENV_ALLOWLIST.contains(name)),
        "child env exposed a non-allowlisted name: {observed}"
    );

    std::env::remove_var(CANARY);
}
