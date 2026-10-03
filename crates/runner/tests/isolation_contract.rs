use std::cell::Cell;
use std::ffi::{OsStr, OsString};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use policy::{Capability, EffectiveCapabilities, FileScope, ScopeId};
use runner::{
    enforce_process_operation, prepare_process_launch, BackendIdentity, ChildEnvironment,
    ChildSecretEnvironment, CommandSpec, EnforcementAxes, EnforcementReport, IsolationBackend,
    IsolationPlan, IsolationRequirements, LaunchRecipe, Platform, RunnerError, RunnerKind,
    CHILD_SECRET_ENV_ALLOWLIST, MAX_CHILD_SECRET_BYTES,
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
    recipe_environment: Option<Vec<(OsString, OsString)>>,
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
            recipe_environment: None,
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
        let environment = self
            .recipe_environment
            .clone()
            .unwrap_or_else(|| plan.environment().vars().to_vec());
        Ok(LaunchRecipe::new(
            self.recipe_program
                .clone()
                .unwrap_or_else(|| spec.program().to_owned()),
            spec.args().iter().cloned(),
            environment,
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
    let root =
        std::env::temp_dir().join(format!("runlith-runner-r1-h1-route-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let workspace = root.canonicalize().unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let backend = FixedBackend {
        sandbox_calls: Arc::clone(&calls),
        recipe_program: Some(OsString::from("/usr/bin/env")),
        ..FixedBackend::new(EnforcementAxes::all())
    };
    let effective = EffectiveCapabilities::new(vec![Capability::process("cargo").unwrap()]);
    let launch = prepare_process_launch(
        Arc::new(backend),
        &effective,
        workspace.to_string_lossy().into_owned(),
        scope("cargo"),
        IsolationRequirements::trusted_host(),
        environment(),
    )
    .expect("verified launch context");

    // Verification alone does not sandbox anything; the backend owns that step.
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    let sandboxed = launch
        .bind(CommandSpec::new(
            OsString::from("cargo"),
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
            wire.current_dir(&workspace)
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .stdin(std::process::Stdio::null());
        })
        .expect("spawn through the backend-owned recipe");
    let output = child.wait_with_output().expect("wait for child");
    // `/usr/bin/env must-not-run` is what actually ran: `env` reports 127 for a
    // command it cannot find. The authorized target was `cargo`, but the
    // backend's sandbox recipe decided the actual wrapper program.
    assert_eq!(
        output.status.code(),
        Some(127),
        "spawned program was not the backend recipe program: {:?}",
        String::from_utf8_lossy(&output.stderr)
    );
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn r1_h1_bind_rejects_a_different_process_target() {
    let calls = Arc::new(AtomicUsize::new(0));
    let backend = FixedBackend {
        sandbox_calls: Arc::clone(&calls),
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
    .unwrap();

    assert!(matches!(
        launch.bind(CommandSpec::new(OsString::from("rustc"), [])),
        Err(RunnerError::ProcessNotAuthorized { .. })
    ));
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[test]
fn r1_h1_bind_rejects_backend_environment_expansion() {
    let mut expanded = environment().vars().to_vec();
    expanded.push((OsString::from("RUNLITH_R1_CANARY"), OsString::from("leak")));
    let backend = FixedBackend {
        recipe_environment: Some(expanded),
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
    .unwrap();

    assert!(matches!(
        launch.bind(CommandSpec::new(OsString::from("cargo"), [])),
        Err(RunnerError::EnvironmentNotIsolated)
    ));
}

#[test]
fn r1_h1_spawn_rejects_missing_working_directory() {
    let root = std::env::temp_dir().join(format!(
        "runlith-runner-r1-h1-no-cwd-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let workspace = root.canonicalize().unwrap();
    let effective = EffectiveCapabilities::new(vec![Capability::process("/usr/bin/true").unwrap()]);
    let launch = prepare_process_launch(
        Arc::new(FixedBackend::new(EnforcementAxes::all())),
        &effective,
        workspace.to_string_lossy().into_owned(),
        scope("/usr/bin/true"),
        IsolationRequirements::trusted_host(),
        environment(),
    )
    .unwrap();
    let sandboxed = launch
        .bind(CommandSpec::new(OsString::from("/usr/bin/true"), []))
        .unwrap();

    let error = sandboxed.spawn(|_| {}).unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::PermissionDenied);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn r1_h1_spawn_rejects_cwd_outside_the_prepared_workspace() {
    let root =
        std::env::temp_dir().join(format!("runlith-runner-r1-h1-cwd-{}", std::process::id()));
    let workspace = root.join("workspace");
    let outside = root.join("outside");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&workspace).unwrap();
    std::fs::create_dir_all(&outside).unwrap();
    let workspace = workspace.canonicalize().unwrap();
    let effective = EffectiveCapabilities::new(vec![Capability::process("/usr/bin/true").unwrap()]);
    let launch = prepare_process_launch(
        Arc::new(FixedBackend::new(EnforcementAxes::all())),
        &effective,
        workspace.to_string_lossy().into_owned(),
        scope("/usr/bin/true"),
        IsolationRequirements::trusted_host(),
        environment(),
    )
    .unwrap();
    let sandboxed = launch
        .bind(CommandSpec::new(OsString::from("/usr/bin/true"), []))
        .unwrap();

    let error = sandboxed
        .spawn(|wire| {
            wire.current_dir(&outside);
        })
        .unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::PermissionDenied);
    let _ = std::fs::remove_dir_all(root);
}

#[cfg(unix)]
#[test]
fn r1_h1_spawn_rejects_cwd_symlink_escape() {
    use std::os::unix::fs::symlink;

    let root = std::env::temp_dir().join(format!(
        "runlith-runner-r1-h1-cwd-symlink-{}",
        std::process::id()
    ));
    let workspace = root.join("workspace");
    let outside = root.join("outside");
    let escape = workspace.join("escape");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&workspace).unwrap();
    std::fs::create_dir_all(&outside).unwrap();
    symlink(&outside, &escape).unwrap();
    let workspace = workspace.canonicalize().unwrap();
    let effective = EffectiveCapabilities::new(vec![Capability::process("/usr/bin/true").unwrap()]);
    let launch = prepare_process_launch(
        Arc::new(FixedBackend::new(EnforcementAxes::all())),
        &effective,
        workspace.to_string_lossy().into_owned(),
        scope("/usr/bin/true"),
        IsolationRequirements::trusted_host(),
        environment(),
    )
    .unwrap();
    let sandboxed = launch
        .bind(CommandSpec::new(OsString::from("/usr/bin/true"), []))
        .unwrap();

    let error = sandboxed
        .spawn(|wire| {
            wire.current_dir(&escape);
        })
        .unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::PermissionDenied);
    let _ = std::fs::remove_dir_all(root);
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

/// The login principal must survive the plan: refusing it silently strips
/// provider-owned authentication, which the provider then reports only as a
/// bare exit 1. Paired with the provider-side passthrough in
/// `provider_claude::ProviderEnvironment`.
#[test]
fn login_principal_is_carried_into_the_plan() {
    let environment = ChildEnvironment::from_explicit([
        (OsString::from("PATH"), OsString::from("/usr/bin")),
        (OsString::from("USER"), OsString::from("principal")),
    ])
    .expect("USER must be allowlisted for provider-owned authentication");
    assert!(environment
        .vars()
        .iter()
        .any(|(name, value)| name == OsStr::new("USER") && value == OsStr::new("principal")));
}

#[test]
fn git_system_config_suppression_is_explicitly_allowlisted() {
    let environment = ChildEnvironment::from_explicit([(
        OsString::from("GIT_CONFIG_NOSYSTEM"),
        OsString::from("1"),
    )])
    .expect("governed Git may disable host system configuration");
    assert_eq!(
        environment.vars(),
        &[(OsString::from("GIT_CONFIG_NOSYSTEM"), OsString::from("1"))]
    );
    assert_eq!(
        ChildEnvironment::from_explicit([(
            OsString::from("GIT_CONFIG_GLOBAL"),
            OsString::from("/tmp/untrusted-gitconfig"),
        )])
        .err(),
        Some(RunnerError::EnvironmentNameNotAllowed {
            name: "GIT_CONFIG_GLOBAL".to_owned(),
        })
    );
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

// -- ADR-0039 P2: late-bound child secret environment -----------------------

const SECRET_NAME: &str = "CLAUDE_CODE_OAUTH_TOKEN";
const SECRET_CANARY: &str = "oat-canary-7f3a91c2d4e85b06";

fn canary() -> Vec<u8> {
    SECRET_CANARY.as_bytes().to_vec()
}

#[test]
fn secret_allowlist_names_only_the_setup_token_variable() {
    assert_eq!(CHILD_SECRET_ENV_ALLOWLIST, &[SECRET_NAME]);
}

#[test]
fn secret_environment_refuses_unlisted_duplicate_and_invalid_values() {
    let mut secrets = ChildSecretEnvironment::new();
    assert!(matches!(
        secrets.insert("AWS_SECRET_ACCESS_KEY", canary()),
        Err(RunnerError::SecretNameNotAllowed { .. })
    ));
    assert!(matches!(
        secrets.insert(SECRET_NAME, Vec::new()),
        Err(RunnerError::InvalidSecretValue)
    ));
    assert!(matches!(
        secrets.insert(SECRET_NAME, b"a\0b".to_vec()),
        Err(RunnerError::InvalidSecretValue)
    ));
    assert!(matches!(
        secrets.insert(SECRET_NAME, vec![b'x'; MAX_CHILD_SECRET_BYTES + 1]),
        Err(RunnerError::InvalidSecretValue)
    ));
    assert_eq!(secrets.names().count(), 0, "refused inserts store nothing");

    secrets.insert(SECRET_NAME, canary()).unwrap();
    assert!(matches!(
        secrets.insert(SECRET_NAME, b"another".to_vec()),
        Err(RunnerError::DuplicateSecretName { .. })
    ));
    assert_eq!(secrets.names().collect::<Vec<_>>(), vec![SECRET_NAME]);
}

#[test]
fn secret_environment_debug_and_errors_never_render_the_value() {
    let mut secrets = ChildSecretEnvironment::new();
    secrets.insert(SECRET_NAME, canary()).unwrap();
    let rendered = format!("{secrets:?}");
    assert!(rendered.contains(SECRET_NAME));
    assert!(!rendered.contains(SECRET_CANARY));

    let mut tainted = SECRET_CANARY.as_bytes().to_vec();
    tainted.push(0);
    let error = ChildSecretEnvironment::new()
        .insert(SECRET_NAME, tainted)
        .unwrap_err();
    assert!(!format!("{error:?} {error}").contains(SECRET_CANARY));
}

/// The secret reaches the real child only through `spawn_with_secrets`, and
/// never enters the plan, the recipe environment or any `Debug` rendering.
#[cfg(unix)]
#[test]
fn secret_reaches_child_only_through_late_bound_spawn() {
    let root = std::env::temp_dir().join(format!("runlith-runner-secret-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let workspace = root.canonicalize().unwrap();
    let backend = FixedBackend {
        recipe_program: Some(OsString::from("/usr/bin/env")),
        ..FixedBackend::new(EnforcementAxes::all())
    };
    let effective = EffectiveCapabilities::new(vec![Capability::process("cargo").unwrap()]);
    let launch = prepare_process_launch(
        Arc::new(backend),
        &effective,
        workspace.to_string_lossy().into_owned(),
        scope("cargo"),
        IsolationRequirements::trusted_host(),
        environment(),
    )
    .expect("verified launch context");
    let sandboxed = launch
        .bind(CommandSpec::new(OsString::from("cargo"), []))
        .expect("backend recipe");

    let mut secrets = ChildSecretEnvironment::new();
    secrets.insert(SECRET_NAME, canary()).unwrap();

    let run = |with_secrets: bool| {
        let wire = |wire: &mut runner::SpawnWiring<'_>| {
            wire.current_dir(&workspace)
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .stdin(std::process::Stdio::null());
        };
        let child = if with_secrets {
            sandboxed.spawn_with_secrets(&secrets, wire)
        } else {
            sandboxed.spawn(wire)
        }
        .expect("spawn");
        let output = child.wait_with_output().expect("wait");
        String::from_utf8_lossy(&output.stdout).into_owned()
    };

    let without = run(false);
    assert!(
        !without.contains(SECRET_CANARY),
        "plain spawn must not carry it"
    );
    let with = run(true);
    assert!(
        with.lines()
            .any(|line| line == format!("{SECRET_NAME}={SECRET_CANARY}")),
        "late-bound spawn must deliver the secret: {with}"
    );
    assert!(with.contains("PATH=/usr/bin:/bin"), "plan environment kept");

    assert!(!format!("{sandboxed:?}").contains(SECRET_CANARY));
    assert!(!format!("{launch:?}").contains(SECRET_CANARY));
    assert!(!format!("{:?}", sandboxed.plan()).contains(SECRET_CANARY));
    assert!(sandboxed
        .environment()
        .iter()
        .all(|(name, _)| name != SECRET_NAME));
    let _ = std::fs::remove_dir_all(root);
}

/// ADR-0039 P3: the provider's private config directory is a plain path
/// variable the plan may carry; unlisted names are still refused.
#[test]
fn child_environment_allowlist_admits_the_claude_config_dir_only_as_a_path_variable() {
    let accepted = ChildEnvironment::from_explicit([(
        OsString::from("CLAUDE_CONFIG_DIR"),
        OsString::from("/private/runtime/claude"),
    )])
    .expect("CLAUDE_CONFIG_DIR is allowlisted");
    assert_eq!(accepted.vars().len(), 1);
    assert!(matches!(
        ChildEnvironment::from_explicit([(
            OsString::from("CLAUDE_CODE_OAUTH_TOKEN"),
            OsString::from("not-a-plan-variable"),
        )]),
        Err(RunnerError::EnvironmentNameNotAllowed { .. })
    ));
}
