//! G8 evidence-requirement vocabulary (ADR-0030 §2/§3).
//!
//! A canonical Task carries, for every acceptance criterion, exactly one
//! explicit [`EvidenceRequirement`]: what observed evidence would satisfy the
//! criterion, which subject components its fingerprint must cover, and the
//! policy under which it was derived (if any). This replaces the TASK-0093
//! implicit `AutomatedTest` default: the v1 default still exists, but as
//! visible data stamped at authoring or migration time
//! ([`EvidenceRequirement::authored_default`]), never as an assumption a
//! reader must rediscover.
//!
//! Boundary: this module is pure vocabulary. It resolves no evidence,
//! constructs no `ObservedEffect` and evaluates no verdict (ADR-0030 §1,
//! TASK-0124/TASK-0125 own those). The plan-side `VerificationMethod` mapping
//! is owned by `plan-domain` (ADR-0030 §3) because task-domain must not depend
//! on the planning contract.

use crate::error::{DomainError, DomainResult};
use crate::task::MAX_TASK_METADATA_ITEM_LEN;

/// Closed requirement-kind registry, v1 (ADR-0030 §3). Unknown kinds fail
/// closed at every boundary; the registry grows only by ADR amendment.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum RequirementKind {
    /// Process exit envelope captured by the trusted harness.
    ProcessExit,
    /// Test-suite result (selector resolved by policy when unset).
    TestSuite,
    /// Artifact content hash.
    ArtifactHash,
    /// Filesystem state reading.
    FsState,
    /// Git state reading.
    GitState,
    /// Independent review verdict record.
    IndependentReview,
    /// G6 security-check result.
    SecurityCheck,
    /// Durable human approval (TASK-0062 exact-binding machinery).
    HumanApproval,
}

impl RequirementKind {
    /// Canonical text form (storage, hashing, wire adapters).
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ProcessExit => "process-exit",
            Self::TestSuite => "test-suite",
            Self::ArtifactHash => "artifact-hash",
            Self::FsState => "fs-state",
            Self::GitState => "git-state",
            Self::IndependentReview => "independent-review",
            Self::SecurityCheck => "security-check",
            Self::HumanApproval => "human-approval",
        }
    }

    /// Parses the canonical text form; unknown values fail closed.
    pub fn parse(value: &str) -> DomainResult<Self> {
        match value {
            "process-exit" => Ok(Self::ProcessExit),
            "test-suite" => Ok(Self::TestSuite),
            "artifact-hash" => Ok(Self::ArtifactHash),
            "fs-state" => Ok(Self::FsState),
            "git-state" => Ok(Self::GitState),
            "independent-review" => Ok(Self::IndependentReview),
            "security-check" => Ok(Self::SecurityCheck),
            "human-approval" => Ok(Self::HumanApproval),
            other => Err(DomainError::input(format!(
                "unknown evidence requirement kind {other:?}"
            ))),
        }
    }

    /// Whether this kind is satisfied by captured `ObservedEffect`s
    /// (`true`) or by its own durable evidence — review verdict, security
    /// result, human approval (ADR-0030 §3).
    #[must_use]
    pub const fn is_capture(self) -> bool {
        matches!(
            self,
            Self::ProcessExit
                | Self::TestSuite
                | Self::ArtifactHash
                | Self::FsState
                | Self::GitState
        )
    }

    /// v1 default subject binding per kind (ADR-0030 §2: what the subject
    /// fingerprint covers, per kind). Defensible minimums, not tunables:
    /// hashes and state readings bind content; process/test outcomes also
    /// depend on the toolchain; review/security/approval depend on both plus
    /// configuration.
    #[must_use]
    pub const fn default_subject_binding(self) -> SubjectBinding {
        match self {
            Self::ArtifactHash | Self::FsState | Self::GitState => SubjectBinding::Content,
            Self::ProcessExit => SubjectBinding::ContentAndToolchain,
            Self::TestSuite
            | Self::IndependentReview
            | Self::SecurityCheck
            | Self::HumanApproval => SubjectBinding::ContentToolchainAndConfig,
        }
    }
}

/// Which components of the subject an effect's fingerprint must cover
/// (ADR-0030 §2 `subject_binding`).
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum SubjectBinding {
    /// Subject content only.
    Content,
    /// Subject content plus the toolchain that produced or observed it.
    ContentAndToolchain,
    /// Subject content, toolchain and configuration.
    ContentToolchainAndConfig,
}

impl SubjectBinding {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Content => "content",
            Self::ContentAndToolchain => "content+toolchain",
            Self::ContentToolchainAndConfig => "content+toolchain+config",
        }
    }

    pub fn parse(value: &str) -> DomainResult<Self> {
        match value {
            "content" => Ok(Self::Content),
            "content+toolchain" => Ok(Self::ContentAndToolchain),
            "content+toolchain+config" => Ok(Self::ContentToolchainAndConfig),
            other => Err(DomainError::input(format!(
                "unknown subject binding {other:?}"
            ))),
        }
    }
}

/// Upper bound on kind-specific parameters per requirement.
pub const MAX_EVIDENCE_PARAMETERS: usize = 8;

/// One bounded kind-specific parameter (`key` unique within a requirement).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RequirementParam {
    key: String,
    value: String,
}

impl RequirementParam {
    /// Creates a validated parameter: both parts non-blank, bounded, free of
    /// control characters.
    pub fn new(key: impl Into<String>, value: impl Into<String>) -> DomainResult<Self> {
        let key = bounded_param("evidence parameter key", key.into())?;
        let value = bounded_param("evidence parameter value", value.into())?;
        Ok(Self { key, value })
    }

    #[must_use]
    pub fn key(&self) -> &str {
        &self.key
    }

    #[must_use]
    pub fn value(&self) -> &str {
        &self.value
    }
}

fn bounded_param(field: &str, value: String) -> DomainResult<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(DomainError::input(format!("{field} must not be blank")));
    }
    if trimmed.len() > MAX_TASK_METADATA_ITEM_LEN {
        return Err(DomainError::input(format!(
            "{field} exceeds {MAX_TASK_METADATA_ITEM_LEN} bytes"
        )));
    }
    if trimmed.chars().any(char::is_control) {
        return Err(DomainError::input(format!(
            "{field} must not contain control characters"
        )));
    }
    Ok(trimmed.to_owned())
}

/// One acceptance criterion's evidence requirement (ADR-0030 §2).
///
/// `policy_fingerprint` is `None` for requirements that were not derived
/// under a Policy fingerprint — the v1 authoring default and migrated legacy
/// rows. Per CV-7 such requirements are stale input and must be re-derived or
/// re-validated at the verification boundary; they are never treated as
/// current policy authority.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EvidenceRequirement {
    /// The acceptance criterion identity this requirement proves.
    pub criterion_id: String,
    /// Typed requirement kind (§3 registry).
    pub requirement_kind: RequirementKind,
    /// What the subject fingerprint must cover for this requirement.
    pub subject_binding: SubjectBinding,
    /// Kind-specific parameters; empty means "resolved by policy at check
    /// time" and is the documented v1 default.
    pub params: Vec<RequirementParam>,
    /// Policy fingerprint under which the requirement was derived, when one
    /// existed. `None` = not policy-derived (default/migrated).
    pub policy_fingerprint: Option<String>,
    /// Hard-floor flag (ADR-0030 §2 `required`, CV-7): policy-derived
    /// minimums cannot be weakened by task/session/UX configuration.
    pub required: bool,
}

impl EvidenceRequirement {
    /// Builds a validated requirement. Parameters are sorted by key and
    /// duplicate keys are refused (never silently merged).
    pub fn new(
        criterion_id: impl Into<String>,
        requirement_kind: RequirementKind,
        subject_binding: SubjectBinding,
        params: Vec<RequirementParam>,
        policy_fingerprint: Option<String>,
        required: bool,
    ) -> DomainResult<Self> {
        let criterion_id = bounded_param("criterion identity", criterion_id.into())?;
        if params.len() > MAX_EVIDENCE_PARAMETERS {
            return Err(DomainError::input(format!(
                "at most {MAX_EVIDENCE_PARAMETERS} evidence parameters are allowed"
            )));
        }
        let mut params = params;
        params.sort_by(|a, b| a.key.cmp(&b.key));
        for pair in params.windows(2) {
            if pair[0].key == pair[1].key {
                return Err(DomainError::input(format!(
                    "duplicate evidence parameter key {:?}",
                    pair[0].key
                )));
            }
        }
        let policy_fingerprint = policy_fingerprint
            .map(|fingerprint| bounded_param("policy fingerprint", fingerprint))
            .transpose()?;
        Ok(Self {
            criterion_id,
            requirement_kind,
            subject_binding,
            params,
            policy_fingerprint,
            required,
        })
    }

    /// The explicit v1 default (ADR-0030 §6): a `test-suite` requirement with
    /// the kind's default subject binding, no parameters (policy resolves the
    /// selector) and no policy fingerprint — stamped at authoring or
    /// migration time so the TASK-0093 default is visible data.
    pub fn authored_default(criterion_id: impl Into<String>) -> DomainResult<Self> {
        Self::new(
            criterion_id,
            RequirementKind::TestSuite,
            RequirementKind::TestSuite.default_subject_binding(),
            Vec::new(),
            None,
            true,
        )
    }
}

/// v1 singularity rule (ADR-0030 §1/§4): every acceptance criterion carries
/// exactly one requirement; a criterion without one, a requirement for an
/// unknown criterion, or a duplicate fails closed.
pub(crate) fn validate_requirement_coverage(
    acceptance: &[String],
    requirements: &[EvidenceRequirement],
) -> DomainResult<()> {
    for criterion in acceptance {
        let matches = requirements
            .iter()
            .filter(|requirement| requirement.criterion_id == *criterion)
            .count();
        match matches {
            1 => {}
            0 => {
                return Err(DomainError::input(format!(
                    "acceptance criterion {criterion:?} has no evidence requirement"
                )))
            }
            _ => {
                return Err(DomainError::input(format!(
                    "acceptance criterion {criterion:?} has multiple evidence requirements"
                )))
            }
        }
    }
    for requirement in requirements {
        if !acceptance.contains(&requirement.criterion_id) {
            return Err(DomainError::input(format!(
                "evidence requirement names criterion {:?}, which is not an acceptance criterion",
                requirement.criterion_id
            )));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kind_registry_is_closed_and_round_trips() {
        for kind in [
            RequirementKind::ProcessExit,
            RequirementKind::TestSuite,
            RequirementKind::ArtifactHash,
            RequirementKind::FsState,
            RequirementKind::GitState,
            RequirementKind::IndependentReview,
            RequirementKind::SecurityCheck,
            RequirementKind::HumanApproval,
        ] {
            assert_eq!(RequirementKind::parse(kind.as_str()).unwrap(), kind);
            let binding = kind.default_subject_binding();
            assert_eq!(SubjectBinding::parse(binding.as_str()).unwrap(), binding);
        }
        assert!(RequirementKind::parse("made-up").is_err());
        assert!(RequirementKind::parse("TEST-SUITE").is_err());
        assert!(SubjectBinding::parse("content+").is_err());
    }

    #[test]
    fn capture_partition_matches_the_registry() {
        assert!(RequirementKind::ProcessExit.is_capture());
        assert!(RequirementKind::TestSuite.is_capture());
        assert!(RequirementKind::ArtifactHash.is_capture());
        assert!(RequirementKind::FsState.is_capture());
        assert!(RequirementKind::GitState.is_capture());
        assert!(!RequirementKind::IndependentReview.is_capture());
        assert!(!RequirementKind::SecurityCheck.is_capture());
        assert!(!RequirementKind::HumanApproval.is_capture());
    }

    #[test]
    fn authored_default_is_explicit_test_suite_without_policy() {
        let requirement = EvidenceRequirement::authored_default("criterion-a").unwrap();
        assert_eq!(requirement.requirement_kind, RequirementKind::TestSuite);
        assert_eq!(
            requirement.subject_binding,
            SubjectBinding::ContentToolchainAndConfig
        );
        assert!(requirement.params.is_empty());
        assert!(requirement.policy_fingerprint.is_none());
        assert!(requirement.required);
    }

    #[test]
    fn parameters_are_bounded_sorted_and_unique() {
        let requirement = EvidenceRequirement::new(
            "c",
            RequirementKind::TestSuite,
            SubjectBinding::ContentAndToolchain,
            vec![
                RequirementParam::new("selector", "cargo test -p task-domain").unwrap(),
                RequirementParam::new("algorithm", "sha256").unwrap(),
            ],
            Some("policy:v1".to_owned()),
            true,
        )
        .unwrap();
        assert_eq!(requirement.params[0].key(), "algorithm");
        assert_eq!(requirement.params[1].key(), "selector");

        assert!(EvidenceRequirement::new(
            "c",
            RequirementKind::TestSuite,
            SubjectBinding::Content,
            vec![
                RequirementParam::new("k", "a").unwrap(),
                RequirementParam::new("k", "b").unwrap(),
            ],
            None,
            true,
        )
        .is_err());

        assert!(RequirementParam::new("  ", "v").is_err());
        assert!(RequirementParam::new("k", "line\nbreak").is_err());
    }

    #[test]
    fn coverage_requires_exactly_one_requirement_per_criterion() {
        let acceptance = vec!["a".to_owned(), "b".to_owned()];
        let complete = vec![
            EvidenceRequirement::authored_default("a").unwrap(),
            EvidenceRequirement::authored_default("b").unwrap(),
        ];
        validate_requirement_coverage(&acceptance, &complete).unwrap();

        let missing = vec![EvidenceRequirement::authored_default("a").unwrap()];
        assert!(validate_requirement_coverage(&acceptance, &missing).is_err());

        let duplicated = vec![
            EvidenceRequirement::authored_default("a").unwrap(),
            EvidenceRequirement::authored_default("a").unwrap(),
            EvidenceRequirement::authored_default("b").unwrap(),
        ];
        assert!(validate_requirement_coverage(&acceptance, &duplicated).is_err());

        let unknown = vec![
            EvidenceRequirement::authored_default("a").unwrap(),
            EvidenceRequirement::authored_default("b").unwrap(),
            EvidenceRequirement::authored_default("c").unwrap(),
        ];
        assert!(validate_requirement_coverage(&acceptance, &unknown).is_err());
    }
}
