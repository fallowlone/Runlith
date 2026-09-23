//! G8 task-prerequisite vocabulary (TASK-0135).
//!
//! A [`Prerequisite`] is a declared precondition for activating a task:
//! a Git ref that must already be an ancestor of `HEAD`, a repo-relative
//! file that must exist, or a command that must be on `PATH`. This module
//! is pure vocabulary, following the [`crate::requirement`] contract: it
//! checks nothing, touches no filesystem, spawns no Git, and reads no
//! environment. Checking is owned by the application-services preflight
//! service; verdict authority stays with the orchestrator (preflight
//! informs, it never transitions a task by itself).
//!
//! Repo-escape protection begins here at the vocabulary boundary: file
//! prerequisites must be relative paths with no parent, root, or prefix
//! components, and command prerequisites must be bare executable names.
//! The checking layer re-validates containment against the canonical
//! repository root (defense in depth); an invalid value can never reach
//! the checker as a trusted shape.

use std::path::{Component, Path};

use crate::error::{DomainError, DomainResult};
use crate::task::MAX_TASK_METADATA_ITEM_LEN;

/// Closed prerequisite-kind registry, v1. Unknown kinds fail closed at
/// every boundary; the registry grows only by ADR amendment.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum PrerequisiteKind {
    /// A Git ref that must resolve to a commit already reachable from
    /// `HEAD` (checked via `merge-base --is-ancestor`).
    GitAncestorRef,
    /// A repo-relative file (or directory) that must exist inside the
    /// repository working tree.
    RepoFileExists,
    /// A bare executable name that must be found on `PATH`.
    CommandOnPath,
}

impl PrerequisiteKind {
    /// Canonical text form (storage, hashing, wire adapters).
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::GitAncestorRef => "git-ancestor-ref",
            Self::RepoFileExists => "repo-file-exists",
            Self::CommandOnPath => "command-on-path",
        }
    }

    /// Parses the canonical text form; unknown values fail closed.
    pub fn parse(value: &str) -> DomainResult<Self> {
        match value {
            "git-ancestor-ref" => Ok(Self::GitAncestorRef),
            "repo-file-exists" => Ok(Self::RepoFileExists),
            "command-on-path" => Ok(Self::CommandOnPath),
            other => Err(DomainError::input(format!(
                "unknown prerequisite kind {other:?}"
            ))),
        }
    }
}

/// One declared prerequisite: a kind plus a single bounded value whose
/// shape is validated per kind at construction.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Prerequisite {
    kind: PrerequisiteKind,
    value: String,
}

impl Prerequisite {
    /// Creates a validated prerequisite. Generic bounds (non-blank,
    /// length-capped, no control characters) apply first, then per-kind
    /// shape rules (see [`PrerequisiteKind`]).
    pub fn new(kind: PrerequisiteKind, value: impl Into<String>) -> DomainResult<Self> {
        let value = bounded_value(value.into())?;
        match kind {
            PrerequisiteKind::GitAncestorRef => validate_git_ref(&value)?,
            PrerequisiteKind::RepoFileExists => validate_repo_relative_path(&value)?,
            PrerequisiteKind::CommandOnPath => validate_command_name(&value)?,
        }
        Ok(Self { kind, value })
    }

    #[must_use]
    pub const fn kind(&self) -> PrerequisiteKind {
        self.kind
    }

    /// The validated kind-specific value (ref name, repo-relative path, or
    /// command name).
    #[must_use]
    pub fn value(&self) -> &str {
        &self.value
    }
}

fn bounded_value(value: String) -> DomainResult<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(DomainError::input("prerequisite value must not be blank"));
    }
    if trimmed.len() > MAX_TASK_METADATA_ITEM_LEN {
        return Err(DomainError::input(format!(
            "prerequisite value exceeds {MAX_TASK_METADATA_ITEM_LEN} bytes"
        )));
    }
    if trimmed.chars().any(char::is_control) {
        return Err(DomainError::input(
            "prerequisite value must not contain control characters",
        ));
    }
    Ok(trimmed.to_owned())
}

/// Ref names are passed as argv to Git; reject anything that could be
/// parsed as a flag, carries whitespace, or hides traversal syntax.
fn validate_git_ref(value: &str) -> DomainResult<()> {
    if value.starts_with('-') {
        return Err(DomainError::input(
            "git ref prerequisite must not start with '-'",
        ));
    }
    if value.chars().any(char::is_whitespace) {
        return Err(DomainError::input(
            "git ref prerequisite must not contain whitespace",
        ));
    }
    if value.contains("..") {
        return Err(DomainError::input(
            "git ref prerequisite must not contain '..'",
        ));
    }
    Ok(())
}

/// Repo-escape protection at the vocabulary boundary: the path must be
/// relative and consist solely of normal components (no root, prefix,
/// current-dir, or parent-dir segments).
fn validate_repo_relative_path(value: &str) -> DomainResult<()> {
    let path = Path::new(value);
    if path.is_absolute() {
        return Err(DomainError::input(
            "repo file prerequisite must be a relative path",
        ));
    }
    // Lexical check first: `Path::components` normalizes interior `.`
    // segments away, so containment must be decided on the raw segments.
    if value
        .split(['/', '\\'])
        .any(|segment| segment.is_empty() || segment == "." || segment == "..")
    {
        return Err(DomainError::input(
            "repo file prerequisite must contain only plain path components",
        ));
    }
    for component in path.components() {
        match component {
            Component::Normal(_) => {}
            Component::ParentDir => {
                return Err(DomainError::input(
                    "repo file prerequisite must not contain '..'",
                ));
            }
            Component::RootDir | Component::Prefix(_) | Component::CurDir => {
                return Err(DomainError::input(
                    "repo file prerequisite must contain only plain path components",
                ));
            }
        }
    }
    Ok(())
}

/// Command prerequisites are bare executable names resolved against
/// `PATH`; any path separator would smuggle in a location the declaration
/// did not authorize.
fn validate_command_name(value: &str) -> DomainResult<()> {
    if value.contains(['/', '\\']) {
        return Err(DomainError::input(
            "command prerequisite must be a bare executable name, not a path",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kind_text_roundtrip() {
        for kind in [
            PrerequisiteKind::GitAncestorRef,
            PrerequisiteKind::RepoFileExists,
            PrerequisiteKind::CommandOnPath,
        ] {
            assert_eq!(PrerequisiteKind::parse(kind.as_str()).unwrap(), kind);
        }
        assert!(PrerequisiteKind::parse("unknown-kind").is_err());
    }

    #[test]
    fn accepts_well_formed_values() {
        Prerequisite::new(PrerequisiteKind::GitAncestorRef, "refs/heads/base").unwrap();
        Prerequisite::new(PrerequisiteKind::GitAncestorRef, "v1.2.3").unwrap();
        Prerequisite::new(PrerequisiteKind::RepoFileExists, "crates/task/Cargo.toml").unwrap();
        Prerequisite::new(PrerequisiteKind::CommandOnPath, "cargo-nextest").unwrap();
    }

    #[test]
    fn rejects_blank_overlong_and_control_values() {
        assert!(Prerequisite::new(PrerequisiteKind::CommandOnPath, "   ").is_err());
        let long = "x".repeat(MAX_TASK_METADATA_ITEM_LEN + 1);
        assert!(Prerequisite::new(PrerequisiteKind::CommandOnPath, long).is_err());
        assert!(Prerequisite::new(PrerequisiteKind::CommandOnPath, "git\0x").is_err());
    }

    #[test]
    fn rejects_repo_escape_paths() {
        for bad in ["../secret", "a/../../b", "/etc/passwd", "./x", "a/./b"] {
            assert!(
                Prerequisite::new(PrerequisiteKind::RepoFileExists, bad).is_err(),
                "accepted {bad:?}"
            );
        }
    }

    #[test]
    fn rejects_command_paths() {
        for bad in ["/bin/sh", "./run", "bin/tool", "..\\tool"] {
            assert!(
                Prerequisite::new(PrerequisiteKind::CommandOnPath, bad).is_err(),
                "accepted {bad:?}"
            );
        }
    }

    #[test]
    fn rejects_flag_like_or_traversal_refs() {
        for bad in ["--upload-pack=x", "main..other", "with space"] {
            assert!(
                Prerequisite::new(PrerequisiteKind::GitAncestorRef, bad).is_err(),
                "accepted {bad:?}"
            );
        }
    }
}
