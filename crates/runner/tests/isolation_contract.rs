use std::cell::Cell;
use std::path::PathBuf;

use policy::{Capability, EffectiveCapabilities, FileScope, ScopeId};
use runner::{
    enforce_process_operation, BackendIdentity, EnforcementAxes, EnforcementReport,
    IsolationBackend, IsolationPlan, IsolationRequirements, Platform, RunnerError, RunnerKind,
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
}

#[test]
fn compiles_only_runner_owned_effective_authority() {
    let plan = IsolationPlan::compile(
        &effective(),
        "ws-1",
        scope("cargo"),
        IsolationRequirements::new(true, true, true),
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
