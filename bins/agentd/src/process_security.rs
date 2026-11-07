pub fn harden() -> std::io::Result<()> {
    use rustix::process::{getrlimit, setrlimit, Resource, Rlimit};

    let disabled = Rlimit {
        current: Some(0),
        maximum: Some(0),
    };
    if getrlimit(Resource::Core) != disabled {
        setrlimit(Resource::Core, disabled).map_err(|error| {
            std::io::Error::other(format!("cannot disable process core dumps: {error}"))
        })?;
    }
    #[cfg(target_os = "macos")]
    deny_debugger_attach()?;
    Ok(())
}

#[cfg(target_os = "macos")]
fn deny_debugger_attach() -> std::io::Result<()> {
    #[cfg(debug_assertions)]
    if std::env::var_os("RUNLITH_ALLOW_DEBUGGER").as_deref() == Some(std::ffi::OsStr::new("1")) {
        return Ok(());
    }
    // SAFETY: PT_DENY_ATTACH affects this process and ignores the PID/address/data arguments.
    if unsafe { libc::ptrace(libc::PT_DENY_ATTACH, 0, std::ptr::null_mut(), 0) } != 0 {
        return Err(std::io::Error::other(format!(
            "cannot deny debugger attach: {}",
            std::io::Error::last_os_error()
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use rustix::process::{getrlimit, setrlimit, Resource, Rlimit};
    use std::process::Command;

    const FIXTURE: &str = "process_security::tests::subprocess";
    const CASE: &str = "RUNLITH_CORE_DUMP_TEST_CASE";

    fn spawn_fixture(case: &str) {
        let mut command = Command::new(std::env::current_exe().unwrap());
        command
            .args(["--ignored", "--exact", FIXTURE, "--nocapture"])
            .env_clear()
            .env(CASE, case);
        if case == "debug-helper" {
            command.env("RUNLITH_ALLOW_DEBUGGER", "1");
        }
        let output = command.output().unwrap();
        assert!(
            output.status.success(),
            "core-dump fixture {case}: {}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    #[test]
    fn daemon_disables_core_dumps_before_endpoint_admission() {
        spawn_fixture("daemon");
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn helper_disables_core_dumps_before_authentication() {
        spawn_fixture("helper");
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn debugger_opt_out_does_not_enable_core_dumps() {
        spawn_fixture("debug-helper");
    }

    #[test]
    #[ignore = "subprocess fixture; invoked by core-dump startup tests"]
    fn subprocess() {
        let case = std::env::var(CASE).unwrap();
        if case != "inherited" {
            let enabled = Rlimit {
                current: Some(4096),
                maximum: Some(4096),
            };
            setrlimit(Resource::Core, enabled).unwrap();
            assert_eq!(getrlimit(Resource::Core), enabled);
        }
        match case.as_str() {
            "daemon" => {
                use std::os::unix::fs::PermissionsExt;
                let root = tempfile::tempdir().unwrap();
                std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700))
                    .unwrap();
                let endpoint = ipc_transport::platform::unix::UnixEndpoint {
                    socket_path: root.path().join("agentd.sock"),
                    runtime_dir: root.path().to_owned(),
                };
                std::fs::write(&endpoint.socket_path, b"occupied regular file").unwrap();
                let runtime = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .unwrap();
                let result = runtime.block_on(crate::run_on_endpoint(
                    &endpoint,
                    crate::DaemonConfig::g1_default(),
                ));
                assert!(
                    matches!(result, Err(crate::DaemonError::EndpointSecurity(_))),
                    "unexpected admission result: {result:?}"
                );
            }
            #[cfg(target_os = "macos")]
            "helper" | "debug-helper" => assert_eq!(
                crate::run_candidate_helper(&["__candidate-evaluate".to_owned()]),
                std::process::ExitCode::FAILURE
            ),
            "inherited" => crate::harden_process().unwrap(),
            _ => panic!("unknown core-dump fixture"),
        }
        assert_eq!(
            getrlimit(Resource::Core),
            Rlimit {
                current: Some(0),
                maximum: Some(0)
            }
        );
        assert!(setrlimit(
            Resource::Core,
            Rlimit {
                current: Some(1),
                maximum: Some(1)
            }
        )
        .is_err());
        if case != "inherited" {
            spawn_fixture("inherited");
        }
    }
}
