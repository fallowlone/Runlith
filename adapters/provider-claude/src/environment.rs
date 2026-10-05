//! Controlled provider-process environment (TASK-0065, G6 H7).
//!
//! Canonical authority still comes from Policy/Runner. This module only
//! removes ambient process state and moves provider-writable runtime state
//! outside the managed Git worktree.

use std::ffi::{OsStr, OsString};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
#[cfg(any(test, feature = "trusted-host"))]
use std::process::Command;

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

/// Exact provider environment prepared for one managed worktree.
#[derive(Debug, Clone)]
pub struct ProviderEnvironment {
    workdir: PathBuf,
    runtime_root: PathBuf,
    xdg_config_home: PathBuf,
    xdg_data_home: PathBuf,
    xdg_state_home: PathBuf,
    xdg_cache_home: PathBuf,
    scratch_dir: PathBuf,
    /// Private `HOME` handed to a governed (runner-path) child.
    private_home: PathBuf,
    /// Private `CLAUDE_CONFIG_DIR` handed to a governed child.
    claude_config_dir: PathBuf,
    path: OsString,
    /// Host `HOME`; used only by the feature-gated trusted-host supervisor.
    home: Option<OsString>,
    user: Option<OsString>,
    #[cfg(windows)]
    user_profile: Option<OsString>,
    #[cfg(windows)]
    system_root: Option<OsString>,
    #[cfg(windows)]
    path_ext: Option<OsString>,
}

impl ProviderEnvironment {
    /// Prepares deterministic provider state for `workdir` under
    /// `runtime_base/<worktree-leaf>`. The base must be absolute and must not
    /// live inside the managed worktree. Missing/unsafe roots fail closed.
    pub fn prepare(workdir: &Path, runtime_base: &Path) -> Result<Self, String> {
        Self::prepare_from(workdir, runtime_base, &|name| std::env::var_os(name))
    }

    /// [`Self::prepare`] with the host variables (PATH, HOME, USER, ...)
    /// resolved through `host` instead of the ambient process environment.
    /// The requirements are identical and absent names never fall back to
    /// the ambient environment (TASK-0155).
    pub(crate) fn prepare_from(
        workdir: &Path,
        runtime_base: &Path,
        host: &dyn Fn(&str) -> Option<OsString>,
    ) -> Result<Self, String> {
        let host_var = |name: &str| host(name).filter(|value| !value.is_empty());
        if !workdir.is_absolute() {
            return Err("provider workdir must be absolute".to_owned());
        }
        if !runtime_base.is_absolute() {
            return Err("provider environment base must be absolute".to_owned());
        }
        let leaf = workdir.file_name().ok_or_else(|| {
            "provider workdir has no terminal component for environment identity".to_owned()
        })?;
        if leaf.is_empty() || leaf == OsStr::new(".") || leaf == OsStr::new("..") {
            return Err("provider workdir has invalid environment identity".to_owned());
        }

        let runtime_root = runtime_base.join(leaf);
        if runtime_root.starts_with(workdir) || workdir.starts_with(&runtime_root) {
            return Err("provider environment must be outside managed worktree".to_owned());
        }

        ensure_private_dir(runtime_base)?;
        ensure_private_dir(&runtime_root)?;
        let xdg_config_home = private_child(&runtime_root, "config")?;
        let xdg_data_home = private_child(&runtime_root, "data")?;
        let xdg_state_home = private_child(&runtime_root, "state")?;
        let xdg_cache_home = private_child(&runtime_root, "cache")?;
        let scratch_dir = private_child(&runtime_root, "scratch")?;
        let private_home = private_child(&runtime_root, "home")?;
        let claude_config_dir = private_child(&runtime_root, "claude")?;

        if workdir.exists() {
            let canonical_workdir = fs::canonicalize(workdir)
                .map_err(|_| "provider workdir cannot be canonicalized".to_owned())?;
            let canonical_runtime = fs::canonicalize(&runtime_root)
                .map_err(|_| "provider environment cannot be canonicalized".to_owned())?;
            if canonical_runtime.starts_with(&canonical_workdir)
                || canonical_workdir.starts_with(&canonical_runtime)
            {
                return Err("provider environment overlaps managed worktree".to_owned());
            }
        }

        let path = host_var("PATH").ok_or("provider environment requires PATH")?;
        let home = host_var("HOME");
        #[cfg(unix)]
        if home.is_none() {
            return Err(
                "provider environment requires HOME for provider-owned authentication".to_owned(),
            );
        }
        let user = host_var("USER");
        // Fail closed here rather than letting the child discover it: without
        // the login principal the provider cannot resolve its own stored
        // credentials, and it reports that as a bare exit 1 with no stderr,
        // which is indistinguishable from an unrelated launch failure.
        #[cfg(unix)]
        if user.is_none() {
            return Err(
                "provider environment requires USER for provider-owned authentication".to_owned(),
            );
        }

        Ok(Self {
            workdir: workdir.to_path_buf(),
            runtime_root,
            xdg_config_home,
            xdg_data_home,
            xdg_state_home,
            xdg_cache_home,
            scratch_dir,
            private_home,
            claude_config_dir,
            path,
            home,
            user,
            #[cfg(windows)]
            user_profile: host_var("USERPROFILE"),
            #[cfg(windows)]
            system_root: host_var("SystemRoot").or_else(|| host_var("WINDIR")),
            #[cfg(windows)]
            path_ext: host_var("PATHEXT"),
        })
    }

    /// Applies an allowlisted environment to the exact child command.
    /// Ambient API keys, cloud credentials, Git overrides, SSH agents and
    /// provider/plugin variables are intentionally not inherited.
    #[cfg(any(test, feature = "trusted-host"))]
    pub(crate) fn apply(&self, command: &mut Command) {
        command.env_clear();
        for (name, value) in self.resolved_pairs(true) {
            command.env(name, value);
        }
    }

    /// The exact explicit variable set a governed child receives, as
    /// composition-resolved name/value pairs (TASK-SEC-R1 R1-H2).
    ///
    /// The Runner plans this set as the enforced child environment so the
    /// provider no longer owns the only copy of the policy: whatever a
    /// governed child sees is visible to the isolation plan and can be
    /// asserted against it.
    #[must_use]
    ///
    /// A governed child never sees the host `HOME`: it gets the private
    /// `HOME` and `CLAUDE_CONFIG_DIR` under the runtime root (ADR-0039).
    pub fn child_environment(&self) -> Vec<(OsString, OsString)> {
        self.resolved_pairs(false)
    }

    /// `use_host_home` is true only for the feature-gated trusted-host
    /// supervisor, which has no sandbox and keeps provider-owned host login.
    fn resolved_pairs(&self, use_host_home: bool) -> Vec<(OsString, OsString)> {
        let mut pairs: Vec<(OsString, OsString)> = Vec::new();
        pairs.push((OsString::from("PATH"), self.path.clone()));
        if use_host_home {
            if let Some(home) = &self.home {
                pairs.push((OsString::from("HOME"), home.clone()));
            }
        } else {
            pairs.push((
                OsString::from("HOME"),
                self.private_home.as_os_str().to_owned(),
            ));
            pairs.push((
                OsString::from("CLAUDE_CONFIG_DIR"),
                self.claude_config_dir.as_os_str().to_owned(),
            ));
        }
        if let Some(user) = &self.user {
            pairs.push((OsString::from("USER"), user.clone()));
        }
        pairs.push((OsString::from("PWD"), self.workdir.as_os_str().to_owned()));
        for (name, value) in [
            ("XDG_CONFIG_HOME", &self.xdg_config_home),
            ("XDG_DATA_HOME", &self.xdg_data_home),
            ("XDG_STATE_HOME", &self.xdg_state_home),
            ("XDG_CACHE_HOME", &self.xdg_cache_home),
        ] {
            pairs.push((OsString::from(name), value.as_os_str().to_owned()));
        }
        pairs.push((
            OsString::from("CLAUDE_CODE_TMPDIR"),
            self.scratch_dir.as_os_str().to_owned(),
        ));
        for name in ["TMPDIR", "TMP", "TEMP"] {
            pairs.push((
                OsString::from(name),
                self.scratch_dir.as_os_str().to_owned(),
            ));
        }
        for name in ["LANG", "LC_ALL", "LC_CTYPE"] {
            if let Some(value) = non_empty_env(name) {
                pairs.push((OsString::from(name), value));
            }
        }

        #[cfg(windows)]
        {
            if use_host_home {
                if let Some(profile) = &self.user_profile {
                    pairs.push((OsString::from("USERPROFILE"), profile.clone()));
                }
            } else {
                pairs.push((
                    OsString::from("USERPROFILE"),
                    self.private_home.as_os_str().to_owned(),
                ));
            }
            if let Some(root) = &self.system_root {
                pairs.push((OsString::from("SystemRoot"), root.clone()));
                pairs.push((OsString::from("WINDIR"), root.clone()));
            }
            if let Some(path_ext) = &self.path_ext {
                pairs.push((OsString::from("PATHEXT"), path_ext.clone()));
            }
            pairs.push((
                OsString::from("APPDATA"),
                self.xdg_config_home.as_os_str().to_owned(),
            ));
            pairs.push((
                OsString::from("LOCALAPPDATA"),
                self.xdg_cache_home.as_os_str().to_owned(),
            ));
        }

        pairs
    }

    #[must_use]
    pub fn runtime_root(&self) -> &Path {
        &self.runtime_root
    }

    #[must_use]
    pub fn xdg_data_home(&self) -> &Path {
        &self.xdg_data_home
    }

    #[must_use]
    pub fn scratch_dir(&self) -> &Path {
        &self.scratch_dir
    }

    /// Provider-owned paths the OS sandbox may read in addition to the task
    /// workspace: only the isolated runtime root. Host authentication anchors
    /// (`~/.claude`, `~/.claude.json`) are deliberately not granted
    /// (ADR-0039); the credential arrives through the Secrets Broker.
    pub fn isolation_support_read_roots(&self) -> Result<Vec<PathBuf>, String> {
        Ok(vec![self.runtime_root.clone()])
    }

    /// Provider-writable sandbox support is limited to the isolated runtime
    /// root, which contains the private `HOME` and `CLAUDE_CONFIG_DIR`.
    #[must_use]
    pub fn isolation_support_write_roots(&self) -> Vec<PathBuf> {
        vec![self.runtime_root.clone()]
    }
}

fn non_empty_env(name: &str) -> Option<OsString> {
    std::env::var_os(name).filter(|value| !value.is_empty())
}

fn private_child(parent: &Path, name: &str) -> Result<PathBuf, String> {
    let path = parent.join(name);
    ensure_private_dir(&path)?;
    Ok(path)
}

fn ensure_private_dir(path: &Path) -> Result<(), String> {
    let _created = match fs::symlink_metadata(path) {
        Ok(metadata) => {
            if metadata.file_type().is_symlink() || !metadata.is_dir() {
                return Err("provider environment path is not a real directory".to_owned());
            }
            false
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            fs::create_dir_all(path)
                .map_err(|_| "provider environment directory cannot be created".to_owned())?;
            true
        }
        Err(_) => return Err("provider environment path cannot be inspected".to_owned()),
    };

    #[cfg(unix)]
    {
        if _created {
            fs::set_permissions(path, fs::Permissions::from_mode(0o700))
                .map_err(|_| "provider environment permissions cannot be set".to_owned())?;
        }
        let metadata = fs::symlink_metadata(path)
            .map_err(|_| "provider environment permissions cannot be inspected".to_owned())?;
        if metadata.permissions().mode() & 0o7777 != 0o700 {
            return Err("provider environment directory must have mode 0700".to_owned());
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn child_environment_carries_the_authentication_principal() {
        // HOME alone does not authenticate the provider: the CLI resolves its
        // stored credentials through the OS login principal, so a child that
        // receives HOME without USER reports "Not logged in" and exits 1 with
        // empty stderr. The adapter's own contract (`prepare` requires HOME
        // "for provider-owned authentication") is only met when both pass.
        let root = std::env::temp_dir().join(format!(
            "runlith-provider-env-principal-{}",
            std::process::id()
        ));
        let workdir = root.join("work");
        fs::create_dir_all(&workdir).unwrap();
        let environment = ProviderEnvironment::prepare(&workdir, &root.join("provider"))
            .expect("provider environment");
        let pairs = environment.child_environment();
        let names: Vec<&std::ffi::OsStr> = pairs.iter().map(|(name, _)| name.as_os_str()).collect();
        assert!(
            names.contains(&std::ffi::OsStr::new("USER")),
            "child environment must carry USER for provider-owned authentication; got {names:?}"
        );
        let user = pairs
            .iter()
            .find(|(name, _)| name == std::ffi::OsStr::new("USER"))
            .map(|(_, value)| value.clone())
            .expect("USER pair");
        assert_eq!(
            user,
            std::env::var_os("USER").expect("host USER"),
            "USER must be the real login principal, not a synthesized value"
        );
        let _ = fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    #[test]
    fn explicit_host_environment_keeps_the_fail_closed_principal_requirement() {
        // TASK-0155: an explicit host environment only replaces the lookup
        // source; it must not relax the HOME/USER requirement, and it must
        // not fall back to the ambient process environment.
        let root = std::env::temp_dir().join(format!(
            "runlith-provider-env-explicit-{}",
            std::process::id()
        ));
        let workdir = root.join("work");
        fs::create_dir_all(&workdir).unwrap();
        let base = root.join("provider");
        let lookup = |pairs: &'static [(&'static str, &'static str)]| {
            move |name: &str| {
                pairs
                    .iter()
                    .find(|(key, _)| *key == name)
                    .map(|(_, value)| OsString::from(value))
            }
        };

        for (pairs, expected) in [
            (
                &[("HOME", "/nonexistent"), ("USER", "runlith-fixture")][..],
                "provider environment requires PATH",
            ),
            (
                &[("PATH", "/usr/bin:/bin"), ("USER", "runlith-fixture")][..],
                "provider environment requires HOME for provider-owned authentication",
            ),
            (
                &[("PATH", "/usr/bin:/bin"), ("HOME", "/nonexistent")][..],
                "provider environment requires USER for provider-owned authentication",
            ),
            (
                &[
                    ("PATH", "/usr/bin:/bin"),
                    ("HOME", "/nonexistent"),
                    ("USER", ""),
                ][..],
                "provider environment requires USER for provider-owned authentication",
            ),
        ] {
            let missing = ProviderEnvironment::prepare_from(&workdir, &base, &lookup(pairs));
            assert_eq!(missing.unwrap_err(), expected);
        }

        let environment = ProviderEnvironment::prepare_from(
            &workdir,
            &base,
            &lookup(&[
                ("PATH", "/usr/bin:/bin"),
                ("HOME", "/nonexistent"),
                ("USER", "runlith-fixture"),
                ("RUNLITH_UNALLOWLISTED_FIXTURE", "must-not-pass"),
            ]),
        )
        .expect("explicit provider environment");
        let pairs = environment.child_environment();
        let value = |key: &str| {
            pairs
                .iter()
                .find(|(name, _)| name == OsStr::new(key))
                .map(|(_, value)| value.clone())
        };
        assert_eq!(value("USER"), Some(OsString::from("runlith-fixture")));
        // The runner path never carries the host HOME (ADR-0039 P3).
        assert_eq!(
            value("HOME"),
            Some(environment.runtime_root().join("home").into_os_string())
        );
        assert_eq!(value("PATH"), Some(OsString::from("/usr/bin:/bin")));
        assert_eq!(value("RUNLITH_UNALLOWLISTED_FIXTURE"), None);
        let _ = fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    fn fake_host_with_auth_anchors(name: &str) -> (PathBuf, PathBuf, PathBuf) {
        let root = std::env::temp_dir().join(format!(
            "runlith-provider-env-{name}-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        let workdir = root.join("work");
        let host_home = root.join("host-home");
        fs::create_dir_all(&workdir).unwrap();
        fs::create_dir_all(host_home.join(".claude")).unwrap();
        fs::write(host_home.join(".claude.json"), b"{}").unwrap();
        (root, workdir, host_home)
    }

    #[cfg(unix)]
    fn lookup_for(host_home: &Path) -> impl Fn(&str) -> Option<OsString> {
        let host_home = host_home.as_os_str().to_owned();
        move |name: &str| match name {
            "PATH" => Some(OsString::from("/usr/bin:/bin")),
            "HOME" => Some(host_home.clone()),
            "USER" => Some(OsString::from("runlith-fixture")),
            _ => None,
        }
    }

    /// ADR-0039 P3: the runner path gets a private HOME and CLAUDE_CONFIG_DIR
    /// under the runtime root; nothing points into the host profile.
    #[cfg(unix)]
    #[test]
    fn runner_environment_uses_private_home_and_config_dir() {
        let (root, workdir, host_home) = fake_host_with_auth_anchors("private-home");
        let environment = ProviderEnvironment::prepare_from(
            &workdir,
            &root.join("provider"),
            &lookup_for(&host_home),
        )
        .expect("provider environment");
        let pairs = environment.child_environment();
        let value = |key: &str| {
            pairs
                .iter()
                .find(|(name, _)| name == OsStr::new(key))
                .map(|(_, value)| PathBuf::from(value))
        };
        let private_home = environment.runtime_root().join("home");
        let config_dir = environment.runtime_root().join("claude");
        assert_eq!(value("HOME"), Some(private_home.clone()));
        assert_eq!(value("CLAUDE_CONFIG_DIR"), Some(config_dir.clone()));
        for (name, path) in &pairs {
            assert!(
                !PathBuf::from(path).starts_with(&host_home),
                "{name:?} points into the host profile"
            );
        }
        for dir in [private_home, config_dir] {
            let mode = fs::metadata(&dir)
                .expect("private dir exists")
                .permissions()
                .mode();
            assert_eq!(mode & 0o7777, 0o700, "{} must be 0700", dir.display());
        }
        let _ = fs::remove_dir_all(root);
    }

    /// ADR-0039 P3: host authentication anchors are not sandbox support roots.
    #[cfg(unix)]
    #[test]
    fn support_roots_exclude_host_authentication_anchors() {
        let (root, workdir, host_home) = fake_host_with_auth_anchors("support-roots");
        let environment = ProviderEnvironment::prepare_from(
            &workdir,
            &root.join("provider"),
            &lookup_for(&host_home),
        )
        .expect("provider environment");
        assert_eq!(
            environment.isolation_support_read_roots().unwrap(),
            vec![environment.runtime_root().to_path_buf()]
        );
        assert_eq!(
            environment.isolation_support_write_roots(),
            vec![environment.runtime_root().to_path_buf()]
        );
        let _ = fs::remove_dir_all(root);
    }

    /// The feature-gated trusted-host supervisor keeps the host HOME.
    #[cfg(unix)]
    #[test]
    fn trusted_host_apply_keeps_the_host_home() {
        let (root, workdir, host_home) = fake_host_with_auth_anchors("trusted-host");
        let environment = ProviderEnvironment::prepare_from(
            &workdir,
            &root.join("provider"),
            &lookup_for(&host_home),
        )
        .expect("provider environment");
        let mut command = Command::new("/usr/bin/true");
        environment.apply(&mut command);
        let env = |key: &str| {
            command
                .get_envs()
                .find(|(name, _)| *name == OsStr::new(key))
                .and_then(|(_, value)| value.map(OsString::from))
        };
        assert_eq!(env("HOME"), Some(host_home.into_os_string()));
        assert_eq!(env("CLAUDE_CONFIG_DIR"), None);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn runtime_root_must_be_outside_worktree() {
        let root =
            std::env::temp_dir().join(format!("runlith-provider-env-unit-{}", std::process::id()));
        let workdir = root.join("work");
        fs::create_dir_all(&workdir).unwrap();
        let result = ProviderEnvironment::prepare(&workdir, &workdir.join("provider"));
        assert!(result.is_err());
        let _ = fs::remove_dir_all(root);
    }
}
