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
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

use zeroize::Zeroizing;

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
    // The POSIX login principal. Provider-owned authentication resolves the
    // caller's stored credentials through it, so a child that gets HOME
    // without USER is unauthenticated. It carries no credential material and
    // is already implied by HOME's path, so allowlisting it widens nothing.
    "USER",
    "TMPDIR",
    "PWD",
    "LANG",
    "LC_ALL",
    "LC_CTYPE",
    // Git-only system-config suppression. Composition sets the literal value
    // for governed repository reads; ambient values are never inherited.
    "GIT_CONFIG_NOSYSTEM",
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

/// Environment names that may carry a secret into a governed child
/// (ADR-0039). Separate from [`CHILD_ENV_ALLOWLIST`]: these values are never
/// part of a plan, its digest or any receipt. Extending this list is an
/// architecture decision, not a composition detail.
pub const CHILD_SECRET_ENV_ALLOWLIST: &[&str] = &["CLAUDE_CODE_OAUTH_TOKEN"];

/// Upper bound for one child secret value, in bytes.
pub const MAX_CHILD_SECRET_BYTES: usize = 4096;

/// Late-bound secret environment for one governed child (ADR-0039).
///
/// Values live only here and are injected at spawn through
/// [`SandboxedCommand::spawn_with_secrets`]. They are deliberately outside
/// [`ChildEnvironment`], the [`IsolationPlan`], its digests and every receipt.
/// The type is not `Clone` or `PartialEq`, `Debug` prints names only, and
/// values are zeroized on drop.
///
/// Limits: `std::process::Command` keeps its own copy of each value until it
/// is dropped, and that copy is not zeroized. Same-user processes may be able
/// to read a child's environment on some platforms.
#[derive(Default)]
pub struct ChildSecretEnvironment {
    vars: Vec<(&'static str, Zeroizing<Vec<u8>>)>,
}

impl ChildSecretEnvironment {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds one secret. Fails closed on a name outside
    /// [`CHILD_SECRET_ENV_ALLOWLIST`], a duplicate name, and on an empty,
    /// NUL-containing, platform-invalid or oversized value. A refused value is
    /// zeroized before this returns.
    pub fn insert(&mut self, name: &str, value: Vec<u8>) -> Result<(), RunnerError> {
        let value = Zeroizing::new(value);
        let Some(allowed) = CHILD_SECRET_ENV_ALLOWLIST
            .iter()
            .copied()
            .find(|candidate| *candidate == name)
        else {
            return Err(RunnerError::SecretNameNotAllowed {
                name: name.to_owned(),
            });
        };
        if self.vars.iter().any(|(existing, _)| *existing == allowed) {
            return Err(RunnerError::DuplicateSecretName {
                name: allowed.to_owned(),
            });
        }
        if value.is_empty()
            || value.len() > MAX_CHILD_SECRET_BYTES
            || value.contains(&0)
            || (!cfg!(unix) && std::str::from_utf8(&value).is_err())
        {
            return Err(RunnerError::InvalidSecretValue);
        }
        self.vars.push((allowed, value));
        Ok(())
    }

    /// Names only; values are never exposed.
    pub fn names(&self) -> impl Iterator<Item = &'static str> + '_ {
        self.vars.iter().map(|(name, _)| *name)
    }
}

impl fmt::Debug for ChildSecretEnvironment {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ChildSecretEnvironment")
            .field("names", &self.names().collect::<Vec<_>>())
            .finish_non_exhaustive()
    }
}

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

/// Non-spawnable description of the child process a caller wants to launch
/// (TASK-SEC-R1 R1-H1).
///
/// It carries only a program and argv: no `std::process::Command`, no
/// environment and no `spawn`. The only way to turn it into a running child is
/// [`SandboxedLaunch::bind`], which asks the isolation backend to install its
/// restrictions first. There is therefore no un-sandboxed command object a
/// caller could spawn by forgetting a step.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CommandSpec {
    program: OsString,
    args: Vec<OsString>,
}

impl CommandSpec {
    #[must_use]
    pub fn new(program: impl Into<OsString>, args: impl IntoIterator<Item = OsString>) -> Self {
        Self {
            program: program.into(),
            args: args.into_iter().collect(),
        }
    }

    #[must_use]
    pub fn program(&self) -> &OsStr {
        &self.program
    }

    #[must_use]
    pub fn args(&self) -> &[OsString] {
        &self.args
    }
}

/// Exact, already-restricted launch recipe produced by an isolation backend.
///
/// Plain data: it can be cloned, inspected and audited, but it has no spawn
/// method of its own. Only [`SandboxedCommand`] can start the child.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LaunchRecipe {
    program: OsString,
    args: Vec<OsString>,
    environment: Vec<(OsString, OsString)>,
}

impl LaunchRecipe {
    #[must_use]
    pub fn new(
        program: impl Into<OsString>,
        args: impl IntoIterator<Item = OsString>,
        environment: impl IntoIterator<Item = (OsString, OsString)>,
    ) -> Self {
        Self {
            program: program.into(),
            args: args.into_iter().collect(),
            environment: environment.into_iter().collect(),
        }
    }

    #[must_use]
    pub fn program(&self) -> &OsStr {
        &self.program
    }

    #[must_use]
    pub fn args(&self) -> &[OsString] {
        &self.args
    }

    #[must_use]
    pub fn environment(&self) -> &[(OsString, OsString)] {
        &self.environment
    }
}

/// Replaceable backend boundary. Implementations must report only restrictions
/// actually installed for the supplied exact plan.
///
/// `sandbox` is the backend-owned launch path: it installs the backend's
/// restrictions for the exact plan *and* returns the resulting recipe. A
/// backend that cannot restrict a launch must refuse here instead of returning
/// an unrestricted recipe.
pub trait IsolationBackend: Send + Sync {
    fn identity(&self) -> BackendIdentity;

    fn enforce(&self, plan: &IsolationPlan) -> Result<EnforcementReport, RunnerError>;

    fn sandbox(
        &self,
        plan: &IsolationPlan,
        spec: &CommandSpec,
    ) -> Result<LaunchRecipe, RunnerError>;
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

/// Verified launch context for one governed process operation (TASK-SEC-R1
/// R1-H1).
///
/// It exists only after a backend produced enforcement evidence that was
/// verified against the exact plan, and it carries that backend. Callers can
/// therefore not reach a spawnable command without the backend that owns the
/// restrictions.
#[derive(Clone)]
pub struct SandboxedLaunch {
    report: EnforcementReport,
    backend: Arc<dyn IsolationBackend>,
}

impl SandboxedLaunch {
    #[must_use]
    pub fn plan(&self) -> &IsolationPlan {
        &self.report.plan
    }

    #[must_use]
    pub fn backend(&self) -> BackendIdentity {
        self.report.backend
    }

    #[must_use]
    pub const fn enforced_axes(&self) -> EnforcementAxes {
        self.report.enforced
    }

    #[must_use]
    pub const fn environment_isolated(&self) -> bool {
        self.report.env_isolated
    }

    /// Binds an exact program/argv. The backend installs its restrictions for
    /// the verified plan; the returned [`SandboxedCommand`] is the only
    /// spawnable form of that child.
    pub fn bind(&self, spec: CommandSpec) -> Result<SandboxedCommand, RunnerError> {
        let requested = spec.program().to_str();
        if requested != Some(self.report.plan.launch_executable().as_str()) {
            return Err(RunnerError::ProcessNotAuthorized {
                executable: spec.program().to_string_lossy().into_owned(),
            });
        }
        let recipe = self.backend.sandbox(&self.report.plan, &spec)?;
        if recipe.environment() != self.report.plan.environment().vars() {
            return Err(RunnerError::EnvironmentNotIsolated);
        }
        Ok(SandboxedCommand {
            plan: self.report.plan.clone(),
            recipe,
        })
    }
}

impl fmt::Debug for SandboxedLaunch {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SandboxedLaunch")
            .field("backend", &self.report.backend)
            .field("plan", &self.report.plan)
            .finish_non_exhaustive()
    }
}

/// Compiles the plan, verifies enforcement evidence, and returns the verified
/// launch context. The returned [`SandboxedLaunch`] is the only route to a
/// spawnable child; callers must carry it into the real spawn instead of
/// keeping a pre-sandbox command of their own.
pub fn prepare_process_launch(
    backend: Arc<dyn IsolationBackend>,
    effective: &EffectiveCapabilities,
    workspace: impl Into<String>,
    executable: ScopeId,
    requirements: IsolationRequirements,
    environment: ChildEnvironment,
) -> Result<SandboxedLaunch, RunnerError> {
    let plan = IsolationPlan::compile(effective, workspace, executable, requirements, environment)?;
    let report = backend.enforce(&plan)?;
    verify_report(backend.identity(), &plan, &report)?;
    Ok(SandboxedLaunch {
        report,
        backend: Arc::clone(&backend),
    })
}

/// The only spawnable form of a Runner-governed child command.
///
/// It is produced exclusively by [`SandboxedLaunch::bind`] (backend-owned
/// restrictions already installed) and exposes a narrow wiring surface: cwd,
/// stdio and process group. Program, argv and environment cannot be rewritten
/// here, and no raw `Command` escapes.
#[derive(Clone, Debug)]
pub struct SandboxedCommand {
    plan: IsolationPlan,
    recipe: LaunchRecipe,
}

impl SandboxedCommand {
    #[must_use]
    pub fn plan(&self) -> &IsolationPlan {
        &self.plan
    }

    #[must_use]
    pub fn program(&self) -> &OsStr {
        self.recipe.program()
    }

    #[must_use]
    pub fn args(&self) -> &[OsString] {
        self.recipe.args()
    }

    /// Environment installed by the backend: cleared ambient state plus
    /// exactly the plan's explicit pairs.
    #[must_use]
    pub fn environment(&self) -> &[(OsString, OsString)] {
        self.recipe.environment()
    }

    /// Spawns the already-sandboxed child. `wire` may only adjust process
    /// wiring (working directory, stdio, process group).
    pub fn spawn(
        &self,
        wire: impl FnOnce(&mut SpawnWiring<'_>),
    ) -> Result<std::process::Child, std::io::Error> {
        self.spawn_inner(None, wire)
    }

    /// Like [`Self::spawn`], additionally installing the late-bound secrets
    /// (ADR-0039) after the plan's environment. The secrets never enter the
    /// plan, the recipe or any `Debug` output of this command.
    pub fn spawn_with_secrets(
        &self,
        secrets: &ChildSecretEnvironment,
        wire: impl FnOnce(&mut SpawnWiring<'_>),
    ) -> Result<std::process::Child, std::io::Error> {
        self.spawn_inner(Some(secrets), wire)
    }

    fn spawn_inner(
        &self,
        secrets: Option<&ChildSecretEnvironment>,
        wire: impl FnOnce(&mut SpawnWiring<'_>),
    ) -> Result<std::process::Child, std::io::Error> {
        let mut command = Command::new(self.recipe.program());
        command.args(self.recipe.args());
        command.env_clear();
        for (name, value) in self.recipe.environment() {
            command.env(name, value);
        }
        if let Some(secrets) = secrets {
            for (name, value) in &secrets.vars {
                if self
                    .recipe
                    .environment()
                    .iter()
                    .any(|(planned, _)| planned.as_os_str() == OsStr::new(name))
                {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::PermissionDenied,
                        "runner refused a child secret that collides with the plan environment",
                    ));
                }
                command.env(name, secret_os_value(value));
            }
        }
        wire(&mut SpawnWiring {
            command: &mut command,
        });
        validate_spawn_cwd(&self.plan, &mut command)?;
        command.spawn()
    }
}

#[cfg(unix)]
fn secret_os_value(value: &[u8]) -> OsString {
    std::os::unix::ffi::OsStringExt::from_vec(value.to_vec())
}

#[cfg(not(unix))]
fn secret_os_value(value: &[u8]) -> OsString {
    // `ChildSecretEnvironment::insert` guarantees UTF-8 on non-unix targets.
    OsString::from(String::from_utf8_lossy(value).into_owned())
}

fn validate_spawn_cwd(plan: &IsolationPlan, command: &mut Command) -> std::io::Result<()> {
    let cwd = command.get_current_dir().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "runner requires an explicit working directory inside the prepared workspace",
        )
    })?;
    let workspace = Path::new(plan.workspace());
    if !workspace.is_absolute() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "runner refused a working directory for a non-absolute workspace",
        ));
    }
    let canonical_workspace = std::fs::canonicalize(workspace).map_err(|_| {
        std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "runner could not revalidate the workspace before spawn",
        )
    })?;
    if canonical_workspace != workspace {
        return Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "runner workspace identity changed before spawn",
        ));
    }
    let canonical_cwd = std::fs::canonicalize(cwd).map_err(|_| {
        std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "runner could not revalidate the working directory before spawn",
        )
    })?;
    if !canonical_cwd.starts_with(&canonical_workspace) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "runner refused a working directory outside the prepared workspace",
        ));
    }
    command.current_dir(canonical_cwd);
    Ok(())
}

/// Narrow wiring handle for an already-sandboxed command. It deliberately
/// exposes no spawn, no program/argv rewrite and no environment mutation.
pub struct SpawnWiring<'a> {
    command: &'a mut Command,
}

impl SpawnWiring<'_> {
    pub fn current_dir(&mut self, dir: &std::path::Path) -> &mut Self {
        self.command.current_dir(dir);
        self
    }

    pub fn stdin(&mut self, configuration: std::process::Stdio) -> &mut Self {
        self.command.stdin(configuration);
        self
    }

    pub fn stdout(&mut self, configuration: std::process::Stdio) -> &mut Self {
        self.command.stdout(configuration);
        self
    }

    pub fn stderr(&mut self, configuration: std::process::Stdio) -> &mut Self {
        self.command.stderr(configuration);
        self
    }

    /// Own process group for the sandboxed child (unix), so timeout/cancel
    /// paths can kill the whole tree.
    #[cfg(unix)]
    pub fn process_group(&mut self, process_group: i32) -> &mut Self {
        use std::os::unix::process::CommandExt;
        self.command.process_group(process_group);
        self
    }
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
    /// A child secret name outside [`CHILD_SECRET_ENV_ALLOWLIST`].
    SecretNameNotAllowed {
        name: String,
    },
    /// The same child secret name was supplied twice.
    DuplicateSecretName {
        name: String,
    },
    /// A child secret value that is empty, contains NUL, is not valid for
    /// the platform environment or exceeds [`MAX_CHILD_SECRET_BYTES`]. The
    /// value itself is never part of the error.
    InvalidSecretValue,
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
            Self::SecretNameNotAllowed { name } => {
                write!(formatter, "child secret name is not allowlisted: {name}")
            }
            Self::DuplicateSecretName { name } => {
                write!(formatter, "duplicate child secret name: {name}")
            }
            Self::InvalidSecretValue => write!(formatter, "child secret value is not acceptable"),
        }
    }
}

impl std::error::Error for RunnerError {}
