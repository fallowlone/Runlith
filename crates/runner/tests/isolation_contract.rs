use std::cell::Cell;
use std::ffi::{OsStr, OsString};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use policy::{Capability, EffectiveCapabilities, FileScope, ScopeId};
use runner::{
    enforce_process_operation, prepare_process_launch, BackendIdentity, ChildEnvironment,
    CommandSpec, EnforcementAxes, EnforcementReport, IsolationBackend, IsolationPlan,
    IsolationRequirements, LaunchRecipe, Platform, RunnerError, RunnerKind,
};

fn file_read(workspace: &str, root: &str) -> Capability {
    Capability::FilesystemRead(FileScope::new(workspace, root).unwrap())
}

fn file_write(workspace: &str, root: &str) -> Capability {
    Capability::FilesystemWrite(FileScope::new(workspace, root).unwrap())
}

fn scope(value: &str) -> ScopeId {
    ScopeId::new(value).unwrap()
}

/// Explicit, minimal child environment used by these contract fixtures.
fn environment() -> ChildEnvironment {
    ChildEnvironment::from_explicit([
        (OsString::from("PATH"), OsString::from("/usr/bin:/bin")),
        (OsString::from("HOME"), OsString::from("/home/tester")),
    ])
    .unwrap()
}

fn effective() -> EffectiveCapabilities {
    EffectiveCapabilities::new(vec![
        file_read("ws-1", "src"),
        file_write("ws-1", "src/generated"),
        Capability::process("cargo").unwrap(),
        Capability::process("rustc").unwrap(),
        Capability::network("crates.io").unwrap(),
        Capability::secret("secret-opaque").unwrap(),
        Capability::tool("tool-1").unwrap(),
    ])
}

#[derive(Clone)]
struct FixedBackend {
    identity: BackendIdentity,
    enforced: EnforcementAxes,
    report_plan: Option<IsolationPlan>,
    env_isolated: bool,
    sandbox_calls: Arc<AtomicUsize>,
    recipe_program: Option<OsString>,
}

impl FixedBackend {
    fn new(enforced: EnforcementAxes) -> Self {
        Self {
            identity: BackendIdentity {
                name: "fixture-isolation",
                runner: RunnerKind::Container,
                platform: Platform::current(),
            },
            enforced,
            report_plan: None,
            env_isolated: true,
            sandbox_calls: Arc::new(AtomicUsize::new(0)),
            recipe_program: None,
        }
    }
}

impl IsolationBackend for FixedBackend {
    fn identity(&self) -> BackendIdentity {
        self.identity
    }

    fn enforce(&self, plan: &IsolationPlan) -> Result<EnforcementReport, RunnerError> {
        Ok(EnforcementReport::new(
            self.identity,
            self.report_plan.clone().unwrap_or_else(|| plan.clone()),
            self.enforced,
            self.env_isolated,
        ))
    }

    fn sandbox(
        &self,
        plan: &IsolationPlan,
        spec: &CommandSpec,
    ) -> Result<LaunchRecipe, RunnerError> {
        self.sandbox_calls.fetch_add(1, Ordering::SeqCst);
        Ok(LaunchRecipe::new(
            self.recipe_program
                .clone()
                .unwrap_or_else(|| spec.program().to_owned()),
            spec.args().iter().cloned(),
            plan.environment().vars().to_vec(),
        ))
    }
}

struct UnavailableBackend;

impl IsolationBackend for UnavailableBackend {
    fn identity(&self) -> BackendIdentity {
        BackendIdentity {
            name: "unavailable-fixture",
            runner: RunnerKind::Container,
            platform: Platform::current(),
        }
    }

    fn enforce(&self, _plan: &IsolationPlan) -> Result<EnforcementReport, RunnerError> {
        Err(RunnerError::BackendUnavailable {
            backend: "unavailable-fixture",
        })
    }

    fn sandbox(
        &self,
        _plan: &IsolationPlan,
        _spec: &CommandSpec,
    ) -> Result<LaunchRecipe, RunnerError> {
        Err(RunnerError::BackendUnavailable {
            backend: "unavailable-fixture",
        })
    }
}

#[test]
fn compiles_only_runner_owned_effective_authority() {
    let plan = IsolationPlan::compile(
        &effective(),
        "ws-1",
        scope("cargo"),
        IsolationRequirements::new(true, true, true),
        environment(),
    )
    .unwrap();

    assert_eq!(plan.workspace(), "ws-1");
    assert_eq!(plan.launch_executable().as_str(), "cargo");
    assert_eq!(plan.filesystem_read_roots(), &[PathBuf::from("src")]);
    assert_eq!(
        plan.filesystem_write_roots(),
        &[PathBuf::from("src/generated")]
    );
    assert_eq!(
        plan.process_executables()
            .iter()
            .map(ScopeId::as_str)
            .collect::<Vec<_>>(),
        vec!["cargo", "rustc"]
    );
    assert_eq!(
        plan.network_destinations()
            .iter()
            .map(ScopeId::as_str)
            .collect::<Vec<_>>(),
        vec!["crates.io"]
    );
    assert!(plan
        .process_executables()
        .iter()
        .all(|value| value.as_str() != "secret-opaque" && value.as_str() != "tool-1"));
}

#[test]
fn exact_process_authority_is_rechecked_before_backend_or_operation() {
    let called = Cell::new(false);
    let backend = FixedBackend::new(EnforcementAxes::all());
    let result = enforce_process_operation(
        &backend,
        &effective(),
        "ws-1",
        scope("node"),
        IsolationRequirements::new(true, true, true),
        environment(),
        |_| called.set(true),
    );

    assert!(matches!(
        result,
        Err(RunnerError::ProcessNotAuthorized { .. })
    ));
    assert!(!called.get());
}

#[test]
fn cross_workspace_filesystem_authority_fails_closed() {
    let effective = EffectiveCapabilities::new(vec![
        Capability::process("cargo").unwrap(),
        file_read("ws-other", "src"),
    ]);
    let backend = FixedBackend::new(EnforcementAxes::all());

    assert!(matches!(
        enforce_process_operation(
            &backend,
            &effective,
            "ws-1",
            scope("cargo"),
            IsolationRequirements::new(true, false, false),
            environment(),
            |_| (),
        ),
        Err(RunnerError::WorkspaceScopeMismatch { .. })
    ));
}

#[test]
fn filesystem_network_and_process_enforcement_are_independent() {
    let backend = FixedBackend::new(EnforcementAxes::new(true, false, true));
    let called = Cell::new(false);

    assert!(matches!(
        enforce_process_operation(
            &backend,
            &effective(),
            "ws-1",
            scope("cargo"),
            IsolationRequirements::new(true, true, true),
            environment(),
            |_| called.set(true),
        ),
        Err(RunnerError::InsufficientEnforcement { .. })
    ));
    assert!(!called.get());

    let result = enforce_process_operation(
        &backend,
        &effective(),
        "ws-1",
        scope("cargo"),
        IsolationRequirements::new(true, false, true),
        environment(),
        |_| called.set(true),
    );
    assert!(result.is_ok());
    assert!(called.get());
}

#[test]
fn stale_or_widened_backend_report_cannot_authorize_operation() {
    let stale_plan = IsolationPlan::compile(
        &effective(),
        "ws-1",
        scope("rustc"),
        IsolationRequirements::new(true, true, true),
        environment(),
    )
    .unwrap();
    let backend = FixedBackend {
        report_plan: Some(stale_plan),
        ..FixedBackend::new(EnforcementAxes::all())
    };
    let called = Cell::new(false);

    assert_eq!(
        enforce_process_operation(
            &backend,
            &effective(),
            "ws-1",
            scope("cargo"),
            IsolationRequirements::new(true, true, true),
            environment(),
            |_| called.set(true),
        ),
        Err(RunnerError::StaleEnforcementReport)
    );
    assert!(!called.get());
}

#[test]
fn operation_runs_only_after_exact_enforcement_report() {
    let backend = FixedBackend::new(EnforcementAxes::all());
    let called = Cell::new(false);

    let value = enforce_process_operation(
        &backend,
        &effective(),
        "ws-1",
        scope("cargo"),
        IsolationRequirements::new(true, true, true),
        environment(),
        |plan| {
            called.set(true);
            plan.launch_executable().as_str().to_owned()
        },
    )
    .unwrap();

    assert!(called.get());
    assert_eq!(value, "cargo");
}

#[test]
fn unavailable_backend_fails_closed_without_running_operation() {
    let called = Cell::new(false);
    let result = enforce_process_operation(
        &UnavailableBackend,
        &effective(),
        "ws-1",
        scope("cargo"),
        IsolationRequirements::new(true, false, false),
        environment(),
        |_| called.set(true),
    );

    assert_eq!(
        result,
        Err(RunnerError::BackendUnavailable {
            backend: "unavailable-fixture",
        })
    );
    assert!(!called.get());
}

/// TASK-SEC-R1 R1-H2: a backend that cannot prove child environment isolation
/// must never authorize the launch, even when every other axis is proven.
#[test]
fn r1_h2_report_without_env_isolation_fails_verification() {
    let backend = FixedBackend {
        env_isolated: false,
        ..FixedBackend::new(EnforcementAxes::all())
    };
    let called = Cell::new(false);

    assert_eq!(
        enforce_process_operation(
            &backend,
            &effective(),
            "ws-1",
            scope("cargo"),
            IsolationRequirements::new(true, true, true),
            environment(),
            |_| called.set(true),
        ),
        Err(RunnerError::EnvironmentNotIsolated)
    );
    assert!(!called.get());

    assert_eq!(
        prepare_process_launch(
            Arc::new(backend),
            &effective(),
            "ws-1",
            scope("cargo"),
            IsolationRequirements::trusted_host(),
            environment(),
        )
        .err(),
        Some(RunnerError::EnvironmentNotIsolated)
    );
}

/// TASK-SEC-R1 R1-H1 (adversarial, real child): a caller cannot reach a
/// running child without the backend. Binding goes through the backend's
/// `sandbox` step (counted here), the spawned program is the backend's recipe
/// choice rather than the caller's request, and the child environment is the
/// recipe's plan environment.
#[test]
fn r1_h1_spawn_route_is_backend_owned() {
    let calls = Arc::new(AtomicUsize::new(0));
    let backend = FixedBackend {
        sandbox_calls: Arc::clone(&calls),
        recipe_program: Some(OsString::from("/usr/bin/env")),
        ..FixedBackend::new(EnforcementAxes::all())
    };
    let launch = prepare_process_launch(
        Arc::new(backend),
        &effective(),
        "ws-1",
        scope("cargo"),
        IsolationRequirements::trusted_host(),
        environment(),
    )
    .expect("verified launch context");

    // Verification alone does not sandbox anything; the backend owns that step.
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    let sandboxed = launch
        .bind(CommandSpec::new(
            OsString::from("/usr/bin/false"),
            [OsString::from("must-not-run")],
        ))
        .expect("backend recipe");
    assert_eq!(calls.load(Ordering::SeqCst), 1);

    // The backend chose the program: the caller's requested program never ran.
    assert_eq!(sandboxed.program(), OsStr::new("/usr/bin/env"));
    assert_eq!(
        sandboxed.args(),
        &[OsString::from("must-not-run")],
        "argv stays caller-owned; program selection stays backend-owned"
    );
    let recipe_environment: Vec<_> = sandboxed
        .environment()
        .iter()
        .map(|(name, value)| {
            (
                name.to_string_lossy().into_owned(),
                value.to_string_lossy().into_owned(),
            )
        })
        .collect();
    assert_eq!(
        recipe_environment,
        vec![
            ("HOME".to_owned(), "/home/tester".to_owned()),
            ("PATH".to_owned(), "/usr/bin:/bin".to_owned()),
        ]
    );

    let child = sandboxed
        .spawn(|wire| {
            wire.stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .stdin(std::process::Stdio::null());
        })
        .expect("spawn through the backend-owned recipe");
    let output = child.wait_with_output().expect("wait for child");
    // `/usr/bin/env must-not-run` is what actually ran: `env` reports 127 for a
    // command it cannot find, while the caller's requested `/usr/bin/false`
    // would have reported 1. The backend's recipe decided the program.
    assert_eq!(
        output.status.code(),
        Some(127),
        "spawned program was not the backend recipe program: {:?}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// TASK-SEC-R1 R1-H2: the plan carries the complete explicit child environment
/// and nothing else; the environment is data, not inheritance.
#[test]
fn r1_h2_plan_carries_only_the_explicit_child_environment() {
    let plan = IsolationPlan::compile(
        &effective(),
        "ws-1",
        scope("cargo"),
        IsolationRequirements::trusted_host(),
        environment(),
    )
    .unwrap();

    let pairs: Vec<_> = plan
        .environment()
        .vars()
        .iter()
        .map(|(name, value)| {
            (
                name.to_string_lossy().into_owned(),
                value.to_string_lossy().into_owned(),
            )
        })
        .collect();
    assert_eq!(
        pairs,
        vec![
            ("HOME".to_owned(), "/home/tester".to_owned()),
            ("PATH".to_owned(), "/usr/bin:/bin".to_owned()),
        ]
    );
    assert!(plan.environment().contains("PATH"));
    assert!(!plan.environment().contains("AWS_SECRET_ACCESS_KEY"));
}

/// TASK-SEC-R1 R1-H2: ambient-shaped names cannot be smuggled into a plan, so
/// a composition mistake cannot turn inheritance back on.
#[test]
fn r1_h2_poisoned_or_duplicate_env_names_refuse_to_compile() {
    assert_eq!(
        ChildEnvironment::from_explicit([(
            OsString::from("AWS_SECRET_ACCESS_KEY"),
            OsString::from("poison"),
        )])
        .err(),
        Some(RunnerError::EnvironmentNameNotAllowed {
            name: "AWS_SECRET_ACCESS_KEY".to_owned(),
        })
    );
    assert_eq!(
        ChildEnvironment::from_explicit([
            (OsString::from("PATH"), OsString::from("/usr/bin")),
            (OsString::from("PATH"), OsString::from("/attacker/bin")),
        ])
        .err(),
        Some(RunnerError::DuplicateEnvironmentName {
            name: "PATH".to_owned(),
        })
    );

    // The runner's own minimal allowlist resolves only allowlisted ambient
    // names: PATH/HOME/TMPDIR plus locale.
    let minimal = ChildEnvironment::minimal_allowlist().unwrap();
    for (name, _) in minimal.vars() {
        assert!(
            runner::CHILD_ENV_ALLOWLIST
                .iter()
                .any(|allowed| name.as_os_str() == OsStr::new(allowed)),
            "minimal allowlist leaked {name:?}"
        );
    }
}
