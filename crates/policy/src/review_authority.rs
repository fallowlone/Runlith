//! ADR-0033 review authority domain: the canonical B1/B2 setting value,
//! typed reviewer/implementation principals, and the explicit verdict.
//!
//! Organization note: this module owns shapes and validation only. Durable
//! history, nonce freshness, presence binding, and admission-time
//! selection live in the store/service layers (later TASK-0145 phases).

use sha2::{Digest, Sha256};

/// Canonical review path selected by the trusted owner-managed setting.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReviewPath {
    /// Human review: fresh interactive presence plus a personally
    /// authored verdict in the trusted UI.
    B1,
    /// Reviewer-service review: independently authenticated service and
    /// implementation principals under governed G5/G6 execution.
    B2,
}

impl ReviewPath {
    pub fn serialize(&self) -> String {
        match self {
            Self::B1 => "review-path/v1:b1".into(),
            Self::B2 => "review-path/v1:b2".into(),
        }
    }

    pub fn deserialize(serialized: &str) -> Result<Self, ReviewAuthorityError> {
        match serialized {
            "review-path/v1:b1" => Ok(Self::B1),
            "review-path/v1:b2" => Ok(Self::B2),
            _ => Err(ReviewAuthorityError("unknown review path encoding".into())),
        }
    }
}

/// Owner-managed setting value plus its immutable revision. Revisions only
/// grow; switching B1/B2 invalidates prior reviews (enforced by the
/// record layer binding this revision).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReviewPathSetting {
    revision: u64,
    path: ReviewPath,
    fingerprint: String,
}

impl ReviewPathSetting {
    pub fn new(revision: u64, path: ReviewPath) -> Result<Self, ReviewAuthorityError> {
        if revision == 0 {
            return Err(ReviewAuthorityError(
                "review path revision must be positive".into(),
            ));
        }
        let mut hash = Sha256::new();
        hash.update(b"review-path-setting\0");
        hash.update(revision.to_be_bytes());
        hash.update(path.serialize().as_bytes());
        Ok(Self {
            revision,
            path,
            fingerprint: format!("sha256:{:x}", hash.finalize()),
        })
    }

    pub fn restore(
        revision: u64,
        serialized: &str,
        fingerprint: &str,
    ) -> Result<Self, ReviewAuthorityError> {
        let path = ReviewPath::deserialize(serialized)?;
        let value = Self::new(revision, path)?;
        if value.fingerprint != fingerprint {
            return Err(ReviewAuthorityError(
                "stored review path fingerprint mismatch".into(),
            ));
        }
        Ok(value)
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn path(&self) -> &ReviewPath {
        &self.path
    }

    pub fn fingerprint(&self) -> &str {
        &self.fingerprint
    }

    pub fn serialize(&self) -> String {
        self.path.serialize()
    }
}

/// Explicit human verdict. There is no implicit approval: the trusted UI
/// captures exactly one of these per review.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReviewVerdict {
    Approved,
    Rejected,
}

impl ReviewVerdict {
    pub fn serialize(&self) -> &'static str {
        match self {
            Self::Approved => "approved",
            Self::Rejected => "rejected",
        }
    }
}

/// B1 reviewer: an interactive human actor. The trusted product process
/// populates this only from a fresh presence confirmation plus a
/// personally submitted verdict, bound to a fresh operation nonce and the
/// digest of the captured exact review input. Freshness itself is proven
/// by the store/service layers (nonce unseen, proof fresh); this type
/// only carries validated values.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HumanPrincipal {
    nonce: String,
    review_input_digest: String,
    verdict: ReviewVerdict,
}

impl HumanPrincipal {
    pub fn new(
        nonce: &str,
        review_input_digest: &str,
        verdict: ReviewVerdict,
    ) -> Result<Self, ReviewAuthorityError> {
        if !valid_token(nonce) {
            return Err(ReviewAuthorityError(
                "human principal requires a bounded operation nonce".into(),
            ));
        }
        if !valid_sha256(review_input_digest) {
            return Err(ReviewAuthorityError(
                "human principal requires a sha256 review-input digest".into(),
            ));
        }
        Ok(Self {
            nonce: nonce.to_owned(),
            review_input_digest: review_input_digest.to_owned(),
            verdict,
        })
    }

    pub fn namespace(&self) -> &'static str {
        "human/v1"
    }

    pub fn nonce(&self) -> &str {
        &self.nonce
    }

    pub fn review_input_digest(&self) -> &str {
        &self.review_input_digest
    }

    pub fn verdict(&self) -> &ReviewVerdict {
        &self.verdict
    }

    pub fn fingerprint(&self) -> String {
        let mut hash = Sha256::new();
        hash.update(b"human-principal\0");
        hash.update(self.nonce.as_bytes());
        hash.update(b"\0");
        hash.update(self.review_input_digest.as_bytes());
        hash.update(b"\0");
        hash.update(self.verdict.serialize().as_bytes());
        format!("sha256:{:x}", hash.finalize())
    }
}

/// Product-minted AI Attempt principal for the governed G5/G6
/// implementation execution. Derived only from trusted product records
/// (Runner admission provenance), never caller/provider text. A distinct
/// Rust type from [`HumanPrincipal`] so reviewer == implementer is
/// unrepresentable in a B1 pair.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImplementationPrincipal {
    execution_id: String,
    attempt_id: String,
    admission_ref: String,
}

impl ImplementationPrincipal {
    pub fn new(
        execution_id: &str,
        attempt_id: &str,
        admission_ref: &str,
    ) -> Result<Self, ReviewAuthorityError> {
        if !valid_token(execution_id) || !valid_token(attempt_id) || !valid_token(admission_ref) {
            return Err(ReviewAuthorityError(
                "implementation principal requires execution, attempt and admission refs".into(),
            ));
        }
        Ok(Self {
            execution_id: execution_id.to_owned(),
            attempt_id: attempt_id.to_owned(),
            admission_ref: admission_ref.to_owned(),
        })
    }

    pub fn namespace(&self) -> &'static str {
        "implementation/v1"
    }

    pub fn execution_id(&self) -> &str {
        &self.execution_id
    }

    pub fn attempt_id(&self) -> &str {
        &self.attempt_id
    }

    pub fn admission_ref(&self) -> &str {
        &self.admission_ref
    }

    pub fn fingerprint(&self) -> String {
        let mut hash = Sha256::new();
        hash.update(b"implementation-principal\0");
        hash.update(self.execution_id.as_bytes());
        hash.update(b"\0");
        hash.update(self.attempt_id.as_bytes());
        hash.update(b"\0");
        hash.update(self.admission_ref.as_bytes());
        format!("sha256:{:x}", hash.finalize())
    }
}

/// Fixed product-managed reviewer profile: the minimal mapping the
/// product (never callers/providers) resolves reviewers through. The
/// profile revision is bound into review records alongside the setting
/// and Policy revisions.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReviewerProfile {
    profile_id: String,
    revision: u64,
    key_ref: String,
    fingerprint: String,
}

impl ReviewerProfile {
    pub fn new(
        profile_id: &str,
        revision: u64,
        key_ref: &str,
    ) -> Result<Self, ReviewAuthorityError> {
        if !valid_token(profile_id) || !valid_token(key_ref) {
            return Err(ReviewAuthorityError(
                "reviewer profile requires an id and key reference".into(),
            ));
        }
        if revision == 0 {
            return Err(ReviewAuthorityError(
                "reviewer profile revision must be positive".into(),
            ));
        }
        let mut hash = Sha256::new();
        hash.update(b"reviewer-profile\0");
        hash.update(profile_id.as_bytes());
        hash.update(b"\0");
        hash.update(revision.to_be_bytes());
        hash.update(b"\0");
        hash.update(key_ref.as_bytes());
        Ok(Self {
            profile_id: profile_id.to_owned(),
            revision,
            key_ref: key_ref.to_owned(),
            fingerprint: format!("sha256:{:x}", hash.finalize()),
        })
    }

    pub fn restore(
        profile_id: &str,
        revision: u64,
        key_ref: &str,
        fingerprint: &str,
    ) -> Result<Self, ReviewAuthorityError> {
        let value = Self::new(profile_id, revision, key_ref)?;
        if value.fingerprint != fingerprint {
            return Err(ReviewAuthorityError(
                "stored reviewer profile fingerprint mismatch".into(),
            ));
        }
        Ok(value)
    }

    pub fn profile_id(&self) -> &str {
        &self.profile_id
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn key_ref(&self) -> &str {
        &self.key_ref
    }

    pub fn fingerprint(&self) -> &str {
        &self.fingerprint
    }

    pub fn serialize(&self) -> String {
        format!(
            "reviewer-profile/v1\nid:{}\nrev:{}\nkey:{}\n",
            self.profile_id, self.revision, self.key_ref
        )
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReviewAuthorityError(pub String);

impl std::fmt::Display for ReviewAuthorityError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for ReviewAuthorityError {}

impl From<ReviewAuthorityError> for crate::PolicyContractError {
    fn from(error: ReviewAuthorityError) -> Self {
        Self::InvalidReviewAuthority(error.0)
    }
}

fn valid_token(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 256
        && !value.chars().any(|c| c.is_control() || c.is_whitespace())
}

fn valid_sha256(value: &str) -> bool {
    value.strip_prefix("sha256:").is_some_and(|hex| {
        hex.len() == 64
            && hex
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn setting_selects_exactly_b1_or_b2_with_positive_revision() {
        let b1 = ReviewPathSetting::new(1, ReviewPath::B1).unwrap();
        assert_eq!(b1.path(), &ReviewPath::B1);
        assert_eq!(b1.revision(), 1);
        let b2 = ReviewPathSetting::new(2, ReviewPath::B2).unwrap();
        assert_eq!(b2.path(), &ReviewPath::B2);
        assert!(ReviewPathSetting::new(0, ReviewPath::B1).is_err());
        assert_ne!(b1.fingerprint(), b2.fingerprint());
    }

    #[test]
    fn setting_serialization_round_trips_and_rejects_unknown_paths() {
        let setting = ReviewPathSetting::new(3, ReviewPath::B1).unwrap();
        let restored = ReviewPathSetting::restore(
            setting.revision(),
            &setting.serialize(),
            setting.fingerprint(),
        )
        .unwrap();
        assert_eq!(restored, setting);
        assert!(ReviewPathSetting::restore(3, "review-path/v1:b9", setting.fingerprint()).is_err());
        assert!(ReviewPathSetting::restore(3, &setting.serialize(), "sha256:0").is_err());
    }

    #[test]
    fn human_principal_binds_nonce_input_digest_and_explicit_verdict() {
        let digest = format!("sha256:{}", "a".repeat(64));
        let human = HumanPrincipal::new("nonce-1", &digest, ReviewVerdict::Approved).unwrap();
        assert_eq!(human.verdict(), &ReviewVerdict::Approved);
        assert!(HumanPrincipal::new("", &digest, ReviewVerdict::Approved).is_err());
        assert!(HumanPrincipal::new("nonce-1", "not-a-digest", ReviewVerdict::Approved).is_err());
        assert!(HumanPrincipal::new("nonce-1", &digest, ReviewVerdict::Rejected).is_ok());
    }

    #[test]
    fn implementation_principal_requires_attempt_and_admission_provenance() {
        let principal = ImplementationPrincipal::new("E-1", "A-1", "runner-admission-1").unwrap();
        assert_eq!(principal.attempt_id(), "A-1");
        assert!(ImplementationPrincipal::new("", "A-1", "runner-admission-1").is_err());
        assert!(ImplementationPrincipal::new("E-1", "", "runner-admission-1").is_err());
        assert!(ImplementationPrincipal::new("E-1", "A-1", "").is_err());
    }

    #[test]
    fn reviewer_profile_is_revisioned_and_fingerprinted() {
        let profile = ReviewerProfile::new("owner-primary", 4, "key:platform-1").unwrap();
        assert_eq!(profile.revision(), 4);
        let restored =
            ReviewerProfile::restore("owner-primary", 4, "key:platform-1", profile.fingerprint())
                .unwrap();
        assert_eq!(restored, profile);
        assert!(ReviewerProfile::new("", 4, "key:platform-1").is_err());
        assert!(ReviewerProfile::new("owner-primary", 0, "key:platform-1").is_err());
        assert!(
            ReviewerProfile::restore("owner-primary", 4, "key:other", profile.fingerprint())
                .is_err()
        );
    }

    #[test]
    fn reviewer_and_implementer_are_distinct_types() {
        // Structural inequality: a B1 review pair cannot name the same
        // principal twice — the namespaces are different Rust types.
        fn assert_pair(_: &HumanPrincipal, _: &ImplementationPrincipal) {}
        let digest = format!("sha256:{}", "b".repeat(64));
        let human = HumanPrincipal::new("nonce-9", &digest, ReviewVerdict::Approved).unwrap();
        let implementer = ImplementationPrincipal::new("E-9", "A-9", "runner-admission-9").unwrap();
        assert_pair(&human, &implementer);
        assert_ne!(human.namespace(), implementer.namespace());
    }
}
