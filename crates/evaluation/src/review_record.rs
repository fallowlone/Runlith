//! ADR-0033 trusted review records: exact bindings plus validation.
//!
//! Organization note: this crate cannot depend on `policy` (ADR-0018),
//! so the record carries validated primitive values; the service layer
//! converts `policy` authority types into these fields. Durable history,
//! replay detection, and supersession state live in the store/service
//! layers (later TASK-0145 phases).

use sha2::{Digest, Sha256};

/// Review path name, mirroring `policy::ReviewPath` across the
/// Evaluation/Policy crate boundary (ADR-0018). The service layer
/// converts explicitly; the two serializations must stay identical.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReviewPathName {
    B1,
    B2,
}

impl ReviewPathName {
    pub fn serialize(&self) -> &'static str {
        match self {
            Self::B1 => "review-path/v1:b1",
            Self::B2 => "review-path/v1:b2",
        }
    }
}

/// Explicit verdict name, mirroring `policy::ReviewVerdict`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReviewVerdictName {
    Approved,
    Rejected,
}

impl ReviewVerdictName {
    pub fn serialize(&self) -> &'static str {
        match self {
            Self::Approved => "approved",
            Self::Rejected => "rejected",
        }
    }
}

/// Constructor fields for [`ReviewRecord`]. Vectors are canonicalized
/// (sorted, deduplicated) at construction so the fingerprint is stable.
#[derive(Clone, Debug)]
pub struct ReviewRecordFields {
    pub record_id: String,
    pub task_id: String,
    pub requirement_revision: String,
    pub base_manifest: String,
    pub candidate_manifest: String,
    pub changed_inputs: Vec<String>,
    pub affected_checks: Vec<String>,
    pub policy_revision: u64,
    pub policy_fingerprint: String,
    pub setting_revision: u64,
    pub setting_path: ReviewPathName,
    pub reviewer_nonce: String,
    pub reviewer_input_digest: String,
    pub reviewer_verdict: ReviewVerdictName,
    pub implementer_execution_id: String,
    pub implementer_attempt_id: String,
    pub implementer_admission_ref: String,
    pub rationale: String,
    pub evidence_refs: Vec<String>,
    pub captured_at_ms: u64,
    pub supersedes: Option<String>,
}

/// Current values a record is validated against. Any inequality fails
/// closed: a changed binding requires a new review and assessment.
#[derive(Clone, Debug)]
pub struct ReviewExpectation {
    pub task_id: String,
    pub requirement_revision: String,
    pub base_manifest: String,
    pub candidate_manifest: String,
    pub changed_inputs: Vec<String>,
    pub affected_checks: Vec<String>,
    pub policy_revision: u64,
    pub policy_fingerprint: String,
    pub setting_revision: u64,
    pub setting_path: ReviewPathName,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ReviewError {
    MalformedRecord(String),
    BindingMismatch { field: &'static str },
    FingerprintMismatch,
    VerdictNotApproved,
    RecordStale,
}

impl std::fmt::Display for ReviewError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MalformedRecord(detail) => write!(f, "malformed review record: {detail}"),
            Self::BindingMismatch { field } => {
                write!(f, "review binding changed: {field}")
            }
            Self::FingerprintMismatch => write!(f, "review fingerprint mismatch"),
            Self::VerdictNotApproved => write!(f, "review verdict is not approved"),
            Self::RecordStale => write!(f, "review record is stale"),
        }
    }
}

impl std::error::Error for ReviewError {}

/// Trusted B1 review record with exact ADR-0033 bindings. Fields are
/// private so the fingerprint cannot drift from the validated values.
#[derive(Clone, Debug)]
pub struct ReviewRecord {
    record_id: String,
    task_id: String,
    requirement_revision: String,
    base_manifest: String,
    candidate_manifest: String,
    changed_inputs: Vec<String>,
    affected_checks: Vec<String>,
    policy_revision: u64,
    policy_fingerprint: String,
    setting_revision: u64,
    setting_path: ReviewPathName,
    reviewer_nonce: String,
    reviewer_input_digest: String,
    reviewer_verdict: ReviewVerdictName,
    implementer_execution_id: String,
    implementer_attempt_id: String,
    implementer_admission_ref: String,
    rationale: String,
    evidence_refs: Vec<String>,
    captured_at_ms: u64,
    supersedes: Option<String>,
    fingerprint: String,
}

impl ReviewRecord {
    pub fn new(fields: ReviewRecordFields) -> Result<Self, ReviewError> {
        let malformed = |detail: &str| ReviewError::MalformedRecord(detail.to_owned());
        if !valid_id(&fields.record_id) {
            return Err(malformed("record id"));
        }
        if !valid_id(&fields.task_id) {
            return Err(malformed("task id"));
        }
        if fields.requirement_revision.is_empty() || fields.requirement_revision.len() > 1024 {
            return Err(malformed("requirement revision"));
        }
        for (value, name) in [
            (&fields.base_manifest, "base manifest"),
            (&fields.candidate_manifest, "candidate manifest"),
            (&fields.policy_fingerprint, "policy fingerprint"),
            (&fields.reviewer_input_digest, "reviewer input digest"),
        ] {
            if !valid_sha256(value) {
                return Err(malformed(name));
            }
        }
        let changed_inputs = canonical_paths(fields.changed_inputs)
            .map_err(|detail| malformed(&format!("changed inputs: {detail}")))?;
        let affected_checks = canonical_checks(fields.affected_checks)
            .map_err(|detail| malformed(&format!("affected checks: {detail}")))?;
        if fields.policy_revision == 0 || fields.setting_revision == 0 {
            return Err(malformed("policy/setting revision"));
        }
        for (value, name) in [
            (&fields.reviewer_nonce, "reviewer nonce"),
            (&fields.implementer_execution_id, "implementer execution"),
            (&fields.implementer_attempt_id, "implementer attempt"),
            (&fields.implementer_admission_ref, "implementer admission"),
        ] {
            if !valid_token(value) {
                return Err(malformed(name));
            }
        }
        if fields.rationale.len() > 4096 {
            return Err(malformed("rationale"));
        }
        let evidence_refs = canonical_tokens(fields.evidence_refs, 128)
            .map_err(|detail| malformed(&format!("evidence refs: {detail}")))?;
        if fields
            .supersedes
            .as_deref()
            .is_some_and(|value| !valid_id(value))
        {
            return Err(malformed("supersedes"));
        }
        let mut record = Self {
            record_id: fields.record_id,
            task_id: fields.task_id,
            requirement_revision: fields.requirement_revision,
            base_manifest: fields.base_manifest,
            candidate_manifest: fields.candidate_manifest,
            changed_inputs,
            affected_checks,
            policy_revision: fields.policy_revision,
            policy_fingerprint: fields.policy_fingerprint,
            setting_revision: fields.setting_revision,
            setting_path: fields.setting_path,
            reviewer_nonce: fields.reviewer_nonce,
            reviewer_input_digest: fields.reviewer_input_digest,
            reviewer_verdict: fields.reviewer_verdict,
            implementer_execution_id: fields.implementer_execution_id,
            implementer_attempt_id: fields.implementer_attempt_id,
            implementer_admission_ref: fields.implementer_admission_ref,
            rationale: fields.rationale,
            evidence_refs,
            captured_at_ms: fields.captured_at_ms,
            supersedes: fields.supersedes,
            fingerprint: String::new(),
        };
        record.fingerprint = record.compute_fingerprint();
        Ok(record)
    }

    /// Rebuilds a record from durable parts, refusing on any fingerprint
    /// drift (the store layer's restore path).
    pub fn restore(fields: ReviewRecordFields, fingerprint: &str) -> Result<Self, ReviewError> {
        let record = Self::new(fields)?;
        if record.fingerprint != fingerprint {
            return Err(ReviewError::FingerprintMismatch);
        }
        Ok(record)
    }

    /// Validates the record against current values. Approved verdict,
    /// freshness, and every binding must hold; anything else fails
    /// closed and the changed checks stay ineligible.
    pub fn validate_current(
        &self,
        expected: &ReviewExpectation,
        now_ms: u64,
        max_age_ms: u64,
    ) -> Result<(), ReviewError> {
        if self.reviewer_verdict != ReviewVerdictName::Approved {
            return Err(ReviewError::VerdictNotApproved);
        }
        if self.captured_at_ms > now_ms || now_ms - self.captured_at_ms > max_age_ms {
            return Err(ReviewError::RecordStale);
        }
        let mismatch = |field| ReviewError::BindingMismatch { field };
        if self.task_id != expected.task_id {
            return Err(mismatch("task_id"));
        }
        if self.requirement_revision != expected.requirement_revision {
            return Err(mismatch("requirement_revision"));
        }
        if self.base_manifest != expected.base_manifest {
            return Err(mismatch("base_manifest"));
        }
        if self.candidate_manifest != expected.candidate_manifest {
            return Err(mismatch("candidate_manifest"));
        }
        if self.changed_inputs != canonical_paths_lossy(&expected.changed_inputs) {
            return Err(mismatch("changed_inputs"));
        }
        if self.affected_checks != canonical_checks_lossy(&expected.affected_checks) {
            return Err(mismatch("affected_checks"));
        }
        if self.policy_revision != expected.policy_revision {
            return Err(mismatch("policy_revision"));
        }
        if self.policy_fingerprint != expected.policy_fingerprint {
            return Err(mismatch("policy_fingerprint"));
        }
        if self.setting_revision != expected.setting_revision {
            return Err(mismatch("setting_revision"));
        }
        if self.setting_path != expected.setting_path {
            return Err(mismatch("setting_path"));
        }
        Ok(())
    }

    fn compute_fingerprint(&self) -> String {
        let mut hash = Sha256::new();
        hash.update(b"review-record\0");
        for field in [
            self.record_id.as_str(),
            self.task_id.as_str(),
            self.requirement_revision.as_str(),
            self.base_manifest.as_str(),
            self.candidate_manifest.as_str(),
            self.policy_fingerprint.as_str(),
            self.setting_path.serialize(),
            self.reviewer_nonce.as_str(),
            self.reviewer_input_digest.as_str(),
            self.reviewer_verdict.serialize(),
            self.implementer_execution_id.as_str(),
            self.implementer_attempt_id.as_str(),
            self.implementer_admission_ref.as_str(),
            self.rationale.as_str(),
        ] {
            hash.update(field.as_bytes());
            hash.update(b"\0");
        }
        for list in [
            &self.changed_inputs,
            &self.affected_checks,
            &self.evidence_refs,
        ] {
            for item in list {
                hash.update(item.as_bytes());
                hash.update(b"\0");
            }
            hash.update(b"\x01\0");
        }
        hash.update(self.policy_revision.to_be_bytes());
        hash.update(self.setting_revision.to_be_bytes());
        hash.update(self.captured_at_ms.to_be_bytes());
        if let Some(previous) = &self.supersedes {
            hash.update(previous.as_bytes());
        }
        hash.update(b"\0");
        format!("sha256:{:x}", hash.finalize())
    }

    pub fn record_id(&self) -> &str {
        &self.record_id
    }
    pub fn task_id(&self) -> &str {
        &self.task_id
    }
    pub fn requirement_revision(&self) -> &str {
        &self.requirement_revision
    }
    pub fn base_manifest(&self) -> &str {
        &self.base_manifest
    }
    pub fn candidate_manifest(&self) -> &str {
        &self.candidate_manifest
    }
    pub fn changed_inputs(&self) -> &[String] {
        &self.changed_inputs
    }
    pub fn affected_checks(&self) -> &[String] {
        &self.affected_checks
    }
    pub fn policy_revision(&self) -> u64 {
        self.policy_revision
    }
    pub fn policy_fingerprint(&self) -> &str {
        &self.policy_fingerprint
    }
    pub fn setting_revision(&self) -> u64 {
        self.setting_revision
    }
    pub fn setting_path(&self) -> ReviewPathName {
        self.setting_path
    }
    pub fn reviewer_nonce(&self) -> &str {
        &self.reviewer_nonce
    }
    pub fn reviewer_input_digest(&self) -> &str {
        &self.reviewer_input_digest
    }
    pub fn reviewer_verdict(&self) -> ReviewVerdictName {
        self.reviewer_verdict
    }
    pub fn implementer_execution_id(&self) -> &str {
        &self.implementer_execution_id
    }
    pub fn implementer_attempt_id(&self) -> &str {
        &self.implementer_attempt_id
    }
    pub fn implementer_admission_ref(&self) -> &str {
        &self.implementer_admission_ref
    }
    pub fn rationale(&self) -> &str {
        &self.rationale
    }
    pub fn evidence_refs(&self) -> &[String] {
        &self.evidence_refs
    }
    pub fn captured_at_ms(&self) -> u64 {
        self.captured_at_ms
    }
    pub fn supersedes(&self) -> Option<&str> {
        self.supersedes.as_deref()
    }
    pub fn fingerprint(&self) -> &str {
        &self.fingerprint
    }
}

fn valid_id(value: &str) -> bool {
    valid_token(value)
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

fn valid_path(path: &str) -> bool {
    !path.is_empty()
        && path.len() <= 1024
        && path.is_ascii()
        && !path.starts_with('/')
        && !path.contains('\\')
        && path
            .split('/')
            .all(|part| !part.is_empty() && part != "." && part != ".." && part != ".git")
}

fn valid_check(check: &str) -> bool {
    !check.trim().is_empty() && check.len() <= 256 && !check.chars().any(char::is_control)
}

fn canonical_paths(mut values: Vec<String>) -> Result<Vec<String>, String> {
    if values.is_empty() || values.len() > 1024 {
        return Err("at least one changed input is required".to_owned());
    }
    if values.iter().any(|value| !valid_path(value)) {
        return Err("invalid changed input".to_owned());
    }
    values.sort();
    values.dedup();
    Ok(values)
}

fn canonical_checks(mut values: Vec<String>) -> Result<Vec<String>, String> {
    if values.is_empty() || values.len() > 1024 {
        return Err("at least one affected check is required".to_owned());
    }
    if values.iter().any(|value| !valid_check(value)) {
        return Err("invalid affected check".to_owned());
    }
    values.sort();
    values.dedup();
    Ok(values)
}

fn canonical_tokens(mut values: Vec<String>, limit: usize) -> Result<Vec<String>, String> {
    if values.len() > limit {
        return Err("too many entries".to_owned());
    }
    if values.iter().any(|value| !valid_token(value)) {
        return Err("invalid entry".to_owned());
    }
    values.sort();
    values.dedup();
    Ok(values)
}

/// Canonicalizes expectation lists for comparison. Expectations come
/// from live current values; an uncanonicalizable list can never equal
/// a validated record list, so it maps to a sentinel mismatch.
fn canonical_paths_lossy(values: &[String]) -> Vec<String> {
    let mut sorted = values.to_vec();
    sorted.sort();
    sorted.dedup();
    sorted
}

fn canonical_checks_lossy(values: &[String]) -> Vec<String> {
    let mut sorted = values.to_vec();
    sorted.sort();
    sorted.dedup();
    sorted
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sha(byte: u8) -> String {
        format!("sha256:{}", (byte as char).to_string().repeat(64))
    }

    fn sample_fields() -> ReviewRecordFields {
        ReviewRecordFields {
            record_id: "review-1".to_owned(),
            task_id: "T-1".to_owned(),
            requirement_revision: "rev-1".to_owned(),
            base_manifest: sha(b'a'),
            candidate_manifest: sha(b'b'),
            changed_inputs: vec!["tests/unit.rs".to_owned()],
            affected_checks: vec!["cargo-test-unit".to_owned()],
            policy_revision: 7,
            policy_fingerprint: sha(b'c'),
            setting_revision: 3,
            setting_path: ReviewPathName::B1,
            reviewer_nonce: "nonce-1".to_owned(),
            reviewer_input_digest: sha(b'd'),
            reviewer_verdict: ReviewVerdictName::Approved,
            implementer_execution_id: "E-1".to_owned(),
            implementer_attempt_id: "A-1".to_owned(),
            implementer_admission_ref: "runner-admission-1".to_owned(),
            rationale: "covers the changed check".to_owned(),
            evidence_refs: vec!["effect-1".to_owned()],
            captured_at_ms: 1_700_000_000_000,
            supersedes: None,
        }
    }

    fn sample() -> ReviewRecord {
        ReviewRecord::new(sample_fields()).unwrap()
    }

    fn expectation() -> ReviewExpectation {
        ReviewExpectation {
            task_id: "T-1".to_owned(),
            requirement_revision: "rev-1".to_owned(),
            base_manifest: sha(b'a'),
            candidate_manifest: sha(b'b'),
            changed_inputs: vec!["tests/unit.rs".to_owned()],
            affected_checks: vec!["cargo-test-unit".to_owned()],
            policy_revision: 7,
            policy_fingerprint: sha(b'c'),
            setting_revision: 3,
            setting_path: ReviewPathName::B1,
        }
    }

    #[test]
    fn current_approved_record_validates() {
        let record = sample();
        record
            .validate_current(&expectation(), 1_700_000_100_000, 86_400_000)
            .unwrap();
    }

    #[test]
    fn changed_binding_fails_closed() {
        let record = sample();
        let mut changed = expectation();
        changed.candidate_manifest = sha(b'z');
        assert!(matches!(
            record.validate_current(&changed, 1_700_000_100_000, 86_400_000),
            Err(ReviewError::BindingMismatch { .. })
        ));
        let mut changed_path = expectation();
        changed_path.setting_path = ReviewPathName::B2;
        assert!(matches!(
            record.validate_current(&changed_path, 1_700_000_100_000, 86_400_000),
            Err(ReviewError::BindingMismatch { .. })
        ));
    }

    #[test]
    fn rejected_or_stale_record_fails_closed() {
        let mut fields = sample_fields();
        fields.reviewer_verdict = ReviewVerdictName::Rejected;
        let rejected = ReviewRecord::new(fields).unwrap();
        assert!(matches!(
            rejected.validate_current(&expectation(), 1_700_000_100_000, 86_400_000),
            Err(ReviewError::VerdictNotApproved)
        ));
        let record = sample();
        assert!(matches!(
            record.validate_current(&expectation(), 1_700_000_100_000 + 86_400_001, 86_400_000),
            Err(ReviewError::RecordStale)
        ));
    }

    #[test]
    fn malformed_fields_refuse_construction() {
        let mut fields = sample_fields();
        fields.changed_inputs = vec!["../escape.rs".to_owned()];
        assert!(ReviewRecord::new(fields.clone()).is_err());
        fields.changed_inputs = vec!["tests/unit.rs".to_owned()];
        fields.reviewer_input_digest = "bogus".to_owned();
        assert!(ReviewRecord::new(fields.clone()).is_err());
        fields.reviewer_input_digest = sha(b'd');
        fields.policy_revision = 0;
        assert!(ReviewRecord::new(fields).is_err());
    }

    #[test]
    fn restore_refuses_fingerprint_drift() {
        let record = sample();
        let fingerprint = record.fingerprint().to_owned();
        let restored = ReviewRecord::restore(sample_fields(), &fingerprint).unwrap();
        assert_eq!(restored.fingerprint(), fingerprint);
        assert!(matches!(
            ReviewRecord::restore(sample_fields(), &sha(b'z')),
            Err(ReviewError::FingerprintMismatch)
        ));
    }
}
