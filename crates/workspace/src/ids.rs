//! Strong provider-neutral Workspace/Git identities.
//!
//! ```text
//! WorkspaceId != path != BranchRef != ExecutionId
//! RepositoryRef (durable) != RepositoryRoot (local path, infrastructure)
//! ```

use std::fmt;

/// Maximum validated identity length, matching task-domain conventions.
pub const MAX_ID_LEN: usize = 256;

/// Domain validation failure for workspace identities.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceError(String);

impl WorkspaceError {
    pub(crate) fn input(detail: impl Into<String>) -> Self {
        Self(detail.into())
    }
}

impl fmt::Display for WorkspaceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid workspace identity: {}", self.0)
    }
}

impl std::error::Error for WorkspaceError {}

/// Workspace result alias.
pub type WorkspaceResult<T> = Result<T, WorkspaceError>;

fn validate_opaque(kind: &str, raw: &str) -> WorkspaceResult<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(WorkspaceError::input(format!("{kind} must not be blank")));
    }
    if trimmed.len() > MAX_ID_LEN {
        return Err(WorkspaceError::input(format!(
            "{kind} exceeds {MAX_ID_LEN} bytes"
        )));
    }
    Ok(trimmed.to_owned())
}

macro_rules! opaque_id {
    ($name:ident, $kind:literal, $doc:literal) => {
        #[doc = $doc]
        #[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub struct $name(String);

        impl $name {
            /// Validates and normalizes (trims) the raw identity.
            pub fn new(raw: impl Into<String>) -> WorkspaceResult<Self> {
                Ok(Self(validate_opaque($kind, &raw.into())?))
            }

            /// Borrows the normalized identity text.
            #[must_use]
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }
    };
}

opaque_id!(
    WorkspaceId,
    "workspace id",
    "Strong provider-neutral workspace identity. Never a filesystem path, never a branch name, never an ExecutionId."
);

opaque_id!(
    RepositoryRef,
    "repository ref",
    "Provider-neutral durable repository identity. Distinct from the local checkout path (RepositoryRoot), which is infrastructure data and may change across machines."
);

/// Immutable Git object identity (commit OID) a managed workspace is based on.
///
/// Branch names, `HEAD`, and default-branch tips are never revisions.
/// Accepts SHA-1 (40 hex) or SHA-256 (64 hex), lowercase.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Revision(String);

impl Revision {
    /// Validates a commit OID: 40 or 64 lowercase hex characters.
    pub fn new(raw: impl Into<String>) -> WorkspaceResult<Self> {
        let trimmed = raw.into();
        let trimmed = trimmed.trim();
        let valid_len = trimmed.len() == 40 || trimmed.len() == 64;
        let valid_hex = trimmed
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase());
        if !valid_len || !valid_hex {
            return Err(WorkspaceError::input(
                "revision must be a 40- or 64-character lowercase hex commit OID",
            ));
        }
        Ok(Self(trimmed.to_owned()))
    }

    /// Borrows the normalized OID text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Revision {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Managed branch reference (`refs/heads/<name>`), recorded on the lease.
///
/// Separate from `WorkspaceId`: the ref is Git mechanics, the id is domain
/// identity. Collision detection compares this recorded ref against actual
/// ref state — a lease is never silently attached to an unrelated branch.
///
/// Type semantics (TASK-0046, G4 finding F2, interpretation B): `BranchRef`
/// intentionally represents arbitrary valid/existing Git refs — it is used
/// both to record managed branches and to observe ref state — so the generic
/// constructor is NOT restricted to the managed `agent/` scope. Instead:
/// (a) the constructor rejects every shape Git itself forbids plus every
/// shape carrying `rev-parse` revision semantics (`~`, `^`, `:`, `@{`, …),
/// so a `BranchRef` always names exactly one ref and never a revision
/// expression; (b) only the managed-branch derivation boundary (slug-safe,
/// in the application service) may mint refs that count as managed, gated
/// by [`BranchRef::is_managed`]. Untrusted/provider input can therefore
/// never smuggle revision semantics or escape the managed scope through
/// this type.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct BranchRef(String);

/// Prefix below `refs/heads/` scoping application-managed branches.
/// Only refs derived through the managed-branch derivation boundary may
/// carry it; deletion paths gate on [`BranchRef::is_managed`].
pub const MANAGED_BRANCH_PREFIX: &str = "agent/";

impl BranchRef {
    /// Validates a full branch ref under `refs/heads/`.
    ///
    /// Accepts exactly the refs Git can actually hold: no blank names, no
    /// `..`, no ASCII control characters, none of Git's ref-forbidden bytes
    /// (`~ ^ : ? * [` plus `\` and space), no `@{` reflog syntax, no empty
    /// path components, no components starting with `.`, no trailing `/`,
    /// `.`, or `.lock`. These are precisely the shapes that would alter
    /// `rev-parse`/`show-ref` semantics (e.g. `name~` resolving to a parent
    /// commit) or that Git forbids via `git check-ref-format`.
    pub fn new(raw: impl Into<String>) -> WorkspaceResult<Self> {
        let trimmed = validate_opaque("branch ref", &raw.into())?;
        let name = trimmed
            .strip_prefix("refs/heads/")
            .ok_or_else(|| WorkspaceError::input("branch ref must start with refs/heads/"))?;
        validate_branch_name(name)?;
        Ok(Self(trimmed))
    }

    /// Borrows the normalized ref text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The short branch name below `refs/heads/`.
    #[must_use]
    pub fn short_name(&self) -> &str {
        self.0.strip_prefix("refs/heads/").unwrap_or(&self.0)
    }

    /// Whether this ref lies inside the application-managed branch scope
    /// (`refs/heads/agent/`). Derivation-only: only refs minted by the
    /// managed-branch derivation boundary satisfy this; deletion paths
    /// must gate on it so untrusted/provider input can never authorize
    /// branch deletion, even when it parses as a valid `BranchRef`.
    #[must_use]
    pub fn is_managed(&self) -> bool {
        self.short_name().starts_with(MANAGED_BRANCH_PREFIX)
    }
}

/// Rejects every branch-name shape that is either forbidden by
/// `git check-ref-format` or carries revision-expression semantics.
fn validate_branch_name(name: &str) -> WorkspaceResult<()> {
    if name.is_empty() {
        return Err(WorkspaceError::input("branch ref name must not be blank"));
    }
    if name.contains("..")
        || name.contains("@{")
        || name.contains(' ')
        || name.contains('~')
        || name.contains('^')
        || name.contains(':')
        || name.contains('?')
        || name.contains('*')
        || name.contains('[')
        || name.contains('\\')
        || name.bytes().any(|b| b < 0x20 || b == 0x7f)
    {
        return Err(WorkspaceError::input(
            "branch ref name carries forbidden characters or revision semantics",
        ));
    }
    if name.ends_with('/') || name.ends_with('.') || name.ends_with(".lock") {
        return Err(WorkspaceError::input(
            "branch ref name must not end with '/', '.', or '.lock'",
        ));
    }
    for component in name.split('/') {
        if component.is_empty() || component.starts_with('.') {
            return Err(WorkspaceError::input(
                "branch ref name must not hold empty or dot-led components",
            ));
        }
    }
    Ok(())
}

impl fmt::Display for BranchRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identities_reject_blank_and_overlong() {
        assert!(WorkspaceId::new("  ").is_err());
        assert!(RepositoryRef::new("").is_err());
        assert!(BranchRef::new("refs/heads/").is_err());
        assert!(WorkspaceId::new("w".repeat(MAX_ID_LEN + 1)).is_err());
    }

    #[test]
    fn workspace_id_is_not_a_path_branch_or_execution() {
        // Type-level separation: a path parses as an opaque id string, but
        // the three concepts never unify — BranchRef refuses non-refs, and
        // ExecutionId lives in agent-runtime, not here.
        let id = WorkspaceId::new("ws-1").unwrap();
        assert_eq!(id.as_str(), "ws-1");
        assert!(BranchRef::new("ws-1").is_err());
        assert!(BranchRef::new("main").is_err());
        assert_eq!(
            BranchRef::new("refs/heads/agent/e-1").unwrap().short_name(),
            "agent/e-1"
        );
    }

    #[test]
    fn revision_requires_an_oid_never_a_branch_name() {
        assert!(Revision::new("main").is_err());
        assert!(Revision::new("HEAD").is_err());
        assert!(Revision::new("abc123").is_err());
        assert!(Revision::new("ABCDEF1234567890ABCDEF1234567890ABCDEF12").is_err());
        assert!(Revision::new("a".repeat(40)).is_ok());
        assert!(Revision::new("b".repeat(64)).is_ok());
        assert!(Revision::new("c".repeat(41)).is_err());
    }

    #[test]
    fn branch_ref_rejects_relative_and_blank_names() {
        assert!(BranchRef::new("refs/heads/a/../b").is_err());
        assert!(BranchRef::new("refs/heads/has space").is_err());
    }

    #[test]
    fn branch_ref_rejects_revision_semantics_and_git_forbidden_shapes() {
        // G4 finding F2: every shape that would alter rev-parse/show-ref
        // semantics or that git check-ref-format forbids is rejected, so a
        // BranchRef always names exactly one ref. (Interpretation B: the
        // type still represents arbitrary valid refs — it is not restricted
        // to the managed scope; see is_managed below.)
        for hostile in [
            "refs/heads/agent/e-1~",
            "refs/heads/agent/e-1^",
            "refs/heads/agent/e-1~3",
            "refs/heads/agent/e-1^{commit}",
            "refs/heads/agent/e-1@{1}",
            "refs/heads/agent/e-1@{u}",
            "refs/heads/main:other",
            "refs/heads/wild*",
            "refs/heads/quest?",
            "refs/heads/brack[et",
            "refs/heads/back\\slash",
            "refs/heads/ctrl\x01char",
            "refs/heads/trailingslash/",
            "refs/heads/trailingdot.",
            "refs/heads/name.lock",
            "refs/heads//empty",
            "refs/heads/.dotled",
            "refs/heads/a/.hidden",
        ] {
            assert!(
                BranchRef::new(hostile).is_err(),
                "{hostile:?} must not parse as a branch ref"
            );
        }
        // Ordinary valid refs — managed and otherwise — still parse.
        for valid in [
            "refs/heads/agent/e-1",
            "refs/heads/agent/h-00FF",
            "refs/heads/trunk",
            "refs/heads/feature/some-work",
            "refs/heads/release-1.2",
            "refs/heads/under_score",
        ] {
            assert!(
                BranchRef::new(valid).is_ok(),
                "{valid:?} must remain a valid branch ref"
            );
        }
    }

    #[test]
    fn managed_scope_is_derivation_gated_not_parse_gated() {
        // Valid non-managed refs parse (interpretation B) but never count
        // as managed: deletion paths gate on is_managed, so provider input
        // cannot authorize branch deletion by spelling a valid ref.
        let managed = BranchRef::new("refs/heads/agent/e-1").unwrap();
        assert!(managed.is_managed());
        for outside in [
            "refs/heads/trunk",
            "refs/heads/main",
            "refs/heads/feature/x",
            "refs/heads/agentspoof/x",
        ] {
            let parsed = BranchRef::new(outside).unwrap();
            assert!(
                !parsed.is_managed(),
                "{outside:?} parses but must not count as managed"
            );
        }
    }
}
