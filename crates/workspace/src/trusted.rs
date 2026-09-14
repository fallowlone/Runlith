//! Trusted common-Git anchor (TASK-0071 follow-up, closes F7).
//!
//! `RepositoryRef` is durable domain identity (provider-neutral, never a
//! path). `TrustedCommonGit` is local infrastructure binding: the canonical
//! common-Git directory (`--git-common-dir`, canonicalized) captured from
//! trusted admission state before provider execution and carried into every
//! worktree Git operation.
//!
//! The two are never substituted: `RepositoryRef` never authenticates a
//! filesystem location, and `TrustedCommonGit` never enters Task semantics,
//! lease snapshots, or the journal. It lives only in the durable sidecar
//! (`workspace_trusted_anchor`) and as an in-operation `&TrustedCommonGit`
//! parameter (never `Option`, never cached across operations).

use std::path::{Path, PathBuf};

use crate::failure::WorkspaceFailure;

/// Opaque trusted common-Git identity: canonical absolute commondir.
///
/// Equality is byte equality of the canonical paths.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrustedCommonGit {
    canonical_commondir: PathBuf,
}

fn backend_failure(detail: impl Into<String>) -> WorkspaceFailure {
    // Bound detail like the adapter does (500 chars) to keep diagnostics
    // bounded even when paths are long.
    let mut text = detail.into();
    if text.chars().count() > 500 {
        let truncated: String = text.chars().take(500).collect();
        text = format!("{truncated}…[truncated]");
    }
    WorkspaceFailure::GitBackendFailure { detail: text }
}

impl TrustedCommonGit {
    /// Trusted-capture constructor only.
    ///
    /// Must be called only by `GitPort::establish_trusted_anchor` (admission,
    /// trusted-deposit before provider execution) and by durable-sidecar
    /// rehydration (which re-validates). Never construct from
    /// provider-writable observations outside the admission boundary.
    pub fn from_canonical_commondir(path: PathBuf) -> Result<Self, WorkspaceFailure> {
        if !path.is_absolute() {
            return Err(backend_failure(
                "trusted Git anchor must be an absolute canonical path",
            ));
        }
        let as_str = path.to_string_lossy();
        if as_str.contains('\0') {
            return Err(backend_failure("trusted Git anchor path contains NUL"));
        }
        // Canonicalize-compatible: the path must resolve on this machine.
        // Missing/unresolvable anchors fail closed (no backfill from FS).
        let canonical = std::fs::canonicalize(&path).map_err(|e| {
            backend_failure(format!(
                "trusted Git anchor is not canonicalize-compatible {}: {e}",
                path.display()
            ))
        })?;
        if canonical != path {
            return Err(backend_failure(format!(
                "trusted Git anchor is not canonical: {}",
                path.display()
            )));
        }
        Ok(Self {
            canonical_commondir: canonical,
        })
    }

    /// Rehydrates a sidecar row. Validates absolute, NUL-free,
    /// canonicalize-compatible; otherwise `GitBackendFailure` (fail closed,
    /// never backfill from current FS).
    pub fn restore(canonical_commondir: &str) -> Result<Self, WorkspaceFailure> {
        if canonical_commondir.is_empty() || canonical_commondir.contains('\0') {
            return Err(backend_failure("stored trusted Git anchor is not usable"));
        }
        let path = PathBuf::from(canonical_commondir);
        if !path.is_absolute() {
            return Err(backend_failure(
                "stored trusted Git anchor must be absolute",
            ));
        }
        Self::from_canonical_commondir(path)
    }

    /// Borrows the canonical commondir.
    #[must_use]
    pub fn canonical_commondir(&self) -> &Path {
        &self.canonical_commondir
    }

    /// Storage form (UTF-8 required; non-UTF-8 anchors are refused at
    /// capture rather than lossily persisted).
    pub fn as_str(&self) -> Result<&str, WorkspaceFailure> {
        self.canonical_commondir
            .to_str()
            .ok_or_else(|| backend_failure("trusted Git anchor path is not UTF-8; refusing"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_relative_and_nul() {
        assert!(TrustedCommonGit::from_canonical_commondir(PathBuf::from("rel/path")).is_err());
        assert!(TrustedCommonGit::restore("rel/path").is_err());
        assert!(TrustedCommonGit::restore("a\0b").is_err());
        assert!(TrustedCommonGit::restore("").is_err());
    }

    #[test]
    fn equality_is_byte_equality_of_canonical_paths() {
        let dir = std::env::temp_dir();
        let canon = std::fs::canonicalize(&dir).unwrap();
        let a = TrustedCommonGit::from_canonical_commondir(canon.clone()).unwrap();
        let b = TrustedCommonGit::from_canonical_commondir(canon.clone()).unwrap();
        assert_eq!(a, b);
        let other = TrustedCommonGit::from_canonical_commondir(std::fs::canonicalize("/").unwrap())
            .unwrap();
        if canon.as_os_str() != "/" {
            assert_ne!(a, other);
        }
    }
}
