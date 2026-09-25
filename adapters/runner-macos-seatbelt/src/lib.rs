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
        }
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
        // reads within its granted scope. Grant `file-read-metadata` and
        // `file-test-existence` (stat/existence only — never directory
        // contents or file data) on the exact ancestor chain of every
        // workspace/root so that resolution succeeds without widening data
        // access beyond the roots already explicitly granted above.
        let mut ancestor_literals: Vec<PathBuf> = vec![workspace.to_path_buf()];
        ancestor_literals.extend(read_roots.iter().cloned());
        ancestor_literals.extend(write_roots.iter().cloned());
        let mut ancestors: Vec<PathBuf> = ancestor_literals
            .iter()
            .flat_map(|path| path.ancestors().skip(1).map(Path::to_path_buf))
            .collect();
        ancestors.sort();
        ancestors.dedup();

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

        /// Builds the explicit isolation plan for one probe. Retained for
        /// parity with isolation_contract.rs fixtures and future negative-path
        /// assertions (TASK-SEC-R1). Not currently asserted on directly.
        #[allow(dead_code)]
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
}
