//! macOS Seatbelt runtime isolation backend.
//!
//! The backend transforms the provider child launch itself through
//! `/usr/bin/sandbox-exec`. Reports are produced only after a concrete
//! sandbox profile has been prepared for the exact launch plan.

#![cfg(target_os = "macos")]

use std::path::{Path, PathBuf};
use std::process::Command;

use runner::{
    BackendIdentity, EnforcementAxes, EnforcementReport, IsolationBackend, IsolationPlan, Platform,
    PreparedLaunch, RunnerError, RunnerKind, NETWORK_SCOPE_OUTBOUND_TCP_443,
};

#[derive(Clone, Debug, Default)]
pub struct MacOsSeatbeltRunner {
    support_read_roots: Vec<PathBuf>,
    support_write_roots: Vec<PathBuf>,
}

#[derive(Clone, Debug)]
struct SeatbeltLaunch {
    profile: String,
    interpreter: Option<&'static str>,
}

impl PreparedLaunch for SeatbeltLaunch {
    fn apply(&self, command: &mut Command) {
        let program = command.get_program().to_owned();
        let args: Vec<_> = command.get_args().map(ToOwned::to_owned).collect();
        let mut sandboxed = Command::new("/usr/bin/sandbox-exec");
        sandboxed.arg("-p").arg(&self.profile);
        if let Some(interpreter) = self.interpreter {
            sandboxed.arg(interpreter).arg(program).args(args);
        } else {
            sandboxed.arg(program).args(args);
        }
        *command = sandboxed;
    }
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
        ))
    }

    fn prepare_launch(
        &self,
        plan: &IsolationPlan,
    ) -> Result<Box<dyn PreparedLaunch + Send + Sync>, RunnerError> {
        Ok(Box::new(SeatbeltLaunch {
            profile: self.profile_for(plan)?,
            interpreter: interpreter_for(plan.launch_executable().as_str()),
        }))
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

        let mut profile = String::from(
            "(version 1)\n(deny default)\n(import \"system.sb\")\n(allow process-fork)\n",
        );

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
    let path = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
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
    use std::process::Output;
    use std::sync::atomic::{AtomicU64, Ordering};

    static COUNTER: AtomicU64 = AtomicU64::new(0);

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

        fn plan(&self, executable: &Path, network: Option<&str>) -> IsolationPlan {
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
            IsolationPlan::compile(
                &EffectiveCapabilities::new(capabilities),
                workspace,
                ScopeId::new(executable).unwrap(),
                runner::IsolationRequirements::new(true, true, true),
            )
            .unwrap()
        }
    }

    impl Drop for Case {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    fn run(backend: &MacOsSeatbeltRunner, plan: &IsolationPlan, args: &[&str]) -> Output {
        let launch = backend
            .prepare_launch(plan)
            .expect("prepare Seatbelt launch");
        let mut command = Command::new(plan.launch_executable().as_str());
        command.args(args);
        launch.apply(&mut command);
        command.output().expect("run sandbox-exec")
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
        let read_plan = case.plan(Path::new("/usr/bin/head"), None);
        let inside_result = run(&backend, &read_plan, &["-c", "1", inside.to_str().unwrap()]);
        assert!(
            inside_result.status.success(),
            "inside read failed: {:?}",
            String::from_utf8_lossy(&inside_result.stderr)
        );
        assert!(!run(
            &backend,
            &read_plan,
            &["-c", "1", outside.to_str().unwrap()]
        )
        .status
        .success());
        assert!(
            !run(&backend, &read_plan, &["-c", "1", link.to_str().unwrap()])
                .status
                .success()
        );

        let write_plan = case.plan(Path::new("/usr/bin/touch"), None);
        let inside_write = case.workspace.join("new.txt");
        assert!(
            run(&backend, &write_plan, &[inside_write.to_str().unwrap()])
                .status
                .success()
        );
        let outside_write = case.root.join("outside-write.txt");
        assert!(
            !run(&backend, &write_plan, &[outside_write.to_str().unwrap()])
                .status
                .success()
        );
        assert!(!outside_write.exists());
    }

    #[test]
    fn unauthorized_child_exec_is_denied() {
        let case = Case::new("process");
        let backend =
            MacOsSeatbeltRunner::with_support_roots(vec![PathBuf::from("/usr/bin")], Vec::new());
        let plan = case.plan(Path::new("/usr/bin/env"), None);
        let output = run(&backend, &plan, &["/usr/bin/true"]);
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
        let plan = case.plan(
            Path::new("/usr/bin/nc"),
            Some(NETWORK_SCOPE_OUTBOUND_TCP_443),
        );

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let control = TcpStream::connect(address).unwrap();
        let _ = listener.accept().unwrap();
        drop(control);
        listener.set_nonblocking(true).unwrap();

        let port = address.port().to_string();
        let output = run(&backend, &plan, &["-z", "127.0.0.1", &port]);
        assert!(!output.status.success());
        assert!(matches!(
            listener.accept(),
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock
        ));
    }

    #[test]
    fn unsupported_network_scope_fails_before_spawn() {
        let case = Case::new("network-scope");
        let plan = case.plan(Path::new("/usr/bin/true"), Some("example.com"));
        assert!(matches!(
            MacOsSeatbeltRunner::default().prepare_launch(&plan),
            Err(RunnerError::BackendUnavailable { .. })
        ));
    }
}
