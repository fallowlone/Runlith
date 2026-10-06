//! macOS Seatbelt runtime isolation backend.
//!
//! The backend transforms the provider child launch itself through
//! `/usr/bin/sandbox-exec`. Reports are produced only after a concrete
//! sandbox profile has been prepared for the exact launch plan.

#![cfg(target_os = "macos")]

use std::path::{Path, PathBuf};

use runner::{
    BackendIdentity, CommandSpec, EnforcementAxes, EnforcementReport, IsolationBackend,
    IsolationPlan, LaunchRecipe, Platform, RunnerError, RunnerKind, NETWORK_SCOPE_OUTBOUND_TCP_443,
};
use std::ffi::OsString;

#[derive(Clone, Debug, Default)]
pub struct MacOsSeatbeltRunner {
    support_read_roots: Vec<PathBuf>,
    support_write_roots: Vec<PathBuf>,
    /// Owner-ratified 2026-09-25: whether this backend instance emits the
    /// ancestor `file-read-metadata`/`file-test-existence` ("stat only")
    /// grant a nested workspace needs for `getcwd()`/`realpath()`
    /// resolution under Seatbelt. This is a typed, construction-time flag
    /// decided only by trusted composition (`production_runner_backend`'s
    /// caller in `bins/agentd/src/execution.rs`) — never a caller/IPC
    /// string or per-launch input — and defaults to `false` (no grant,
    /// fail closed). Only candidate-input Runner launches (Git-read,
    /// snapshot-write, evaluator-read-only) set it; provider/tool launches
    /// never do. See ADR-0035 "Amendment 2026-09-25".
    grant_candidate_ancestor_metadata: bool,
}

impl MacOsSeatbeltRunner {
    #[must_use]
    pub fn with_support_roots(
        support_read_roots: Vec<PathBuf>,
        support_write_roots: Vec<PathBuf>,
    ) -> Self {
        Self {
            support_read_roots,
            support_write_roots,
            grant_candidate_ancestor_metadata: false,
        }
    }

    /// Owner-ratified 2026-09-25 (R1): opts this backend instance into the
    /// narrow ancestor metadata/existence grant. Trusted composition calls
    /// this only when building the backend for a candidate-input Runner
    /// operation; it is never derived from caller input.
    #[must_use]
    pub fn with_candidate_ancestor_metadata_grant(mut self) -> Self {
        self.grant_candidate_ancestor_metadata = true;
        self
    }
}

impl IsolationBackend for MacOsSeatbeltRunner {
    fn identity(&self) -> BackendIdentity {
        BackendIdentity {
            name: "macos-seatbelt-sandbox-exec",
            runner: RunnerKind::Native,
            platform: Platform::MacOs,
        }
    }

    fn enforce(&self, plan: &IsolationPlan) -> Result<EnforcementReport, RunnerError> {
        let _ = self.profile_for(plan)?;
        Ok(EnforcementReport::new(
            self.identity(),
            plan.clone(),
            EnforcementAxes::all(),
            true,
        ))
    }

    /// Installs the Seatbelt profile and returns the exact already-sandboxed
    /// recipe (`sandbox-exec` as the program). `sandbox-exec` execs the child
    /// with its own environment, so the plan's explicit allowlist is carried in
    /// the recipe and installed by the runner-owned spawn path.
    fn sandbox(
        &self,
        plan: &IsolationPlan,
        spec: &CommandSpec,
    ) -> Result<LaunchRecipe, RunnerError> {
        let profile = self.profile_for(plan)?;
        let program = spec.program().to_owned();
        let mut args: Vec<OsString> = vec![OsString::from("-p"), OsString::from(&profile)];
        if let Some(interpreter) = interpreter_for(plan.launch_executable().as_str()) {
            args.push(OsString::from(interpreter));
        }
        args.push(program);
        args.extend(spec.args().iter().cloned());
        Ok(LaunchRecipe::new(
            "/usr/bin/sandbox-exec",
            args,
            plan.environment().vars().to_vec(),
        ))
    }
}

impl MacOsSeatbeltRunner {
    fn profile_for(&self, plan: &IsolationPlan) -> Result<String, RunnerError> {
        let workspace = checked_absolute(Path::new(plan.workspace()))?;
        let process_paths = plan
            .process_executables()
            .iter()
            .map(|scope| checked_absolute(Path::new(scope.as_str())))
            .collect::<Result<Vec<_>, _>>()?;

        let mut read_roots = plan
            .filesystem_read_roots()
            .iter()
            .map(|root| workspace_root(workspace, root))
            .collect::<Result<Vec<_>, _>>()?;
        let mut write_roots = plan
            .filesystem_write_roots()
            .iter()
            .map(|root| workspace_root(workspace, root))
            .collect::<Result<Vec<_>, _>>()?;
        for root in &self.support_read_roots {
            read_roots.push(canonical_support_root(root)?);
        }
        for root in &self.support_write_roots {
            let root = canonical_support_root(root)?;
            read_roots.push(root.clone());
            write_roots.push(root);
        }
        read_roots.sort();
        read_roots.dedup();
        write_roots.sort();
        write_roots.dedup();

        // Absolute-path resolution (`getcwd()`/`realpath()`, which a
        // process legitimately performing `chdir`-based CWD positioning or
        // resolving a symlinked/relative reference such as a linked Git
        // worktree's `commondir` back-reference needs) walks every
        // ancestor directory of the path being resolved. A Seatbelt
        // `subpath` grant covers only the given path and its descendants —
        // never its ancestors — so a workspace/root nested several levels
        // under a narrow allowed subtree fails closed with a misleading
        // "Operation not permitted" even though the process only ever
        // reads within its granted scope.
        //
        // Owner-ratified 2026-09-25 (R1, ADR-0035 "Amendment 2026-09-25"):
        // this grant applies only to candidate-input Runner launches
        // (`grant_candidate_ancestor_metadata`, set only by trusted
        // composition for Git-read/snapshot-write/evaluator-read-only
        // operations) — never to provider/tool launches, which keep
        // exactly main's behavior. Each ancestor is emitted as an exact
        // `literal` (never `subpath`), granting `file-read-metadata` and
        // `file-test-existence` only — stat/existence, never directory
        // listing or file data — so a process can resolve its own
        // absolute path without gaining any read of ancestor contents
        // outside the roots already explicitly granted above.
        let ancestors = if self.grant_candidate_ancestor_metadata {
            let mut ancestor_sources: Vec<PathBuf> = vec![workspace.to_path_buf()];
            ancestor_sources.extend(read_roots.iter().cloned());
            ancestor_sources.extend(write_roots.iter().cloned());
            for source in &ancestor_sources {
                reject_unnormalized(source)?;
            }
            let mut ancestors: Vec<PathBuf> = ancestor_sources
                .iter()
                .flat_map(|path| path.ancestors().skip(1).map(Path::to_path_buf))
                // The root `/` itself yields no literal: granting stat on
                // the filesystem root is not meaningful ancestor-resolution
                // authority and Apple's imported `system.sb` already covers
                // it, so it is excluded rather than emitted redundantly.
                .filter(|path| path != Path::new("/"))
                .collect();
            ancestors.sort();
            ancestors.dedup();
            ancestors
        } else {
            Vec::new()
        };

        let mut profile = String::from(
            "(version 1)\n(deny default)\n(import \"system.sb\")\n(allow process-fork)\n",
        );

        if !ancestors.is_empty() {
            profile.push_str("(allow file-read-metadata file-test-existence");
            for path in &ancestors {
                push_literal(&mut profile, path)?;
            }
            profile.push_str(")\n");
        }

        profile.push_str("(allow process-exec");
        for path in &process_paths {
            push_literal(&mut profile, path)?;
        }
        if let Some(interpreter) = interpreter_for(plan.launch_executable().as_str()) {
            push_literal(&mut profile, Path::new(interpreter))?;
            if interpreter == "/bin/sh" {
                push_literal(&mut profile, Path::new("/bin/bash"))?;
            }
        }
        profile.push_str(")\n");

        profile.push_str("(allow file-read* file-test-existence file-map-executable");
        for path in &process_paths {
            push_literal(&mut profile, path)?;
        }
        if let Some(interpreter) = interpreter_for(plan.launch_executable().as_str()) {
            push_literal(&mut profile, Path::new(interpreter))?;
            if interpreter == "/bin/sh" {
                push_literal(&mut profile, Path::new("/bin/bash"))?;
            }
        }
        profile.push_str(")\n");

        if !read_roots.is_empty() {
            profile.push_str("(allow file-read* file-test-existence");
            for root in &read_roots {
                push_subpath(&mut profile, root)?;
            }
            profile.push_str(")\n");
        }
        if !write_roots.is_empty() {
            profile.push_str("(allow file-write*");
            for root in &write_roots {
                push_subpath(&mut profile, root)?;
            }
            profile.push_str(")\n");
        }

        match plan.network_destinations() {
            [] => {}
            destinations
                if destinations.len() == 1
                    && destinations[0].as_str() == NETWORK_SCOPE_OUTBOUND_TCP_443 =>
            {
                profile.push_str(
                    "(allow network-outbound\n  (literal \"/private/var/run/mDNSResponder\")\n  (remote tcp \"*:443\"))\n",
                );
            }
            _ => {
                return Err(RunnerError::BackendUnavailable {
                    backend: "macOS Seatbelt cannot enforce requested network scope",
                });
            }
        }

        Ok(profile)
    }
}

fn workspace_root(workspace: &Path, root: &Path) -> Result<PathBuf, RunnerError> {
    if root.is_absolute() {
        return Err(RunnerError::BackendUnavailable {
            backend: "macOS Seatbelt requires relative workspace roots",
        });
    }
    if root == Path::new(".") {
        return Ok(workspace.to_path_buf());
    }
    let path = workspace.join(root);
    checked_absolute(&path)?;
    Ok(path)
}

fn canonical_support_root(path: &Path) -> Result<PathBuf, RunnerError> {
    let path = std::fs::canonicalize(path).map_err(|_| RunnerError::BackendUnavailable {
        backend: "macOS Seatbelt support root cannot be canonicalized",
    })?;
    checked_absolute(&path)?;
    Ok(path)
}

fn checked_absolute(path: &Path) -> Result<&Path, RunnerError> {
    let text = path.to_str().ok_or(RunnerError::BackendUnavailable {
        backend: "macOS Seatbelt requires UTF-8 paths",
    })?;
    if !path.is_absolute()
        || text.contains('"')
        || text.contains('\\')
        || text.chars().any(char::is_control)
    {
        return Err(RunnerError::BackendUnavailable {
            backend: "macOS Seatbelt requires safe absolute paths",
        });
    }
    Ok(path)
}

/// R1(b): fails closed on any input whose ancestor chain could be
/// ambiguous or escape-prone — a `..`/`.` component (never expected from
/// an already-canonicalized source, but never trusted blindly here), a
/// non-absolute path, or a doubled separator (`//`) that could make a
/// naive ancestor walk disagree with the kernel's own resolution. Ancestor
/// literals are computed only from paths that pass this check.
fn reject_unnormalized(path: &Path) -> Result<(), RunnerError> {
    let checked = checked_absolute(path)?;
    let text = checked.to_str().ok_or(RunnerError::BackendUnavailable {
        backend: "macOS Seatbelt requires UTF-8 paths",
    })?;
    if text.contains("//") {
        return Err(RunnerError::BackendUnavailable {
            backend: "macOS Seatbelt requires normalized ancestor source paths",
        });
    }
    for component in checked.components() {
        match component {
            std::path::Component::RootDir | std::path::Component::Normal(_) => {}
            std::path::Component::ParentDir
            | std::path::Component::CurDir
            | std::path::Component::Prefix(_) => {
                return Err(RunnerError::BackendUnavailable {
                    backend: "macOS Seatbelt requires normalized ancestor source paths",
                });
            }
        }
    }
    Ok(())
}

fn push_literal(profile: &mut String, path: &Path) -> Result<(), RunnerError> {
    let path = checked_absolute(path)?;
    profile.push_str("\n  (literal \"");
    profile.push_str(path.to_str().expect("checked UTF-8 path"));
    profile.push_str("\")");
    Ok(())
}

fn push_subpath(profile: &mut String, path: &Path) -> Result<(), RunnerError> {
    let path = checked_absolute(path)?;
    profile.push_str("\n  (subpath \"");
    profile.push_str(path.to_str().expect("checked UTF-8 path"));
    profile.push_str("\")");
    Ok(())
}

fn interpreter_for(executable: &str) -> Option<&'static str> {
    let bytes = std::fs::read(Path::new(executable)).ok()?;
    let first_line = bytes.split(|byte| *byte == b'\n').next()?;
    let line = std::str::from_utf8(first_line).ok()?;
    if line == "#!/bin/sh" || line.starts_with("#!/bin/sh ") {
        Some("/bin/sh")
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use policy::{Capability, EffectiveCapabilities, FileScope, ScopeId};
    use runner::{prepare_process_launch, ChildEnvironment, SandboxedLaunch};
    use std::process::{Output, Stdio};
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::Arc;

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    /// Explicit minimal child environment for sandbox probes: only the
    /// allowlisted basics a governed child may see (TASK-SEC-R1 R1-H2).
    fn child_environment() -> ChildEnvironment {
        ChildEnvironment::minimal_allowlist().expect("runner minimal allowlist")
    }

    struct Case {
        root: PathBuf,
        workspace: PathBuf,
    }

    impl Case {
        fn new(name: &str) -> Self {
            let id = COUNTER.fetch_add(1, Ordering::SeqCst);
            let root = std::env::temp_dir().join(format!(
                "runlith-seatbelt-{name}-{id}-{}",
                std::process::id()
            ));
            let workspace = root.join("work");
            std::fs::create_dir_all(&workspace).unwrap();
            let root = std::fs::canonicalize(root).unwrap();
            let workspace = std::fs::canonicalize(workspace).unwrap();
            Self { root, workspace }
        }

        fn authority(
            &self,
            executable: &Path,
            network: Option<&str>,
        ) -> (EffectiveCapabilities, String, ScopeId) {
            let workspace = self.workspace.to_string_lossy().into_owned();
            let executable = executable.to_string_lossy().into_owned();
            let mut capabilities = vec![
                Capability::FilesystemRead(FileScope::new(&workspace, ".").unwrap()),
                Capability::FilesystemWrite(FileScope::new(&workspace, ".").unwrap()),
                Capability::ProcessExecute(ScopeId::new(&executable).unwrap()),
            ];
            if let Some(network) = network {
                capabilities.push(Capability::network(network).unwrap());
            }
            (
                EffectiveCapabilities::new(capabilities),
                workspace,
                ScopeId::new(executable).unwrap(),
            )
        }

        /// Builds the explicit isolation plan for one probe.
        fn plan(&self, executable: &Path, network: Option<&str>) -> IsolationPlan {
            let (effective, workspace, launch_executable) = self.authority(executable, network);
            IsolationPlan::compile(
                &effective,
                workspace,
                launch_executable,
                runner::IsolationRequirements::new(true, true, true),
                child_environment(),
            )
            .unwrap()
        }
    }

    impl Drop for Case {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    /// Verifies enforcement and returns the only spawnable launch context
    /// (TASK-SEC-R1 R1-H1).
    fn prepare(
        backend: &MacOsSeatbeltRunner,
        case: &Case,
        executable: &Path,
        network: Option<&str>,
    ) -> Result<SandboxedLaunch, RunnerError> {
        let (effective, workspace, launch_executable) = case.authority(executable, network);
        prepare_process_launch(
            Arc::new(backend.clone()),
            &effective,
            workspace,
            launch_executable,
            runner::IsolationRequirements::new(true, true, true),
            child_environment(),
        )
    }

    /// Runs one child through the backend-owned sandbox path: the command is
    /// only spawnable after the backend installed its restrictions.
    fn run(
        backend: &MacOsSeatbeltRunner,
        case: &Case,
        executable: &Path,
        network: Option<&str>,
        args: &[&str],
    ) -> Output {
        let launch = prepare(backend, case, executable, network).expect("verifiable sandbox plan");
        let sandboxed = launch
            .bind(CommandSpec::new(
                executable,
                args.iter().map(std::ffi::OsString::from),
            ))
            .expect("backend installs a sandboxed recipe");
        let child = sandboxed
            .spawn(|wire| {
                wire.current_dir(&case.workspace)
                    .stdin(Stdio::null())
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped());
            })
            .expect("spawn sandboxed child");
        child.wait_with_output().expect("collect child output")
    }

    #[test]
    fn filesystem_scope_blocks_direct_and_symlink_escape() {
        use std::os::unix::fs::symlink;

        let case = Case::new("filesystem");
        let outside = case.root.join("outside.txt");
        let inside = case.workspace.join("inside.txt");
        let link = case.workspace.join("escape.txt");
        std::fs::write(&outside, b"secret").unwrap();
        std::fs::write(&inside, b"inside").unwrap();
        symlink(&outside, &link).unwrap();

        let backend = MacOsSeatbeltRunner::default();
        let inside_result = run(
            &backend,
            &case,
            Path::new("/usr/bin/head"),
            None,
            &["-c", "1", inside.to_str().unwrap()],
        );
        assert!(
            inside_result.status.success(),
            "inside read failed: {:?}",
            String::from_utf8_lossy(&inside_result.stderr)
        );
        assert!(!run(
            &backend,
            &case,
            Path::new("/usr/bin/head"),
            None,
            &["-c", "1", outside.to_str().unwrap()]
        )
        .status
        .success());
        assert!(!run(
            &backend,
            &case,
            Path::new("/usr/bin/head"),
            None,
            &["-c", "1", link.to_str().unwrap()]
        )
        .status
        .success());

        let inside_write = case.workspace.join("new.txt");
        assert!(run(
            &backend,
            &case,
            Path::new("/usr/bin/touch"),
            None,
            &[inside_write.to_str().unwrap()]
        )
        .status
        .success());
        let outside_write = case.root.join("outside-write.txt");
        assert!(!run(
            &backend,
            &case,
            Path::new("/usr/bin/touch"),
            None,
            &[outside_write.to_str().unwrap()]
        )
        .status
        .success());
        assert!(!outside_write.exists());
    }

    #[test]
    fn unauthorized_child_exec_is_denied() {
        let case = Case::new("process");
        let backend =
            MacOsSeatbeltRunner::with_support_roots(vec![PathBuf::from("/usr/bin")], Vec::new());
        let output = run(
            &backend,
            &case,
            Path::new("/usr/bin/env"),
            None,
            &["/usr/bin/true"],
        );
        assert!(!output.status.success());
        assert!(
            String::from_utf8_lossy(&output.stderr).contains("Operation not permitted"),
            "unexpected stderr: {:?}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    #[test]
    fn network_scope_denies_unauthorized_remote_port() {
        use std::net::{TcpListener, TcpStream};

        let case = Case::new("network");
        let backend = MacOsSeatbeltRunner::default();

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let control = TcpStream::connect(address).unwrap();
        let _ = listener.accept().unwrap();
        drop(control);
        listener.set_nonblocking(true).unwrap();

        let port = address.port().to_string();
        let output = run(
            &backend,
            &case,
            Path::new("/usr/bin/nc"),
            Some(NETWORK_SCOPE_OUTBOUND_TCP_443),
            &["-z", "127.0.0.1", &port],
        );
        assert!(!output.status.success());
        assert!(matches!(
            listener.accept(),
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock
        ));
    }

    /// ADR-0040 addendum: pins this platform fact — `sandbox-exec`
    /// accepts only `*` or `localhost` as the host in `(remote tcp …)`.
    /// IP literals are rejected at profile-parse time (exit 65), which is
    /// why per-IP destination pinning is not implemented. Fails loudly if
    /// Apple ever relaxes the grammar, which is exactly when pinning
    /// becomes worth revisiting.
    #[test]
    fn seatbelt_remote_tcp_accepts_only_star_or_localhost() {
        use std::process::Command;

        fn probe(remote: &str) -> std::process::Output {
            let dir = std::env::temp_dir()
                .join(format!("runlith-seatbelt-grammar-{}", std::process::id()));
            std::fs::create_dir_all(&dir).unwrap();
            let profile = dir.join("probe.sb");
            std::fs::write(
                &profile,
                format!(
                    "(version 1)\n(allow default)\n(allow network-outbound\n  (remote tcp \"{remote}\"))\n"
                ),
            )
            .unwrap();
            let output = Command::new("/usr/bin/sandbox-exec")
                .arg("-f")
                .arg(&profile)
                .arg("/usr/bin/true")
                .output()
                .unwrap();
            std::fs::remove_dir_all(&dir).ok();
            output
        }

        let ip = probe("93.184.216.34:443");
        assert!(!ip.status.success());
        assert!(
            String::from_utf8_lossy(&ip.stderr).contains("host must be * or localhost"),
            "unexpected stderr: {:?}",
            String::from_utf8_lossy(&ip.stderr)
        );
        assert!(probe("localhost:443").status.success());
        assert!(probe("*:443").status.success());
    }

    #[test]
    fn unsupported_network_scope_fails_before_spawn() {
        let case = Case::new("network-scope");
        assert!(matches!(
            prepare(
                &MacOsSeatbeltRunner::default(),
                &case,
                Path::new("/usr/bin/true"),
                Some("example.com")
            ),
            Err(RunnerError::BackendUnavailable { .. })
        ));
    }

    #[test]
    fn missing_support_root_fails_closed() {
        let case = Case::new("missing-support-root");
        let missing = case.root.join("missing-provider-support");
        let backend = MacOsSeatbeltRunner::with_support_roots(vec![missing], Vec::new());

        assert!(matches!(
            prepare(&backend, &case, Path::new("/usr/bin/true"), None),
            Err(RunnerError::BackendUnavailable { .. })
        ));
    }

    /// TASK-SEC-R1 R1-H1 (adversarial, real sandboxed child): the backend-owned
    /// spawn path must produce a child that is actually confined. The probe
    /// reads a file outside the plan's filesystem scope through the very path
    /// production uses, and the sandbox must refuse it.
    #[test]
    fn r1_h1_child_of_seatbelt_backend_is_sandboxed() {
        let case = Case::new("h1-probe");
        let outside = case.root.join("outside-probe.txt");
        std::fs::write(&outside, b"secret").unwrap();
        let backend = MacOsSeatbeltRunner::default();

        let launch =
            prepare(&backend, &case, Path::new("/usr/bin/head"), None).expect("sandboxed plan");
        assert_eq!(launch.backend().name, "macos-seatbelt-sandbox-exec");
        let sandboxed = launch
            .bind(CommandSpec::new(
                Path::new("/usr/bin/head"),
                [
                    std::ffi::OsString::from("-c"),
                    std::ffi::OsString::from("1"),
                    outside.as_os_str().to_owned(),
                ],
            ))
            .expect("sandboxed recipe");
        assert_eq!(sandboxed.program(), Path::new("/usr/bin/sandbox-exec"));
        assert!(!sandboxed
            .args()
            .iter()
            .any(|arg| arg == "/usr/bin/sandbox-exec"));

        let child = sandboxed
            .spawn(|wire| {
                wire.current_dir(&case.workspace)
                    .stdin(Stdio::null())
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped());
            })
            .expect("spawn sandboxed child");
        let output = child.wait_with_output().expect("wait for sandboxed child");
        assert!(
            !output.status.success(),
            "child spawned through the backend escaped the sandbox: {:?}",
            String::from_utf8_lossy(&output.stdout)
        );
    }

    /// TASK-SEC-R1 R1-H2 (adversarial, real sandboxed child): a poisoned
    /// ambient environment must not reach the child through `sandbox-exec`,
    /// which the Seatbelt profile cannot restrict by itself.
    #[test]
    fn r1_h2_sandboxed_child_env_cannot_see_poisoned_vars() {
        const CANARY: &str = "RUNLITH_SEC_CANARY_AWS_SECRET_ACCESS_KEY";
        std::env::set_var(CANARY, "poison-value");

        let case = Case::new("env-isolation");
        let backend = MacOsSeatbeltRunner::default();
        let output = run(&backend, &case, Path::new("/usr/bin/env"), None, &[]);
        std::env::remove_var(CANARY);

        assert!(
            output.status.success(),
            "sandboxed env probe failed: {:?}",
            String::from_utf8_lossy(&output.stderr)
        );
        let observed = String::from_utf8_lossy(&output.stdout);
        let names: Vec<&str> = observed
            .lines()
            .map(|line| line.split('=').next().unwrap_or_default())
            .collect();
        assert!(
            !names.contains(&CANARY),
            "sandboxed child inherited a poisoned ambient variable: {observed}"
        );
        assert!(
            names
                .iter()
                .all(|name| runner::CHILD_ENV_ALLOWLIST.contains(name)),
            "sandboxed child env exposed a non-allowlisted name: {observed}"
        );
    }

    /// TASK-0144 R1(c) (owner-ratified 2026-09-25, adversarial, real
    /// sandboxed child): mirrors `filesystem_scope_blocks_direct_and_symlink_escape`
    /// for the candidate-input ancestor grant. `case.root` is the direct
    /// parent of `case.workspace`, exactly the ancestor the grant covers:
    /// stat succeeds, but listing the ancestor's entries and reading file
    /// data placed directly in it (outside the authorized workspace
    /// subtree) must both still fail.
    #[test]
    fn candidate_ancestor_grant_permits_stat_but_denies_listing_and_data() {
        let case = Case::new("candidate-ancestor");
        let secret = case.root.join("ancestor-secret.txt");
        std::fs::write(&secret, b"secret").unwrap();

        let backend = MacOsSeatbeltRunner::default().with_candidate_ancestor_metadata_grant();

        let stat = run(
            &backend,
            &case,
            Path::new("/usr/bin/stat"),
            None,
            &[case.root.to_str().unwrap()],
        );
        assert!(
            stat.status.success(),
            "a candidate launch must be able to stat an ancestor directory: {:?}",
            String::from_utf8_lossy(&stat.stderr)
        );

        let listing = run(
            &backend,
            &case,
            Path::new("/bin/ls"),
            None,
            &[case.root.to_str().unwrap()],
        );
        assert!(
            !listing.status.success(),
            "a candidate launch must not be able to list an ancestor directory's entries"
        );

        let data = run(
            &backend,
            &case,
            Path::new("/usr/bin/head"),
            None,
            &["-c", "1", secret.to_str().unwrap()],
        );
        assert!(
            !data.status.success(),
            "a candidate launch must not read file data placed directly in an ancestor"
        );
    }

    /// TASK-0144 R1(c) (owner-ratified 2026-09-25): a provider/tool launch
    /// never opts into the ancestor grant — the emitted profile carries no
    /// ancestor metadata rule at all, and behavior stays exactly what main
    /// produces (ancestor stat denied).
    #[test]
    fn provider_launch_has_no_ancestor_grant_in_profile_or_behavior() {
        let case = Case::new("provider-ancestor");
        let backend = MacOsSeatbeltRunner::default();

        let plan = case.plan(Path::new("/usr/bin/stat"), None);
        let profile = backend.profile_for(&plan).expect("provider profile");
        assert!(
            !profile.contains("file-read-metadata"),
            "a provider launch profile must carry no ancestor metadata grant: {profile}"
        );

        let stat = run(
            &backend,
            &case,
            Path::new("/usr/bin/stat"),
            None,
            &[case.root.to_str().unwrap()],
        );
        assert!(
            !stat.status.success(),
            "a provider launch must not gain ancestor stat access (matches main)"
        );
    }
}
