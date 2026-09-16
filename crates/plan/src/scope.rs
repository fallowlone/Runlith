//! V1 scope syntax owned by TASK-0076 (ADR-0027 §3 V4).
//!
//! A scope is a set of roots of the form `workspace:path/prefix`, where the
//! path is relative (no `..`, no absolute paths — same fail-closed posture
//! as the policy crate). The subset relation: every child root must fall
//! within (equal to or under) some bound root in the same workspace.
//! Segment-aware prefix matching applies, so `ws:src/api` is within
//! `ws:src` but `ws:src-other` is not.

use std::path::{Component, Path, PathBuf};

use crate::error::PlanDomainError;

/// Authorized-scope bound for one plan branch (V4 `scope_bound`).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ScopeBound {
    roots: Vec<ScopeRoot>,
}

/// Scope carried by one plan node or `scope_delta`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlanScope {
    roots: Vec<ScopeRoot>,
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
struct ScopeRoot {
    workspace: String,
    path: PathBuf,
}

impl ScopeRoot {
    fn parse(value: &str) -> Result<Self, PlanDomainError> {
        let (workspace, path) = value.split_once(':').ok_or_else(|| {
            PlanDomainError::input(format!("scope root {value:?} must be workspace:path"))
        })?;
        if workspace.trim().is_empty() || workspace.chars().any(char::is_control) {
            return Err(PlanDomainError::input(format!(
                "scope root {value:?} has a malformed workspace"
            )));
        }
        let path = normalize_relative(Path::new(path)).ok_or_else(|| {
            PlanDomainError::input(format!("scope root {value:?} has a non-relative path"))
        })?;
        Ok(Self {
            workspace: workspace.to_owned(),
            path,
        })
    }

    fn contains(&self, other: &Self) -> bool {
        if self.workspace != other.workspace {
            return false;
        }
        if self.path == Path::new(".") {
            return true;
        }
        other.path.starts_with(&self.path)
    }
}

impl ScopeRoot {
    fn canonical_bytes(&self) -> Vec<u8> {
        let mut out = Vec::new();
        push_field(&mut out, self.workspace.as_bytes());
        push_field(&mut out, self.path.as_os_str().as_encoded_bytes());
        out
    }
}

fn push_field(out: &mut Vec<u8>, value: &[u8]) {
    out.extend_from_slice(&(value.len() as u64).to_be_bytes());
    out.extend_from_slice(value);
}

fn normalize_relative(path: &Path) -> Option<PathBuf> {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Normal(part) => normalized.push(part),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => return None,
        }
    }
    if normalized.as_os_str().is_empty() {
        Some(PathBuf::from("."))
    } else {
        Some(normalized)
    }
}

fn parse_sorted_unique(values: Vec<String>) -> Result<Vec<ScopeRoot>, PlanDomainError> {
    if values.is_empty() {
        return Err(PlanDomainError::input(
            "scope must declare at least one root",
        ));
    }
    let mut roots: Vec<ScopeRoot> = values
        .iter()
        .map(|value| ScopeRoot::parse(value))
        .collect::<Result<_, _>>()?;
    roots.sort();
    roots.dedup();
    Ok(roots)
}

impl ScopeBound {
    /// Parses and normalizes the authorized branch scope.
    pub fn new(roots: Vec<String>) -> Result<Self, PlanDomainError> {
        Ok(Self {
            roots: parse_sorted_unique(roots)?,
        })
    }

    /// Returns the normalized canonical roots used by this bound.
    ///
    /// Planner adapters need a value representation to serialize the
    /// authorized bound into provider input. Returning owned strings keeps
    /// the domain independent of any filesystem handle or adapter type.
    #[must_use]
    pub fn roots(&self) -> Vec<String> {
        self.roots
            .iter()
            .map(|root| {
                format!(
                    "{}:{}",
                    root.workspace,
                    root.path.as_os_str().to_string_lossy()
                )
            })
            .collect()
    }

    /// True when every root in `self` is contained by `bound`. Human
    /// decisions use this to refuse scope widening before continuation.
    #[must_use]
    pub fn is_subset_of(&self, bound: &ScopeBound) -> bool {
        self.roots
            .iter()
            .all(|child| bound.roots.iter().any(|parent| parent.contains(child)))
    }
}

impl PlanScope {
    /// Parses and normalizes a node/delta scope.
    pub fn new(roots: Vec<String>) -> Result<Self, PlanDomainError> {
        Ok(Self {
            roots: parse_sorted_unique(roots)?,
        })
    }

    /// V4 subset check: every root of `self` must fall within `bound`.
    #[must_use]
    pub fn is_subset_of(&self, bound: &ScopeBound) -> bool {
        self.roots
            .iter()
            .all(|child| bound.roots.iter().any(|parent| parent.contains(child)))
    }

    /// Returns the normalized canonical roots used by this scope.
    ///
    /// This is a value-level representation for durable adapters and
    /// diagnostics; callers receive owned strings and no filesystem handle
    /// or mutation authority.
    #[must_use]
    pub fn roots(&self) -> Vec<String> {
        self.roots
            .iter()
            .map(|root| {
                format!(
                    "{}:{}",
                    root.workspace,
                    root.path.as_os_str().to_string_lossy()
                )
            })
            .collect()
    }

    pub(crate) fn canonical_bytes(&self) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&(self.roots.len() as u64).to_be_bytes());
        for root in &self.roots {
            out.extend_from_slice(&root.canonical_bytes());
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scope_of(roots: &[&str]) -> PlanScope {
        PlanScope::new(roots.iter().map(ToString::to_string).collect()).unwrap()
    }

    fn bound_of(roots: &[&str]) -> ScopeBound {
        ScopeBound::new(roots.iter().map(ToString::to_string).collect()).unwrap()
    }

    #[test]
    fn subset_holds_for_equal_narrower_and_workspace_root() {
        let bound = bound_of(&["ws1:src"]);
        assert!(scope_of(&["ws1:src"]).is_subset_of(&bound));
        assert!(scope_of(&["ws1:src/api"]).is_subset_of(&bound));
        assert!(scope_of(&["ws1:src/api", "ws1:src/web"]).is_subset_of(&bound));
    }

    #[test]
    fn subset_rejects_sibling_prefix_and_other_workspace() {
        let bound = bound_of(&["ws1:src"]);
        assert!(!scope_of(&["ws1:src-other"]).is_subset_of(&bound));
        assert!(!scope_of(&["ws2:src/api"]).is_subset_of(&bound));
        assert!(!scope_of(&["ws1:src/api", "ws1:elsewhere"]).is_subset_of(&bound));
    }

    #[test]
    fn malformed_roots_fail_closed() {
        assert!(PlanScope::new(vec![]).is_err());
        assert!(PlanScope::new(vec!["no-colon".to_owned()]).is_err());
        assert!(PlanScope::new(vec!["ws1:../outside".to_owned()]).is_err());
        assert!(PlanScope::new(vec!["ws1:/tmp/abs".to_owned()]).is_err());
        assert!(ScopeBound::new(vec![":src".to_owned()]).is_err());
    }
}
