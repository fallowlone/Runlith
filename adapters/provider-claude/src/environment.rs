//! Controlled provider-process environment (TASK-0065, G6 H7).
//!
//! Canonical authority still comes from Policy/Runner. This module only
//! removes ambient process state and moves provider-writable runtime state
//! outside the managed Git worktree.

use std::ffi::{OsStr, OsString};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
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
    path: OsString,
    home: Option<OsString>,
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

        let path = required_env("PATH")?;
        let home = non_empty_env("HOME");
        #[cfg(unix)]
        if home.is_none() {
            return Err(
                "provider environment requires HOME for provider-owned authentication".to_owned(),
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
            path,
            home,
            #[cfg(windows)]
            user_profile: non_empty_env("USERPROFILE"),
            #[cfg(windows)]
            system_root: non_empty_env("SystemRoot").or_else(|| non_empty_env("WINDIR")),
            #[cfg(windows)]
            path_ext: non_empty_env("PATHEXT"),
        })
    }

    /// Applies an allowlisted environment to the exact child command.
    /// Ambient API keys, cloud credentials, Git overrides, SSH agents and
    /// provider/plugin variables are intentionally not inherited.
    pub(crate) fn apply(&self, command: &mut Command) {
        command.env_clear();
        for (name, value) in self.resolved_pairs() {
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
    pub fn child_environment(&self) -> Vec<(OsString, OsString)> {
        self.resolved_pairs()
    }

    fn resolved_pairs(&self) -> Vec<(OsString, OsString)> {
        let mut pairs: Vec<(OsString, OsString)> = Vec::new();
        pairs.push((OsString::from("PATH"), self.path.clone()));
        if let Some(home) = &self.home {
            pairs.push((OsString::from("HOME"), home.clone()));
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
            if let Some(profile) = &self.user_profile {
                pairs.push((OsString::from("USERPROFILE"), profile.clone()));
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
    /// workspace. The real HOME contribution is limited to Claude's explicit
    /// authentication anchor rather than the whole user profile.
    #[must_use]
    pub fn isolation_support_read_roots(&self) -> Vec<PathBuf> {
        let mut roots = vec![self.runtime_root.clone()];
        if let Some(home) = &self.home {
            let home = PathBuf::from(home);
            roots.push(home.join(".claude"));
            roots.push(home.join(".claude.json"));
        }
        roots
    }

    /// Provider-writable sandbox support is limited to the isolated runtime
    /// root. Authentication anchors remain read-only at the OS boundary.
    #[must_use]
    pub fn isolation_support_write_roots(&self) -> Vec<PathBuf> {
        vec![self.runtime_root.clone()]
    }
}

fn non_empty_env(name: &str) -> Option<OsString> {
    std::env::var_os(name).filter(|value| !value.is_empty())
}

fn required_env(name: &str) -> Result<OsString, String> {
    non_empty_env(name).ok_or_else(|| format!("provider environment requires {name}"))
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
