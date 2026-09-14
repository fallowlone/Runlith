//! Provider-neutral Runner / OS-isolation contracts (TASK-0064, G6).
//!
//! Policy decides authority. This crate consumes already-computed
//! [`policy::EffectiveCapabilities`] plus policy-derived isolation
//! requirements and checks them immediately before a privileged process
//! operation. It owns no policy rules, approvals, provider behavior,
//! environment construction, persistence, or concrete sandbox mechanism.
//!
//! The child environment is *not* constructed here (that stays with the
//! provider/composition owner), but it is *enforced* here: a plan carries the
//! complete explicit child environment ([`ChildEnvironment`]) and every backend
//! clears the ambient environment before installing it, so an ambient
//! credential can never reach a governed child by inheritance (TASK-SEC-R1
//! R1-H2).

use std::ffi::{OsStr, OsString};
use std::fmt;
use std::path::PathBuf;
use std::process::Command;

use policy::{Capability, EffectiveCapabilities, ScopeId};

/// Coarse outbound HTTPS authority currently enforceable by the macOS
/// Seatbelt backend. Scope text matches the actual OS primitive: any remote
/// TCP destination on port 443, with all other remote ports denied.
pub const NETWORK_SCOPE_OUTBOUND_TCP_443: &str = "tcp:443";

/// Names a governed child may receive. Everything outside this list is
/// unreachable for a Runner-launched process: the child environment is
/// explicit data, never inheritance. Provider runtime redirection names are
/// allowlisted because they always carry composition-resolved directories
/// (private runtime/scratch roots), never ambient values.
pub const CHILD_ENV_ALLOWLIST: &[&str] = &[
    // Process/runtime basics.
    "PATH",
    "HOME",
    "TMPDIR",
    "PWD",
    "LANG",
    "LC_ALL",
    "LC_CTYPE",
    // Provider runtime redirection (explicit private directories).
    "XDG_CONFIG_HOME",
    "XDG_DATA_HOME",
    "XDG_STATE_HOME",
    "XDG_CACHE_HOME",
    "CLAUDE_CODE_TMPDIR",
    "TMP",
    "TEMP",
    // Windows process basics.
    "USERPROFILE",
    "SystemRoot",
    "WINDIR",
    "PATHEXT",
    "APPDATA",
    "LOCALAPPDATA",
];

/// Complete, explicit child environment for one isolation plan.
///
/// The value is plain data resolved by trusted composition (provider
/// environment preparation or the runner's own minimal allowlist). It is
/// never populated by implicit inheritance, and backends install exactly
/// these pairs after clearing the ambient environment.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ChildEnvironment {
    vars: Vec<(OsString, OsString)>,
}

impl ChildEnvironment {
    /// Empty environment: the child starts with no variables at all.
    #[must_use]
    pub const fn empty() -> Self {
        Self { vars: Vec::new() }
    }

    /// Builds an explicit environment from already-resolved pairs.
    ///
    /// Fails closed on any name outside [`CHILD_ENV_ALLOWLIST`], on empty
    /// names, and on duplicate names, so an ambient secret cannot be smuggled
    /// into a plan by a composition mistake.
    pub fn from_explicit(
        vars: impl IntoIterator<Item = (OsString, OsString)>,
    ) -> Result<Self, RunnerError> {
        let mut collected: Vec<(OsString, OsString)> = Vec::new();
        for (name, value) in vars {
            if name.is_empty() || !CHILD_ENV_ALLOWLIST.iter().any(|allowed| *allowed == name) {
                return Err(RunnerError::EnvironmentNameNotAllowed {
                    name: name.to_string_lossy().into_owned(),
                });
            }
            if collected
                .iter()
                .any(|(existing, _)| existing.as_os_str() == name.as_os_str())
            {
                return Err(RunnerError::DuplicateEnvironmentName {
                    name: name.to_string_lossy().into_owned(),
                });
            }
            collected.push((name, value));
        }
        collected.sort_by(|left, right| left.0.cmp(&right.0));
        Ok(Self { vars: collected })
    }

    /// Resolves the runner's own minimal allowlist (PATH, HOME, TMPDIR and
    /// locale) from current process state. Only allowlisted names are read;
    /// missing names are simply absent.
    pub fn minimal_allowlist() -> Result<Self, RunnerError> {
        let mut vars: Vec<(OsString, OsString)> = Vec::new();
        for name in ["PATH", "HOME", "TMPDIR", "LANG", "LC_ALL", "LC_CTYPE"] {
            if let Some(value) = std::env::var_os(name).filter(|value| !value.is_empty()) {
                vars.push((OsString::from(name), value));
            }
        }
        Self::from_explicit(vars)
    }

    /// Sorted, de-duplicated explicit pairs.
    #[must_use]
    pub fn vars(&self) -> &[(OsString, OsString)] {
        &self.vars
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.vars.is_empty()
    }

    #[must_use]
    pub fn contains(&self, name: impl AsRef<OsStr>) -> bool {
        self.vars
            .iter()
            .any(|(existing, _)| existing.as_os_str() == name.as_ref())
    }
}

/// Runner implementations accepted by the architecture. Only Native is
/// implemented by TASK-0064; Container remains a replaceable future backend,
/// while Remote/VM remain later architecture targets.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RunnerKind {
    Native,
    Container,
    Remote,
    Vm,
}

/// Host platform identity used only for backend capability reporting.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Platform {
    MacOs,
    Linux,
    Windows,
    Other,
}

impl Platform {
    #[must_use]
    pub const fn current() -> Self {
        #[cfg(target_os = "macos")]
        {
            return Self::MacOs;
        }
        #[cfg(target_os = "linux")]
        {
            return Self::Linux;
        }
        #[cfg(target_os = "windows")]
        {
            return Self::Windows;
        }
        #[allow(unreachable_code)]
        Self::Other
    }
}

/// Distinct isolation axes. Filesystem restriction never implies network or
/// process restriction, and vice versa.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IsolationAxis {
    Filesystem,
    Network,
    Process,
}

/// Compact set of isolation axes. This describes enforcement strength only;
/// it never grants an operation or capability.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct EnforcementAxes {
    filesystem: bool,
    network: bool,
    process: bool,
}

impl EnforcementAxes {
    #[must_use]
    pub const fn none() -> Self {
        Self {
            filesystem: false,
            network: false,
            process: false,
        }
    }

    #[must_use]
    pub const fn all() -> Self {
        Self {
            filesystem: true,
            network: true,
            process: true,
        }
    }

    #[must_use]
    pub const fn new(filesystem: bool, network: bool, process: bool) -> Self {
        Self {
            filesystem,
            network,
            process,
        }
    }

    #[must_use]
    pub const fn contains(self, required: Self) -> bool {
        (!required.filesystem || self.filesystem)
            && (!required.network || self.network)
            && (!required.process || self.process)
    }

    #[must_use]
    pub const fn requires(self, axis: IsolationAxis) -> bool {
        match axis {
            IsolationAxis::Filesystem => self.filesystem,
            IsolationAxis::Network => self.network,
            IsolationAxis::Process => self.process,
        }
    }

    #[must_use]
    pub const fn is_empty(self) -> bool {
        !self.filesystem && !self.network && !self.process
    }
}

/// Policy-derived isolation requirements. They describe which runtime
/// restrictions must be mechanically enforced; canonical operation authority
/// still comes exclusively from `EffectiveCapabilities`.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct IsolationRequirements(EnforcementAxes);

impl IsolationRequirements {
    /// Trusted-host execution: no OS sandbox axis is required. Exact process
    /// authorization is still mandatory at the operation boundary.
    #[must_use]
    pub const fn trusted_host() -> Self {
        Self(EnforcementAxes::none())
    }

    #[must_use]
    pub const fn new(filesystem: bool, network: bool, process: bool) -> Self {
        Self(EnforcementAxes::new(filesystem, network, process))
    }

    #[must_use]
    pub const fn axes(self) -> EnforcementAxes {
        self.0
    }
}

/// Stable identity and honest capability label for one backend instance.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BackendIdentity {
    pub name: &'static str,
    pub runner: RunnerKind,
    pub platform: Platform,
}

/// Deterministic isolation plan compiled only from current effective authority
/// plus non-authorizing isolation requirements.
///
/// The plan carries the complete explicit child environment. It is enforced,
/// not inherited: backends clear the ambient environment and install exactly
/// [`IsolationPlan::environment`] before the child exists.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IsolationPlan {
    workspace: String,
    launch_executable: ScopeId,
    filesystem_read_roots: Vec<PathBuf>,
    filesystem_write_roots: Vec<PathBuf>,
    process_executables: Vec<ScopeId>,
    network_destinations: Vec<ScopeId>,
    requirements: IsolationRequirements,
    environment: ChildEnvironment,
}

impl IsolationPlan {
    /// Compiles the runner-owned part of canonical effective authority.
    /// Capabilities owned by other boundaries (Git, secrets, tools, external
    /// actions) are intentionally absent from this plan.
    pub fn compile(
        effective: &EffectiveCapabilities,
        workspace: impl Into<String>,
        launch_executable: ScopeId,
        requirements: IsolationRequirements,
        environment: ChildEnvironment,
    ) -> Result<Self, RunnerError> {
        let workspace = checked_workspace(workspace.into())?;
        let launch_capability = Capability::ProcessExecute(launch_executable.clone());
        if !effective.permits(&launch_capability) {
            return Err(RunnerError::ProcessNotAuthorized {
                executable: launch_executable.as_str().to_owned(),
            });
        }

        let mut filesystem_read_roots = Vec::new();
        let mut filesystem_write_roots = Vec::new();
        let mut process_executables = Vec::new();
        let mut network_destinations = Vec::new();

        for capability in effective.as_slice() {
            match capability {
                Capability::FilesystemRead(scope) => {
                    ensure_workspace(&workspace, scope.workspace())?;
                    filesystem_read_roots.push(scope.root().to_path_buf());
                }
                Capability::FilesystemWrite(scope) => {
                    ensure_workspace(&workspace, scope.workspace())?;
                    filesystem_write_roots.push(scope.root().to_path_buf());
                }
                Capability::ProcessExecute(scope) => process_executables.push(scope.clone()),
                Capability::NetworkAccess(scope) => network_destinations.push(scope.clone()),
                Capability::GitMutate(_)
                | Capability::SecretAccess(_)
                | Capability::ToolInvoke(_)
                | Capability::ExternalAction(_) => {}
            }
        }

        filesystem_read_roots.sort();
        filesystem_read_roots.dedup();
        filesystem_write_roots.sort();
        filesystem_write_roots.dedup();
        process_executables.sort_by(|left, right| left.as_str().cmp(right.as_str()));
        process_executables.dedup();
        network_destinations.sort_by(|left, right| left.as_str().cmp(right.as_str()));
        network_destinations.dedup();

        Ok(Self {
            workspace,
            launch_executable,
            filesystem_read_roots,
            filesystem_write_roots,
            process_executables,
            network_destinations,
            requirements,
            environment,
        })
    }

    #[must_use]
    pub fn workspace(&self) -> &str {
        &self.workspace
    }

    #[must_use]
    pub fn launch_executable(&self) -> &ScopeId {
        &self.launch_executable
    }

    #[must_use]
    pub fn filesystem_read_roots(&self) -> &[PathBuf] {
        &self.filesystem_read_roots
    }

    #[must_use]
    pub fn filesystem_write_roots(&self) -> &[PathBuf] {
        &self.filesystem_write_roots
    }

    #[must_use]
    pub fn process_executables(&self) -> &[ScopeId] {
        &self.process_executables
    }

    #[must_use]
    pub fn network_destinations(&self) -> &[ScopeId] {
        &self.network_destinations
    }

    #[must_use]
    pub const fn requirements(&self) -> IsolationRequirements {
        self.requirements
    }

    /// The complete explicit child environment this plan enforces.
    #[must_use]
    pub const fn environment(&self) -> &ChildEnvironment {
        &self.environment
    }
}

/// Evidence returned by a trusted backend after it has applied restrictions.
/// The report is tied to the exact plan so stale/widened reports cannot be
/// reused for another process operation.
///
/// `env_isolated` is a distinct, always-required axis (TASK-SEC-R1 R1-H2): a
/// backend may only claim it after it actually cleared the ambient environment
/// and installed the plan's explicit child environment. Filesystem, network and
/// process axes never imply it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EnforcementReport {
    backend: BackendIdentity,
    plan: IsolationPlan,
    enforced: EnforcementAxes,
    env_isolated: bool,
}

impl EnforcementReport {
    #[must_use]
    pub fn new(
        backend: BackendIdentity,
        plan: IsolationPlan,
        enforced: EnforcementAxes,
        env_isolated: bool,
    ) -> Self {
        Self {
            backend,
            plan,
            enforced,
            env_isolated,
        }
    }

    #[must_use]
    pub const fn backend(&self) -> BackendIdentity {
        self.backend
    }

    #[must_use]
    pub const fn enforced(&self) -> EnforcementAxes {
        self.enforced
    }

    /// Whether the backend cleared the ambient environment and installed
    /// exactly the plan's explicit child environment.
    #[must_use]
    pub const fn env_isolated(&self) -> bool {
        self.env_isolated
    }
}

/// Replaceable backend boundary. Implementations must report only restrictions
/// actually installed for the supplied exact plan.
pub trait IsolationBackend {
    fn identity(&self) -> BackendIdentity;

    fn enforce(&self, plan: &IsolationPlan) -> Result<EnforcementReport, RunnerError>;

    fn prepare_launch(
        &self,
        plan: &IsolationPlan,
    ) -> Result<Box<dyn PreparedLaunch + Send + Sync>, RunnerError> {
        let _ = plan;
        Err(RunnerError::BackendUnavailable {
            backend: "launch handoff unavailable",
        })
    }
}

/// Prepared child-process launch handoff. The backend that reports
/// enforcement owns the launch mutation, so providers cannot bypass the
/// runtime boundary with a raw `Command::spawn`.
pub trait PreparedLaunch {
    fn apply(&self, command: &mut Command);
}

/// Enforces the current exact authority and then performs the privileged
/// process operation inside the same call. No reusable bearer authorization is
/// returned; callers re-present current effective authority for each operation.
pub fn enforce_process_operation<T>(
    backend: &dyn IsolationBackend,
    effective: &EffectiveCapabilities,
    workspace: impl Into<String>,
    executable: ScopeId,
    requirements: IsolationRequirements,
    environment: ChildEnvironment,
    operation: impl FnOnce(&IsolationPlan) -> T,
) -> Result<T, RunnerError> {
    let plan = IsolationPlan::compile(effective, workspace, executable, requirements, environment)?;
    let report = backend.enforce(&plan)?;
    verify_report(backend.identity(), &plan, &report)?;
    Ok(operation(&plan))
}

/// Installs and returns the exact launch handoff for a verified plan.
/// Providers must carry this object into the real child spawn.
pub fn prepare_process_launch(
    backend: &dyn IsolationBackend,
    effective: &EffectiveCapabilities,
    workspace: impl Into<String>,
    executable: ScopeId,
    requirements: IsolationRequirements,
    environment: ChildEnvironment,
) -> Result<Box<dyn PreparedLaunch + Send + Sync>, RunnerError> {
    let plan = IsolationPlan::compile(effective, workspace, executable, requirements, environment)?;
    let report = backend.enforce(&plan)?;
    verify_report(backend.identity(), &plan, &report)?;
    backend.prepare_launch(&plan)
}

fn verify_report(
    expected_backend: BackendIdentity,
    plan: &IsolationPlan,
    report: &EnforcementReport,
) -> Result<(), RunnerError> {
    if report.backend != expected_backend {
        return Err(RunnerError::BackendIdentityMismatch);
    }
    if &report.plan != plan {
        return Err(RunnerError::StaleEnforcementReport);
    }
    let required = plan.requirements().axes();
    if !report.enforced.contains(required) {
        return Err(RunnerError::InsufficientEnforcement {
            required,
            enforced: report.enforced,
        });
    }
    // Child environment isolation is mandatory on every path: a backend that
    // cannot prove it cleared the ambient environment must not launch.
    if !report.env_isolated {
        return Err(RunnerError::EnvironmentNotIsolated);
    }
    Ok(())
}

fn checked_workspace(value: String) -> Result<String, RunnerError> {
    let trimmed = value.trim();
    if trimmed.is_empty() || trimmed.len() > 256 || trimmed.chars().any(char::is_control) {
        return Err(RunnerError::InvalidWorkspaceIdentity);
    }
    Ok(trimmed.to_owned())
}

fn ensure_workspace(expected: &str, actual: &str) -> Result<(), RunnerError> {
    if expected == actual {
        Ok(())
    } else {
        Err(RunnerError::WorkspaceScopeMismatch {
            expected: expected.to_owned(),
            actual: actual.to_owned(),
        })
    }
}

/// Contract failures are deliberately separate from provider/runtime failures.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RunnerError {
    InvalidWorkspaceIdentity,
    ProcessNotAuthorized {
        executable: String,
    },
    WorkspaceScopeMismatch {
        expected: String,
        actual: String,
    },
    BackendUnavailable {
        backend: &'static str,
    },
    BackendIdentityMismatch,
    StaleEnforcementReport,
    InsufficientEnforcement {
        required: EnforcementAxes,
        enforced: EnforcementAxes,
    },
    /// A plan tried to install a child-environment name outside
    /// [`CHILD_ENV_ALLOWLIST`].
    EnvironmentNameNotAllowed {
        name: String,
    },
    /// A plan carried the same child-environment name twice.
    DuplicateEnvironmentName {
        name: String,
    },
    /// The backend could not prove it cleared the ambient environment and
    /// installed exactly the plan's explicit child environment.
    EnvironmentNotIsolated,
}

impl fmt::Display for RunnerError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidWorkspaceIdentity => write!(formatter, "invalid workspace identity"),
            Self::ProcessNotAuthorized { executable } => {
                write!(
                    formatter,
                    "process execution is not authorized for {executable}"
                )
            }
            Self::WorkspaceScopeMismatch { .. } => {
                write!(
                    formatter,
                    "effective filesystem scope targets another workspace"
                )
            }
            Self::BackendUnavailable { backend } => {
                write!(formatter, "isolation backend unavailable: {backend}")
            }
            Self::BackendIdentityMismatch => {
                write!(formatter, "isolation backend report identity mismatch")
            }
            Self::StaleEnforcementReport => {
                write!(
                    formatter,
                    "isolation backend report does not match current plan"
                )
            }
            Self::InsufficientEnforcement { .. } => {
                write!(
                    formatter,
                    "isolation backend cannot prove required restrictions"
                )
            }
            Self::EnvironmentNameNotAllowed { name } => {
                write!(
                    formatter,
                    "child environment name is not allowlisted: {name}"
                )
            }
            Self::DuplicateEnvironmentName { name } => {
                write!(formatter, "duplicate child environment name: {name}")
            }
            Self::EnvironmentNotIsolated => {
                write!(
                    formatter,
                    "isolation backend cannot prove child environment isolation"
                )
            }
        }
    }
}

impl std::error::Error for RunnerError {}
