//! G8 claim / claim-verification vocabulary and the pure completion
//! admission evaluation (ADR-0030 §2/§4/§5, TASK-0125).
//!
//! Boundary: this module is provider-neutral vocabulary plus a pure
//! function. It performs no IO, persists nothing, and commits no Task
//! transition — the trusted Task application path owns durability and the
//! authoritative `VERIFYING → COMPLETED` commit (ADR-0018). The rules
//! enforced here:
//!
//! - CV-1: an [`AgentClaim`] is never evidence; only [`ClaimVerification`]
//!   verdicts, CV-4 exemptions, and CV-8 discharges enter admission.
//! - CV-2: a verdict whose verifier equals the claim's `asserted_by` is
//!   ignored (self-verification is structurally ineligible).
//! - CV-3: a verdict/exemption/discharge counts only while its subject
//!   fingerprint equals the current fingerprint the trusted boundary
//!   derived at admission time.
//! - CV-4: exemptions apply only to capture-kind requirements, only with a
//!   current policy fingerprint (the typed `RiskClass::Low` gate is
//!   enforced by the trusted boundary before such a record exists).
//! - CV-5: superseded verdicts never count; nothing is deleted.
//! - CV-6: only VERIFIED passes; UNVERIFIED, BLOCKED, REFUTED and EXEMPT
//!   stay distinct in every outcome.
//! - CV-7: a requirement or exemption whose policy fingerprint differs
//!   from the current policy fingerprint is stale input and fails closed.
//! - CV-8: HUMAN_APPROVAL criteria discharge only through a durable,
//!   bound, unexpired, unconsumed approval discharge — never through an
//!   automated VERIFIED verdict (INV-086).
//!
//! CV-9 (evidence integrity under `TestMutationPolicy`) is not implemented
//! here: `TestMutationPolicy` is not a frozen contract yet (ADR-0018 leaves
//! its serialization open), so no verdict in this module depends on it.

use task_domain::{EvidenceRequirement, RequirementKind};

use crate::observed_effect::{canonical_timestamp, SubjectFingerprint};
use crate::EffectError;

/// Verifier identity minted by the trusted control plane for every
/// [`ClaimVerification`] it records (CV-2). Agents and providers can never
/// hold this identity; it is a constant of the trusted boundary, never
/// derived from provider output.
pub const TRUSTED_VERIFIER_IDENTITY: &str = "runlith.claim-verifier.v1";

/// Upper bound on any free-text field of a claim/verdict record.
pub const MAX_CLAIM_TEXT: usize = crate::MAX_EFFECT_TEXT;
/// Upper bound on reference entries per evidence list.
pub const MAX_EVIDENCE_REFS: usize = 16;

macro_rules! bounded_id {
    ($name:ident, $label:literal) => {
        impl $name {
            /// Validated constructor: non-blank, bounded, control-free.
            pub fn new(value: impl Into<String>) -> Result<Self, EffectError> {
                let value = value.into();
                let trimmed = value.trim();
                if trimmed.is_empty()
                    || trimmed.len() > MAX_CLAIM_TEXT
                    || trimmed.chars().any(char::is_control)
                {
                    return Err(EffectError::Invalid($label));
                }
                Ok(Self(trimmed.to_owned()))
            }

            /// Borrowed text form.
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }
    };
}

/// Durable claim identity.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct ClaimId(String);
/// Durable verdict identity.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct VerificationId(String);
/// Durable policy-exemption identity.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct ExemptionId(String);
/// Durable human-approval discharge identity.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct DischargeId(String);
/// Durable completion-admission record identity.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct AdmissionId(String);

bounded_id!(ClaimId, "invalid claim id");
bounded_id!(VerificationId, "invalid verification id");
bounded_id!(ExemptionId, "invalid exemption id");
bounded_id!(DischargeId, "invalid discharge id");
bounded_id!(AdmissionId, "invalid admission id");

impl std::fmt::Display for ClaimId {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}
impl std::fmt::Display for VerificationId {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

/// What an agent asserts about one acceptance criterion (ADR-0030 §2).
/// Never evidence: a claim enters admission only as the *target* a
/// `ClaimVerification` judged (CV-1).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ClaimedOutcome {
    /// The agent claims the criterion is satisfied.
    Satisfied,
    /// The agent claims the criterion cannot be satisfied.
    Refuted,
    /// The agent makes no decidable claim.
    Unknown,
}

impl ClaimedOutcome {
    /// Canonical text form (storage).
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Satisfied => "SATISFIED",
            Self::Refuted => "REFUTED",
            Self::Unknown => "UNKNOWN",
        }
    }

    /// Parses the canonical text form; unknown values fail closed.
    pub fn parse(value: &str) -> Result<Self, EffectError> {
        match value {
            "SATISFIED" => Ok(Self::Satisfied),
            "REFUTED" => Ok(Self::Refuted),
            "UNKNOWN" => Ok(Self::Unknown),
            _ => Err(EffectError::Invalid("unknown claimed outcome")),
        }
    }
}

/// Bounded evidence references (`receipt:*`, `effect:*`, `session:*`) —
/// references only, never payloads, transcripts, or secrets (INV-008,
/// INV-151). Storage form is canonical length-delimited text.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct EvidenceRefs {
    /// Runtime receipts (provenance only, INV-072).
    pub receipt_refs: Vec<String>,
    /// Observed-effect references.
    pub effect_refs: Vec<String>,
    /// Session references.
    pub session_refs: Vec<String>,
}

impl EvidenceRefs {
    /// Validated constructor: every reference bounded, control-free, and
    /// the list sizes bounded.
    pub fn new(
        receipt_refs: Vec<String>,
        effect_refs: Vec<String>,
        session_refs: Vec<String>,
    ) -> Result<Self, EffectError> {
        let refs = Self {
            receipt_refs,
            effect_refs,
            session_refs,
        };
        refs.validate()?;
        Ok(refs)
    }

    fn validate(&self) -> Result<(), EffectError> {
        for list in [&self.receipt_refs, &self.effect_refs, &self.session_refs] {
            if list.len() > MAX_EVIDENCE_REFS {
                return Err(EffectError::Invalid("too many evidence references"));
            }
            for reference in list {
                let trimmed = reference.trim();
                if trimmed.is_empty()
                    || trimmed.len() > MAX_CLAIM_TEXT
                    || trimmed.chars().any(char::is_control)
                {
                    return Err(EffectError::Invalid("invalid evidence reference"));
                }
            }
        }
        Ok(())
    }

    /// Whether every list is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.receipt_refs.is_empty() && self.effect_refs.is_empty() && self.session_refs.is_empty()
    }

    /// Canonical length-delimited encoding of one list (`len:value` pairs,
    /// concatenated), so storage round-trips cannot drift or collide.
    fn encode_list(list: &[String]) -> String {
        let mut encoded = String::new();
        for reference in list {
            encoded.push_str(&format!("{}:{reference}", reference.len()));
        }
        encoded
    }

    fn decode_list(encoded: &str) -> Result<Vec<String>, EffectError> {
        let mut refs = Vec::new();
        let mut cursor = 0;
        while cursor < encoded.len() {
            let colon = encoded[cursor..]
                .find(':')
                .map(|offset| cursor + offset)
                .ok_or(EffectError::Invalid(
                    "malformed evidence reference encoding",
                ))?;
            let length: usize = encoded[cursor..colon]
                .parse()
                .map_err(|_| EffectError::Invalid("malformed evidence reference encoding"))?;
            let start = colon + 1;
            let end = start + length;
            if end > encoded.len() {
                return Err(EffectError::Invalid(
                    "malformed evidence reference encoding",
                ));
            }
            refs.push(encoded[start..end].to_owned());
            cursor = end;
        }
        Ok(refs)
    }

    /// Canonical storage form of the receipt list.
    #[must_use]
    pub fn encode_receipts(&self) -> String {
        Self::encode_list(&self.receipt_refs)
    }

    /// Canonical storage form of the effect list.
    #[must_use]
    pub fn encode_effects(&self) -> String {
        Self::encode_list(&self.effect_refs)
    }

    /// Canonical storage form of the session list.
    #[must_use]
    pub fn encode_sessions(&self) -> String {
        Self::encode_list(&self.session_refs)
    }

    /// Rebuilds from the three canonical storage forms; malformed encodings
    /// fail closed.
    pub fn decode(receipts: &str, effects: &str, sessions: &str) -> Result<Self, EffectError> {
        Self::new(
            Self::decode_list(receipts)?,
            Self::decode_list(effects)?,
            Self::decode_list(sessions)?,
        )
    }
}

/// The subject an agent claim binds: an opaque reference plus the
/// fingerprint of the exact subject state the claim is about (CV-3).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ClaimedSubject {
    /// Opaque subject reference (bounded, never a payload).
    pub subject_ref: String,
    /// Fingerprint of the claimed subject state.
    pub subject_fingerprint: SubjectFingerprint,
}

/// A statement by an agent about its own work product (ADR-0030 §2).
/// Claims are durable for audit and for CV-2/CV-3 binding; they never
/// carry authority (INV-146).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AgentClaim {
    /// Durable identity (minted by the trusted boundary).
    pub claim_id: ClaimId,
    /// The acceptance criterion this claim is about.
    pub criterion_id: String,
    /// What the agent asserts.
    pub claimed_outcome: ClaimedOutcome,
    /// The bound subject.
    pub claimed_subject: ClaimedSubject,
    /// Bounded evidence references the agent offers.
    pub claimed_evidence: EvidenceRefs,
    /// Agent/provider identity that asserted the claim (data, never
    /// authority; CV-2 compares it against the verifier).
    pub asserted_by: String,
    /// RFC3339 assertion timestamp (trusted clock).
    pub asserted_at: String,
}

impl AgentClaim {
    /// Validated constructor used for both minting and storage restore:
    /// every field bounded, timestamps canonical.
    pub fn new(
        claim_id: ClaimId,
        criterion_id: impl Into<String>,
        claimed_outcome: ClaimedOutcome,
        claimed_subject: ClaimedSubject,
        claimed_evidence: EvidenceRefs,
        asserted_by: impl Into<String>,
        asserted_at: impl Into<String>,
    ) -> Result<Self, EffectError> {
        let criterion_id = checked_text("invalid criterion id", criterion_id.into())?;
        let subject_ref = checked_text(
            "invalid claimed subject reference",
            claimed_subject.subject_ref,
        )?;
        let asserted_by = checked_text("invalid claim asserter", asserted_by.into())?;
        let asserted_at = canonical_timestamp(asserted_at.into())?;
        Ok(Self {
            claim_id,
            criterion_id,
            claimed_outcome,
            claimed_subject: ClaimedSubject {
                subject_ref,
                subject_fingerprint: claimed_subject.subject_fingerprint,
            },
            claimed_evidence,
            asserted_by,
            asserted_at,
        })
    }
}

/// The G8 verdict taxonomy (ADR-0030 §4). Exactly these five values;
/// unknown values fail closed at every boundary.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ClaimVerdict {
    /// Trusted evidence satisfies the requirement.
    Verified,
    /// Trusted evidence contradicts the claim.
    Refuted,
    /// Evidence is missing, stale, or not yet gathered (default state
    /// reached by doing nothing).
    Unverified,
    /// Verification could not run (tooling/infra/policy block) — never a
    /// pass.
    Blocked,
    /// A policy exemption discharged the requirement (CV-4) — not a pass,
    /// and never conflated with VERIFIED (CV-6).
    Exempt,
}

impl ClaimVerdict {
    /// Canonical text form (storage).
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Verified => "VERIFIED",
            Self::Refuted => "REFUTED",
            Self::Unverified => "UNVERIFIED",
            Self::Blocked => "BLOCKED",
            Self::Exempt => "EXEMPT",
        }
    }

    /// Parses the canonical text form; unknown values fail closed.
    pub fn parse(value: &str) -> Result<Self, EffectError> {
        match value {
            "VERIFIED" => Ok(Self::Verified),
            "REFUTED" => Ok(Self::Refuted),
            "UNVERIFIED" => Ok(Self::Unverified),
            "BLOCKED" => Ok(Self::Blocked),
            "EXEMPT" => Ok(Self::Exempt),
            _ => Err(EffectError::Invalid("unknown claim verdict")),
        }
    }

    /// CV-6: only VERIFIED counts as a pass.
    #[must_use]
    pub const fn is_pass(self) -> bool {
        matches!(self, Self::Verified)
    }
}

/// A trusted verdict over one [`AgentClaim`] (ADR-0030 §2). Recorded only
/// by the trusted control plane; supersession is by reference, never
/// deletion (CV-5).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ClaimVerification {
    /// Durable identity (minted by the trusted boundary).
    pub verification_id: VerificationId,
    /// The claim this verdict judges.
    pub claim_id: ClaimId,
    /// Verifier identity (always the trusted boundary, CV-2).
    pub verifier: String,
    /// The requirement kind this verdict was produced against.
    pub requirement_kind: RequirementKind,
    /// Fingerprint of the exact subject state the verdict binds (CV-3).
    pub subject_fingerprint: SubjectFingerprint,
    /// Exact requirement revision the verdict was produced against
    /// (ADR-0032 rule 4; `sha256:` content hash).
    pub requirement_revision: String,
    /// Policy fingerprint echoed from the judged requirement (ADR-0032
    /// rule 4; `None` for policy-free legacy requirements, matched
    /// exactly at admission).
    pub policy_fingerprint: Option<String>,
    /// The verdict.
    pub verdict: ClaimVerdict,
    /// Bounded basis references supporting the verdict.
    pub basis: EvidenceRefs,
    /// Supersession chain: the verdict that replaced this one (CV-5).
    pub superseded_by: Option<VerificationId>,
    /// RFC3339 decision timestamp (trusted clock).
    pub decided_at: String,
}

impl ClaimVerification {
    /// Validated constructor used for both minting and storage restore.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        verification_id: VerificationId,
        claim_id: ClaimId,
        verifier: impl Into<String>,
        requirement_kind: RequirementKind,
        subject_fingerprint: SubjectFingerprint,
        requirement_revision: impl Into<String>,
        policy_fingerprint: Option<String>,
        verdict: ClaimVerdict,
        basis: EvidenceRefs,
        superseded_by: Option<VerificationId>,
        decided_at: impl Into<String>,
    ) -> Result<Self, EffectError> {
        let verifier = checked_text("invalid verifier identity", verifier.into())?;
        let decided_at = canonical_timestamp(decided_at.into())?;
        let requirement_revision = requirement_revision.into();
        if !crate::candidate::valid_sha256(&requirement_revision) {
            return Err(EffectError::Invalid("invalid requirement revision"));
        }
        let policy_fingerprint = policy_fingerprint
            .map(|fp| checked_text("invalid policy fingerprint", fp))
            .transpose()?;
        Ok(Self {
            verification_id,
            claim_id,
            verifier,
            requirement_kind,
            subject_fingerprint,
            requirement_revision,
            policy_fingerprint,
            verdict,
            basis,
            superseded_by,
            decided_at,
        })
    }
}

/// A durable policy exemption (ADR-0030 §2, CV-4). The typed
/// `RiskClass::Low` gate is enforced by the trusted boundary before this
/// record may exist; the schema repeats it. Never issuable for
/// HUMAN_APPROVAL / INDEPENDENT_REVIEW / SECURITY_CHECK requirements.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PolicyExemption {
    /// Durable identity.
    pub exemption_id: ExemptionId,
    /// The criterion exempted, or `None` for a whole-task exemption.
    pub criterion_id: Option<String>,
    /// Fingerprint of the exact subject state exempted (CV-3 binding).
    pub subject_fingerprint: SubjectFingerprint,
    /// Policy fingerprint the exemption was issued under (CV-7).
    pub policy_fingerprint: String,
    /// RFC3339 creation timestamp (trusted clock).
    pub created_at: String,
}

impl PolicyExemption {
    /// Validated constructor used for both minting and storage restore.
    pub fn new(
        exemption_id: ExemptionId,
        criterion_id: Option<String>,
        subject_fingerprint: SubjectFingerprint,
        policy_fingerprint: impl Into<String>,
        created_at: impl Into<String>,
    ) -> Result<Self, EffectError> {
        let criterion_id = criterion_id
            .map(|criterion| checked_text("invalid exemption criterion", criterion))
            .transpose()?;
        let policy_fingerprint = checked_text(
            "invalid exemption policy fingerprint",
            policy_fingerprint.into(),
        )?;
        let created_at = canonical_timestamp(created_at.into())?;
        Ok(Self {
            exemption_id,
            criterion_id,
            subject_fingerprint,
            policy_fingerprint,
            created_at,
        })
    }
}

/// A durable human-approval discharge of a HUMAN_APPROVAL requirement
/// (ADR-0030 §5 CV-8, INV-150): bound to the exact criterion, subject
/// fingerprint, and risk, valid only inside its window, single-use per
/// (criterion, subject fingerprint).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ApprovalDischarge {
    /// Durable identity.
    pub discharge_id: DischargeId,
    /// The criterion discharged.
    pub criterion_id: String,
    /// Fingerprint of the exact subject state approved (INV-150).
    pub subject_fingerprint: SubjectFingerprint,
    /// Typed risk text the approval covers (`low|medium|high|critical`).
    pub risk: String,
    /// Reference to the durable approval record (TASK-0062 machinery).
    pub approval_ref: String,
    /// RFC3339 start of the validity window (inclusive).
    pub not_valid_before: String,
    /// RFC3339 end of the validity window (exclusive).
    pub not_valid_after: String,
    /// The completion admission that consumed this discharge (single-use).
    pub consumed_by: Option<AdmissionId>,
    /// RFC3339 creation timestamp (trusted clock).
    pub created_at: String,
}

impl ApprovalDischarge {
    /// Validated constructor used for both minting and storage restore.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        discharge_id: DischargeId,
        criterion_id: impl Into<String>,
        subject_fingerprint: SubjectFingerprint,
        risk: impl Into<String>,
        approval_ref: impl Into<String>,
        not_valid_before: impl Into<String>,
        not_valid_after: impl Into<String>,
        consumed_by: Option<AdmissionId>,
        created_at: impl Into<String>,
    ) -> Result<Self, EffectError> {
        let criterion_id = checked_text("invalid discharge criterion", criterion_id.into())?;
        let risk = checked_text("invalid discharge risk", risk.into())?;
        if !matches!(risk.as_str(), "low" | "medium" | "high" | "critical") {
            return Err(EffectError::Invalid("unknown discharge risk class"));
        }
        let approval_ref = checked_text("invalid approval reference", approval_ref.into())?;
        let not_valid_before = canonical_timestamp(not_valid_before.into())?;
        let not_valid_after = canonical_timestamp(not_valid_after.into())?;
        if not_valid_before >= not_valid_after {
            return Err(EffectError::Invalid("discharge validity window is empty"));
        }
        let created_at = canonical_timestamp(created_at.into())?;
        Ok(Self {
            discharge_id,
            criterion_id,
            subject_fingerprint,
            risk,
            approval_ref,
            not_valid_before,
            not_valid_after,
            consumed_by,
            created_at,
        })
    }
}

fn checked_text(label: &'static str, value: String) -> Result<String, EffectError> {
    let trimmed = value.trim();
    if trimmed.is_empty() || trimmed.len() > MAX_CLAIM_TEXT || trimmed.chars().any(char::is_control)
    {
        return Err(EffectError::Invalid(label));
    }
    Ok(trimmed.to_owned())
}

/// Per-criterion admission status (ADR-0030 §4 taxonomy, CV-6): the five
/// outcomes stay typed and distinct in every roll-up; only `Verified`
/// satisfies a requirement directly and only `Exempt` discharges one via
/// policy. `Unverified` is the default state reached by doing nothing.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CriterionStatus {
    /// Current VERIFIED `ClaimVerification` (or a valid CV-8 discharge)
    /// satisfies the requirement.
    Verified,
    /// A current REFUTED verdict contradicts the claim; stays visible.
    Refuted,
    /// No current pass evidence: missing, stale, or never gathered.
    Unverified,
    /// Verification was attempted but could not run; never a pass.
    Blocked,
    /// Discharged by a durable CV-4 policy exemption; not a pass.
    Exempt,
}

impl CriterionStatus {
    /// Canonical text form (durable admission records).
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Verified => "VERIFIED",
            Self::Refuted => "REFUTED",
            Self::Unverified => "UNVERIFIED",
            Self::Blocked => "BLOCKED",
            Self::Exempt => "EXEMPT",
        }
    }

    /// Parses the canonical text form; unknown values fail closed.
    pub fn parse(value: &str) -> Result<Self, EffectError> {
        Ok(match value {
            "VERIFIED" => Self::Verified,
            "REFUTED" => Self::Refuted,
            "UNVERIFIED" => Self::Unverified,
            "BLOCKED" => Self::Blocked,
            "EXEMPT" => Self::Exempt,
            _ => return Err(EffectError::Invalid("unknown criterion status")),
        })
    }

    /// Whether this outcome admits completion for its criterion.
    #[must_use]
    pub const fn admits(self) -> bool {
        matches!(self, Self::Verified | Self::Exempt)
    }
}

/// One criterion's typed admission outcome with its replayable evidence
/// reference (ADR-0018 consequence: auditable, replayable).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CriterionOutcome {
    /// The acceptance criterion evaluated.
    pub criterion_id: String,
    /// Typed outcome.
    pub status: CriterionStatus,
    /// The verdict / exemption / discharge identity behind the outcome.
    pub evidence_ref: Option<String>,
    /// Short human-readable detail (derived, bounded).
    pub detail: String,
}

/// Everything the trusted boundary assembled for one criterion: the
/// durable requirement, the current subject fingerprint it re-derived at
/// admission time (CV-3), and the durable claims/verdicts/exemptions /
/// discharge for the criterion. Pure input — no IO here.
#[derive(Clone, Debug, Default)]
pub struct CriterionEvidence<'a> {
    /// The criterion's durable evidence requirement.
    pub requirement: Option<&'a EvidenceRequirement>,
    /// Current subject fingerprint derived by the trusted boundary.
    pub current_fingerprint: Option<&'a SubjectFingerprint>,
    /// Durable claims for the criterion (never evidence — CV-1; used for
    /// CV-2 eligibility and CV-3 binding only).
    pub claims: Vec<&'a AgentClaim>,
    /// Durable verdicts for the criterion, superseded included (CV-5).
    pub verdicts: Vec<&'a ClaimVerification>,
    /// Durable policy exemptions in scope (task-wide and criterion).
    pub exemptions: Vec<&'a PolicyExemption>,
    /// Durable human-approval discharge for the criterion (CV-8).
    pub discharge: Option<&'a ApprovalDischarge>,
    /// Current integrity basis (CV-9); `None` when no assessment was
    /// resolved or the criterion is not candidate-shaped.
    pub integrity: Option<IntegrityBasis>,
    /// Task-bound observed effects the boundary resolved for basis
    /// references (rule 4 resolvability).
    pub resolved_effects: Vec<ResolvedEffect>,
}

/// Current integrity basis for one criterion (ADR-0032 rule 4, CV-9).
/// Assembled by the trusted boundary from the integrity assessment of
/// the relied-on candidate execution; the pure core only compares.
/// Absent on [`CriterionEvidence`] when no assessment was resolved —
/// CV-9 then fails closed for candidate-shaped criteria.
///
/// Assessment/result digest equality (rule 4) holds structurally: the
/// TASK-0144 boundary derives the result's subject fingerprint from the
/// assessment (requirement revision, policy, selector triple, invocation
/// and manifest digests), and admission pins every relied-on effect to
/// the current subject. Cross-revision base staleness awaits the
/// policy-evolution procedure (single Forbidden revision today); the
/// check here binds the assessment to its execution's base row.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IntegrityBasis {
    /// No tamper signal on the assessment.
    pub clean: bool,
    /// Requirement revision the assessment evaluated.
    pub requirement_revision: String,
    /// Evaluated base the assessment ran under.
    pub policy_fingerprint: String,
    /// Evaluated base bound to the relied-on execution (cross-checked
    /// against the base row, not the assessment's own claim).
    pub evaluated_base: String,
}

/// One resolved effect-basis row (ADR-0032 rule 4): the boundary resolved
/// the reference to a durable observed effect bound to this task.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResolvedEffect {
    pub effect_id: String,
    pub subject_fingerprint: SubjectFingerprint,
}

/// Admission-time context supplied by the trusted boundary.
#[derive(Clone, Copy, Debug, Default)]
pub struct AdmissionContext<'a> {
    /// The task's typed risk text, when it carries one (CV-8 binding).
    pub task_risk: Option<&'a str>,
    /// The current policy fingerprint (CV-7). `None` means no policy
    /// derivation is in force: only requirements authored without a policy
    /// fingerprint are current, and no exemption is valid.
    pub current_policy_fingerprint: Option<&'a str>,
    /// RFC3339 admission timestamp (trusted clock).
    pub now: &'a str,
}

/// The admission decision over every acceptance criterion of one task.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AdmissionDecision {
    /// Every criterion is VERIFIED or EXEMPT: completion may proceed.
    Admit,
    /// At least one criterion is REFUTED / UNVERIFIED / BLOCKED:
    /// completion is refused (fail closed).
    Refuse,
}

/// The typed, durable completion-admission verdict (ADR-0030 §4/CV-6).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AdmissionReport {
    /// Overall decision.
    pub decision: AdmissionDecision,
    /// Per-criterion outcomes in requirement order.
    pub outcomes: Vec<CriterionOutcome>,
}

impl AdmissionReport {
    /// Whether completion is admitted.
    #[must_use]
    pub const fn is_admit(&self) -> bool {
        matches!(self.decision, AdmissionDecision::Admit)
    }

    /// Bounded one-line refusal summary for the trusted caller's error
    /// path. Never a verdict by itself — the typed report is authoritative.
    #[must_use]
    pub fn refusal_summary(&self) -> String {
        let mut summary = String::from("completion admission refused:");
        for outcome in &self.outcomes {
            if outcome.status.admits() {
                continue;
            }
            let part = format!(
                " {}={}({});",
                outcome.criterion_id,
                outcome.status.as_str(),
                outcome.detail
            );
            if summary.len() + part.len() > MAX_CLAIM_TEXT {
                summary.push_str(" ...");
                break;
            }
            summary.push_str(&part);
        }
        summary
    }

    /// Canonical length-delimited encoding of the outcomes for the durable
    /// completion-admission record.
    #[must_use]
    pub fn encode_outcomes(&self) -> String {
        let mut encoded = String::new();
        for outcome in &self.outcomes {
            encoded.push_str(&format!(
                "{}:{}",
                outcome.criterion_id.len(),
                outcome.criterion_id
            ));
            encoded.push_str(&format!(
                "{}:{}",
                outcome.status.as_str().len(),
                outcome.status.as_str()
            ));
            let reference = outcome.evidence_ref.as_deref().unwrap_or("");
            encoded.push_str(&format!("{}:{reference}", reference.len()));
            encoded.push_str(&format!("{}:{}", outcome.detail.len(), outcome.detail));
        }
        encoded
    }

    /// Rebuilds outcomes from the canonical encoding; malformed input
    /// fails closed.
    pub fn decode_outcomes(encoded: &str) -> Result<Vec<CriterionOutcome>, EffectError> {
        fn take(encoded: &str, cursor: &mut usize) -> Result<String, EffectError> {
            if *cursor >= encoded.len() {
                return Err(EffectError::Invalid("truncated admission outcome encoding"));
            }
            let colon = encoded[*cursor..]
                .find(':')
                .map(|offset| *cursor + offset)
                .ok_or(EffectError::Invalid("malformed admission outcome encoding"))?;
            let length: usize = encoded[*cursor..colon]
                .parse()
                .map_err(|_| EffectError::Invalid("malformed admission outcome encoding"))?;
            let start = colon + 1;
            let end = start + length;
            if end > encoded.len() {
                return Err(EffectError::Invalid("malformed admission outcome encoding"));
            }
            *cursor = end;
            Ok(encoded[start..end].to_owned())
        }
        let mut outcomes = Vec::new();
        let mut cursor = 0;
        while cursor < encoded.len() {
            let criterion_id = take(encoded, &mut cursor)?;
            let status = CriterionStatus::parse(&take(encoded, &mut cursor)?)?;
            let reference = take(encoded, &mut cursor)?;
            let detail = take(encoded, &mut cursor)?;
            outcomes.push(CriterionOutcome {
                criterion_id,
                status,
                evidence_ref: if reference.is_empty() {
                    None
                } else {
                    Some(reference)
                },
                detail,
            });
        }
        Ok(outcomes)
    }
}

/// Evaluates completion admission over every criterion of one task
/// (ADR-0030 §5, ADR-0018 boundary). Pure: the trusted boundary supplies
/// durable inputs and owns the authoritative commit. UNVERIFIED is the
/// default state reached by doing nothing; REFUTED stays visible and
/// blocks; BLOCKED never passes; EXEMPT discharges only under CV-4.
#[must_use]
pub fn evaluate_admission(
    criteria: &[CriterionEvidence<'_>],
    context: &AdmissionContext<'_>,
) -> AdmissionReport {
    let now_valid = canonical_timestamp(context.now.to_owned()).is_ok();
    let outcomes: Vec<CriterionOutcome> = criteria
        .iter()
        .map(|criterion| evaluate_criterion(criterion, context, now_valid))
        .collect();
    let decision = if outcomes.iter().all(|outcome| outcome.status.admits()) {
        AdmissionDecision::Admit
    } else {
        AdmissionDecision::Refuse
    };
    AdmissionReport { decision, outcomes }
}

fn evaluate_criterion(
    criterion: &CriterionEvidence<'_>,
    context: &AdmissionContext<'_>,
    now_valid: bool,
) -> CriterionOutcome {
    let Some(requirement) = criterion.requirement else {
        return outcome(
            "?",
            CriterionStatus::Unverified,
            None,
            "criterion has no requirement",
        );
    };
    let id = requirement.criterion_id.as_str();
    // CV-7: a requirement is valid only under the policy fingerprint it was
    // derived with; `None` pairs only with `None` (no policy derivation in
    // force). Stale input fails closed.
    if requirement.policy_fingerprint.as_deref() != context.current_policy_fingerprint {
        return outcome(
            id,
            CriterionStatus::Unverified,
            None,
            "requirement policy fingerprint is stale",
        );
    }
    if requirement.requirement_kind == RequirementKind::HumanApproval {
        return evaluate_discharge(criterion, context, now_valid, id);
    }
    evaluate_verdicts(criterion, context, requirement)
}

/// CV-8: a HUMAN_APPROVAL requirement discharges only via a durable
/// approval bound to (criterion, subject fingerprint, risk, validity
/// window), unconsumed and unexpired. Automated verdicts are never even
/// consulted here (INV-086).
fn evaluate_discharge(
    criterion: &CriterionEvidence<'_>,
    context: &AdmissionContext<'_>,
    now_valid: bool,
    id: &str,
) -> CriterionOutcome {
    let Some(discharge) = criterion.discharge else {
        return outcome(
            id,
            CriterionStatus::Unverified,
            None,
            "human approval required; no durable approval recorded",
        );
    };
    let bound_subject = criterion.current_fingerprint == Some(&discharge.subject_fingerprint);
    let in_window = now_valid
        && discharge.not_valid_before.as_str() <= context.now
        && context.now < discharge.not_valid_after.as_str();
    let unconsumed = discharge.consumed_by.is_none();
    let risk_bound = context
        .task_risk
        .is_none_or(|task_risk| task_risk == discharge.risk);
    if discharge.criterion_id == id && bound_subject && in_window && unconsumed && risk_bound {
        return outcome(
            id,
            CriterionStatus::Verified,
            Some(discharge.discharge_id.as_str().to_owned()),
            "durable human approval discharge bound to subject and risk",
        );
    }
    outcome(
        id,
        CriterionStatus::Unverified,
        None,
        "human approval discharge is missing, expired, consumed, or bound to another subject/risk",
    )
}

/// CV-1..CV-7 over capture and review/security kinds: claims are never
/// evidence; self-verified verdicts are ineligible; only current
/// (unsuperseded), subject-bound verdicts count; REFUTED stays visible and
/// blocks; exemptions discharge only capture kinds under a current policy
/// fingerprint; BLOCKED never passes.
fn evaluate_verdicts(
    criterion: &CriterionEvidence<'_>,
    context: &AdmissionContext<'_>,
    requirement: &EvidenceRequirement,
) -> CriterionOutcome {
    let id = requirement.criterion_id.as_str();
    // CV-2: a verdict is eligible only when its claim is durable here and
    // the verifier differs from the claim's asserter.
    let eligible: Vec<&ClaimVerification> = criterion
        .verdicts
        .iter()
        .filter(|verdict| verdict.superseded_by.is_none())
        // The verdict must have been produced against this requirement kind
        // (ADR-0030 §2/§3: capture kinds stay distinct — a PROCESS_EXIT
        // verdict cannot discharge a TEST_SUITE requirement).
        .filter(|verdict| verdict.requirement_kind == requirement.requirement_kind)
        .filter(|verdict| {
            criterion.claims.iter().any(|claim| {
                claim.claim_id == verdict.claim_id && claim.asserted_by != verdict.verifier
            })
        })
        .copied()
        .collect();
    let binds_current = |verdict: &ClaimVerification| {
        criterion.current_fingerprint == Some(&verdict.subject_fingerprint)
    };
    // Contradiction wins over any pass: a current REFUTED bound to the
    // current subject stays visible and blocks admission (CV-5/CV-6).
    if let Some(verdict) = eligible
        .iter()
        .find(|verdict| verdict.verdict == ClaimVerdict::Refuted && binds_current(verdict))
    {
        return outcome(
            id,
            CriterionStatus::Refuted,
            Some(verdict.verification_id.as_str().to_owned()),
            "current REFUTED verdict contradicts the claim",
        );
    }
    // ADR-0032 rule 4: VERIFIED requires the exact requirement revision
    // and policy echo, a nonempty basis resolving to current-subject
    // effects, and — for candidate-shaped requirements — a clean current
    // integrity assessment whose revision, base and manifest/invocation
    // digests all match. Anything else fails closed to UNVERIFIED.
    let expected_revision = crate::candidate::requirement_revision(requirement);
    let basis_resolves = |verdict: &ClaimVerification| {
        !verdict.basis.effect_refs.is_empty()
            && verdict.basis.effect_refs.iter().all(|reference| {
                criterion.resolved_effects.iter().any(|resolved| {
                    resolved.effect_id == *reference
                        && criterion.current_fingerprint == Some(&resolved.subject_fingerprint)
                })
            })
    };
    let integrity_holds = || {
        if requirement.params.is_empty() {
            return true;
        }
        if crate::candidate::CandidateSelector::from_requirement(requirement).is_err() {
            return false;
        }
        let Some(integrity) = criterion.integrity.as_ref() else {
            return false;
        };
        integrity.clean
            && integrity.requirement_revision == expected_revision
            && integrity.policy_fingerprint == integrity.evaluated_base
    };
    if let Some(verdict) = eligible.iter().find(|verdict| {
        verdict.verdict == ClaimVerdict::Verified
            && binds_current(verdict)
            && verdict.requirement_revision == expected_revision
            && verdict.policy_fingerprint == requirement.policy_fingerprint
            && basis_resolves(verdict)
            && integrity_holds()
    }) {
        return outcome(
            id,
            CriterionStatus::Verified,
            Some(verdict.verification_id.as_str().to_owned()),
            "current VERIFIED claim verification binds the current subject",
        );
    }
    // CV-4: exemptions never apply to HUMAN_APPROVAL / INDEPENDENT_REVIEW /
    // SECURITY_CHECK kinds (the record-time gate repeats this) and only
    // under a current policy fingerprint (CV-7) bound to the current
    // subject (CV-3).
    if requirement.requirement_kind.is_capture() {
        if let Some(exemption) = criterion.exemptions.iter().find(|exemption| {
            (exemption.criterion_id.is_none() || exemption.criterion_id.as_deref() == Some(id))
                && Some(exemption.policy_fingerprint.as_str()) == context.current_policy_fingerprint
                && criterion.current_fingerprint == Some(&exemption.subject_fingerprint)
        }) {
            return outcome(
                id,
                CriterionStatus::Exempt,
                Some(exemption.exemption_id.as_str().to_owned()),
                "policy exemption (typed low risk) discharges the requirement",
            );
        }
    }
    if let Some(verdict) = eligible
        .iter()
        .find(|verdict| verdict.verdict == ClaimVerdict::Blocked)
    {
        return outcome(
            id,
            CriterionStatus::Blocked,
            Some(verdict.verification_id.as_str().to_owned()),
            "verification could not run; blocked never counts as pass",
        );
    }
    outcome(
        id,
        CriterionStatus::Unverified,
        None,
        "no current VERIFIED verdict binds the current subject fingerprint",
    )
}

fn outcome(
    criterion_id: &str,
    status: CriterionStatus,
    evidence_ref: Option<String>,
    detail: &str,
) -> CriterionOutcome {
    CriterionOutcome {
        criterion_id: criterion_id.to_owned(),
        status,
        evidence_ref,
        detail: detail.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::candidate::requirement_revision;
    use task_domain::{EvidenceRequirement, RequirementKind, SubjectBinding};

    const NOW: &str = "2026-09-24T12:00:00Z";

    fn fingerprint(byte: char) -> SubjectFingerprint {
        SubjectFingerprint::new(format!("sha256:{}", byte.to_string().repeat(64))).unwrap()
    }

    fn requirement(kind: RequirementKind) -> EvidenceRequirement {
        EvidenceRequirement::new(
            "criterion-a",
            kind,
            SubjectBinding::Content,
            vec![],
            None,
            true,
        )
        .unwrap()
    }

    fn claim(id: &str, asserted_by: &str, fp: &SubjectFingerprint) -> AgentClaim {
        AgentClaim::new(
            ClaimId::new(id).unwrap(),
            "criterion-a",
            ClaimedOutcome::Satisfied,
            ClaimedSubject {
                subject_ref: "subject-1".to_owned(),
                subject_fingerprint: fp.clone(),
            },
            EvidenceRefs::default(),
            asserted_by,
            NOW,
        )
        .unwrap()
    }

    const REVISION_A: &str =
        "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const POLICY_A: &str =
        "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

    fn verdict(
        id: &str,
        claim: &AgentClaim,
        kind: RequirementKind,
        fp: &SubjectFingerprint,
        verdict: ClaimVerdict,
    ) -> ClaimVerification {
        ClaimVerification::new(
            VerificationId::new(id).unwrap(),
            claim.claim_id.clone(),
            TRUSTED_VERIFIER_IDENTITY,
            kind,
            fp.clone(),
            REVISION_A,
            Some(POLICY_A.to_owned()),
            verdict,
            EvidenceRefs::default(),
            None,
            NOW,
        )
        .unwrap()
    }

    fn context() -> AdmissionContext<'static> {
        AdmissionContext {
            task_risk: None,
            current_policy_fingerprint: None,
            now: NOW,
        }
    }

    fn single(
        requirement: &EvidenceRequirement,
        current: Option<&SubjectFingerprint>,
        claims: Vec<&AgentClaim>,
        verdicts: Vec<&ClaimVerification>,
    ) -> AdmissionReport {
        let evidence = [CriterionEvidence {
            requirement: Some(requirement),
            current_fingerprint: current,
            claims,
            verdicts,
            exemptions: Vec::new(),
            discharge: None,
            integrity: None,
            resolved_effects: Vec::new(),
        }];
        evaluate_admission(&evidence, &context())
    }

    fn single_full(
        requirement: &EvidenceRequirement,
        current: Option<&SubjectFingerprint>,
        claims: Vec<&AgentClaim>,
        verdicts: Vec<&ClaimVerification>,
        integrity: Option<IntegrityBasis>,
        resolved_effects: Vec<ResolvedEffect>,
        current_policy: Option<&str>,
    ) -> AdmissionReport {
        let evidence = [CriterionEvidence {
            requirement: Some(requirement),
            current_fingerprint: current,
            claims,
            verdicts,
            exemptions: Vec::new(),
            discharge: None,
            integrity,
            resolved_effects,
        }];
        let context = AdmissionContext {
            current_policy_fingerprint: current_policy,
            ..context()
        };
        evaluate_admission(&evidence, &context)
    }

    fn candidate_requirement() -> EvidenceRequirement {
        use task_domain::RequirementParam;
        EvidenceRequirement::new(
            "criterion-a",
            RequirementKind::TestSuite,
            SubjectBinding::ContentToolchainAndConfig,
            vec![
                RequirementParam::new("literal", "needle").unwrap(),
                RequirementParam::new("path", "src/lib.rs").unwrap(),
                RequirementParam::new("selector", "candidate.file-contains.v1").unwrap(),
            ],
            Some(REQUIREMENT_FP.to_owned()),
            true,
        )
        .unwrap()
    }

    const EVALUATED_BASE: &str =
        "sha256:dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd";

    const REQUIREMENT_FP: &str =
        "sha256:cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";

    fn integrity_for(revision: &str) -> IntegrityBasis {
        IntegrityBasis {
            clean: true,
            requirement_revision: revision.to_owned(),
            policy_fingerprint: EVALUATED_BASE.to_owned(),
            evaluated_base: EVALUATED_BASE.to_owned(),
        }
    }

    fn resolved(effect_id: &str, fp: &SubjectFingerprint) -> ResolvedEffect {
        ResolvedEffect {
            effect_id: effect_id.to_owned(),
            subject_fingerprint: fp.clone(),
        }
    }

    fn with_basis(verdict: &mut ClaimVerification, effect_id: &str) {
        verdict.basis = EvidenceRefs::new(vec![], vec![effect_id.to_owned()], vec![]).unwrap();
    }

    fn bind_full(req: &EvidenceRequirement, verdict: &mut ClaimVerification) {
        verdict.requirement_revision = requirement_revision(req);
        verdict.policy_fingerprint = req.policy_fingerprint.clone();
        with_basis(verdict, "effect-1");
    }

    #[test]
    fn cv1_agent_claim_alone_never_satisfies_a_requirement() {
        let req = requirement(RequirementKind::TestSuite);
        let fp = fingerprint('a');
        // Even a confident claim carrying evidence references is not
        // evidence: without an independent ClaimVerification the criterion
        // stays UNVERIFIED and admission refuses.
        let mut claim = claim("claim-1", "agent-1", &fp);
        claim.claimed_evidence = EvidenceRefs::new(
            vec!["receipt:r-1".to_owned()],
            vec!["effect-1".to_owned()],
            vec![],
        )
        .unwrap();
        let report = single(&req, Some(&fp), vec![&claim], vec![]);
        assert_eq!(report.decision, AdmissionDecision::Refuse);
        assert_eq!(report.outcomes[0].status, CriterionStatus::Unverified);
        assert!(!report.outcomes[0].status.admits());
    }

    #[test]
    fn cv1_cv6_unverified_is_the_default_state_reached_by_doing_nothing() {
        let req = requirement(RequirementKind::TestSuite);
        let report = single(&req, None, vec![], vec![]);
        assert_eq!(report.decision, AdmissionDecision::Refuse);
        assert_eq!(report.outcomes[0].status, CriterionStatus::Unverified);
    }

    #[test]
    fn cv2_self_verified_verdict_is_ineligible() {
        let req = requirement(RequirementKind::TestSuite);
        let fp = fingerprint('a');
        let agent_claim = claim("claim-1", "agent-1", &fp);
        // A verdict minted by the evaluated agent itself is ignored.
        let self_verdict = ClaimVerification::new(
            VerificationId::new("verification-self").unwrap(),
            agent_claim.claim_id.clone(),
            "agent-1",
            RequirementKind::TestSuite,
            fp.clone(),
            REVISION_A,
            Some(POLICY_A.to_owned()),
            ClaimVerdict::Verified,
            EvidenceRefs::default(),
            None,
            NOW,
        )
        .unwrap();
        let report = single(&req, Some(&fp), vec![&agent_claim], vec![&self_verdict]);
        assert_eq!(report.decision, AdmissionDecision::Refuse);
        assert_eq!(report.outcomes[0].status, CriterionStatus::Unverified);

        // The trusted verifier identity differs from the asserter: eligible.
        let mut trusted = verdict(
            "verification-1",
            &agent_claim,
            RequirementKind::TestSuite,
            &fp,
            ClaimVerdict::Verified,
        );
        bind_full(&req, &mut trusted);
        let report = single_full(
            &req,
            Some(&fp),
            vec![&agent_claim],
            vec![&trusted],
            None,
            vec![resolved("effect-1", &fp)],
            None,
        );
        assert_eq!(report.decision, AdmissionDecision::Admit);
        assert_eq!(report.outcomes[0].status, CriterionStatus::Verified);
        assert_eq!(
            report.outcomes[0].evidence_ref.as_deref(),
            Some("verification-1")
        );
    }

    #[test]
    fn cv3_stale_subject_fingerprint_fails_closed() {
        let req = requirement(RequirementKind::TestSuite);
        let fp = fingerprint('a');
        let moved = fingerprint('b');
        let agent_claim = claim("claim-1", "agent-1", &fp);
        let verified = verdict(
            "verification-1",
            &agent_claim,
            RequirementKind::TestSuite,
            &fp,
            ClaimVerdict::Verified,
        );
        // Subject moved since the verdict: the verdict is stale input.
        let report = single(&req, Some(&moved), vec![&agent_claim], vec![&verified]);
        assert_eq!(report.decision, AdmissionDecision::Refuse);
        assert_eq!(report.outcomes[0].status, CriterionStatus::Unverified);
        // No derivable current fingerprint: fail closed as well.
        let report = single(&req, None, vec![&agent_claim], vec![&verified]);
        assert_eq!(report.decision, AdmissionDecision::Refuse);
        // Same subject state: the binding holds.
        let mut current = verified.clone();
        bind_full(&req, &mut current);
        let report = single_full(
            &req,
            Some(&fp),
            vec![&agent_claim],
            vec![&current],
            None,
            vec![resolved("effect-1", &fp)],
            None,
        );
        assert_eq!(report.decision, AdmissionDecision::Admit);
    }

    fn exemption(
        id: &str,
        criterion: Option<&str>,
        fp: &SubjectFingerprint,
        policy: &str,
    ) -> PolicyExemption {
        PolicyExemption::new(
            ExemptionId::new(id).unwrap(),
            criterion.map(str::to_owned),
            fp.clone(),
            policy,
            NOW,
        )
        .unwrap()
    }

    fn single_with_exemptions(
        requirement: &EvidenceRequirement,
        current: Option<&SubjectFingerprint>,
        exemptions: Vec<&PolicyExemption>,
        policy: Option<&str>,
    ) -> AdmissionReport {
        let evidence = [CriterionEvidence {
            requirement: Some(requirement),
            current_fingerprint: current,
            claims: Vec::new(),
            verdicts: Vec::new(),
            exemptions,
            discharge: None,
            integrity: None,
            resolved_effects: Vec::new(),
        }];
        let context = AdmissionContext {
            current_policy_fingerprint: policy,
            ..context()
        };
        evaluate_admission(&evidence, &context)
    }

    #[test]
    fn cv4_exemption_discharges_only_capture_kinds_under_current_policy() {
        let fp = fingerprint('a');
        let policy_req = |kind: RequirementKind| {
            EvidenceRequirement::new(
                "criterion-a",
                kind,
                SubjectBinding::Content,
                vec![],
                Some("policy:v1".to_owned()),
                true,
            )
            .unwrap()
        };
        let exempt = exemption("exemption-1", Some("criterion-a"), &fp, "policy:v1");

        // Capture kind + current policy fingerprint + bound subject: EXEMPT.
        let capture_req = policy_req(RequirementKind::TestSuite);
        let report =
            single_with_exemptions(&capture_req, Some(&fp), vec![&exempt], Some("policy:v1"));
        assert_eq!(report.decision, AdmissionDecision::Admit);
        assert_eq!(report.outcomes[0].status, CriterionStatus::Exempt);
        assert_eq!(
            report.outcomes[0].evidence_ref.as_deref(),
            Some("exemption-1")
        );

        // INDEPENDENT_REVIEW / SECURITY_CHECK can never be exempted.
        for kind in [
            RequirementKind::IndependentReview,
            RequirementKind::SecurityCheck,
        ] {
            let protected_req = policy_req(kind);
            let report =
                single_with_exemptions(&protected_req, Some(&fp), vec![&exempt], Some("policy:v1"));
            assert_eq!(report.decision, AdmissionDecision::Refuse, "{kind:?}");
            assert_eq!(report.outcomes[0].status, CriterionStatus::Unverified);
        }

        // Exemption issued under another policy fingerprint: stale (CV-7),
        // even while the requirement itself is current.
        let stale_exempt = exemption("exemption-2", Some("criterion-a"), &fp, "policy:v2");
        let report = single_with_exemptions(
            &capture_req,
            Some(&fp),
            vec![&stale_exempt],
            Some("policy:v1"),
        );
        assert_eq!(report.decision, AdmissionDecision::Refuse);
        // Exemption bound to a moved subject: stale (CV-3 binding).
        let moved = fingerprint('b');
        let report =
            single_with_exemptions(&capture_req, Some(&moved), vec![&exempt], Some("policy:v1"));
        assert_eq!(report.decision, AdmissionDecision::Refuse);
    }

    #[test]
    fn cv5_superseded_verdicts_never_count_and_refuted_stays_visible() {
        let req = requirement(RequirementKind::TestSuite);
        let fp = fingerprint('a');
        let agent_claim = claim("claim-1", "agent-1", &fp);
        let mut old = verdict(
            "verification-1",
            &agent_claim,
            RequirementKind::TestSuite,
            &fp,
            ClaimVerdict::Verified,
        );
        let new_id = VerificationId::new("verification-2").unwrap();
        old.superseded_by = Some(new_id.clone());
        // A superseded VERIFIED no longer counts.
        let report = single(&req, Some(&fp), vec![&agent_claim], vec![&old]);
        assert_eq!(report.decision, AdmissionDecision::Refuse);
        assert_eq!(report.outcomes[0].status, CriterionStatus::Unverified);

        // A superseded REFUTED no longer blocks once its replacement is current.
        let mut old_refuted = verdict(
            "verification-1",
            &agent_claim,
            RequirementKind::TestSuite,
            &fp,
            ClaimVerdict::Refuted,
        );
        old_refuted.superseded_by = Some(new_id);
        let mut current = verdict(
            "verification-2",
            &agent_claim,
            RequirementKind::TestSuite,
            &fp,
            ClaimVerdict::Verified,
        );
        bind_full(&req, &mut current);
        let report = single_full(
            &req,
            Some(&fp),
            vec![&agent_claim],
            vec![&old_refuted, &current],
            None,
            vec![resolved("effect-1", &fp)],
            None,
        );
        assert_eq!(report.decision, AdmissionDecision::Admit);

        // A current REFUTED stays visible and wins over a current VERIFIED:
        // contradiction is never hidden.
        let contradiction = verdict(
            "verification-3",
            &agent_claim,
            RequirementKind::TestSuite,
            &fp,
            ClaimVerdict::Refuted,
        );
        let report = single(
            &req,
            Some(&fp),
            vec![&agent_claim],
            vec![&current, &contradiction],
        );
        assert_eq!(report.decision, AdmissionDecision::Refuse);
        assert_eq!(report.outcomes[0].status, CriterionStatus::Refuted);
        assert_eq!(
            report.outcomes[0].evidence_ref.as_deref(),
            Some("verification-3")
        );
    }

    #[test]
    fn cv6_blocked_unverified_exempt_never_count_as_pass() {
        let req = requirement(RequirementKind::TestSuite);
        let fp = fingerprint('a');
        let agent_claim = claim("claim-1", "agent-1", &fp);
        let blocked = verdict(
            "verification-1",
            &agent_claim,
            RequirementKind::TestSuite,
            &fp,
            ClaimVerdict::Blocked,
        );
        let report = single(&req, Some(&fp), vec![&agent_claim], vec![&blocked]);
        assert_eq!(report.decision, AdmissionDecision::Refuse);
        assert_eq!(report.outcomes[0].status, CriterionStatus::Blocked);
        // BLOCKED stays distinct from UNVERIFIED in the typed outcome even
        // though both refuse.
        assert_ne!(CriterionStatus::Blocked, CriterionStatus::Unverified);

        assert!(ClaimVerdict::Verified.is_pass());
        for verdict in [
            ClaimVerdict::Refuted,
            ClaimVerdict::Unverified,
            ClaimVerdict::Blocked,
            ClaimVerdict::Exempt,
        ] {
            assert!(!verdict.is_pass(), "{verdict:?} must never count as pass");
        }
        assert!(CriterionStatus::Verified.admits());
        assert!(CriterionStatus::Exempt.admits());
        assert!(!CriterionStatus::Refuted.admits());
        assert!(!CriterionStatus::Unverified.admits());
        assert!(!CriterionStatus::Blocked.admits());
    }

    #[test]
    fn cv7_stale_policy_fingerprint_invalidates_requirement() {
        let fp = fingerprint('a');
        let agent_claim = claim("claim-1", "agent-1", &fp);
        let policy_req = EvidenceRequirement::new(
            "criterion-a",
            RequirementKind::TestSuite,
            SubjectBinding::Content,
            vec![],
            Some("policy:v1".to_owned()),
            true,
        )
        .unwrap();
        let mut verified = verdict(
            "verification-1",
            &agent_claim,
            RequirementKind::TestSuite,
            &fp,
            ClaimVerdict::Verified,
        );
        bind_full(&policy_req, &mut verified);
        let evidence = [CriterionEvidence {
            requirement: Some(&policy_req),
            current_fingerprint: Some(&fp),
            claims: vec![&agent_claim],
            verdicts: vec![&verified],
            exemptions: Vec::new(),
            discharge: None,
            integrity: None,
            resolved_effects: vec![resolved("effect-1", &fp)],
        }];
        // Policy moved on: the requirement itself is stale input.
        let stale = AdmissionContext {
            current_policy_fingerprint: Some("policy:v2"),
            ..context()
        };
        let report = evaluate_admission(&evidence, &stale);
        assert_eq!(report.decision, AdmissionDecision::Refuse);
        assert_eq!(report.outcomes[0].status, CriterionStatus::Unverified);
        assert!(report.outcomes[0].detail.contains("policy fingerprint"));
        // No policy derivation in force cannot validate a derived requirement.
        let report = evaluate_admission(&evidence, &context());
        assert_eq!(report.decision, AdmissionDecision::Refuse);
        // Current fingerprint: the requirement evaluates normally.
        let current = AdmissionContext {
            current_policy_fingerprint: Some("policy:v1"),
            ..context()
        };
        let report = evaluate_admission(&evidence, &current);
        assert_eq!(report.decision, AdmissionDecision::Admit);
    }

    fn discharge(
        id: &str,
        fp: &SubjectFingerprint,
        risk: &str,
        before: &str,
        after: &str,
        consumed: Option<AdmissionId>,
    ) -> ApprovalDischarge {
        ApprovalDischarge::new(
            DischargeId::new(id).unwrap(),
            "criterion-a",
            fp.clone(),
            risk,
            "approval-req-1",
            before,
            after,
            consumed,
            NOW,
        )
        .unwrap()
    }

    fn human_report(
        discharge: Option<&ApprovalDischarge>,
        current: Option<&SubjectFingerprint>,
        task_risk: Option<&str>,
        verdicts: Vec<&ClaimVerification>,
        claims: Vec<&AgentClaim>,
    ) -> AdmissionReport {
        let req = requirement(RequirementKind::HumanApproval);
        let evidence = [CriterionEvidence {
            requirement: Some(&req),
            current_fingerprint: current,
            claims,
            verdicts,
            exemptions: Vec::new(),
            discharge,
            integrity: None,
            resolved_effects: Vec::new(),
        }];
        let context = AdmissionContext {
            task_risk,
            ..context()
        };
        evaluate_admission(&evidence, &context)
    }

    #[test]
    fn cv8_human_approval_discharges_only_via_durable_bound_approval() {
        let fp = fingerprint('a');
        let valid = discharge(
            "discharge-1",
            &fp,
            "low",
            "2026-01-01T00:00:00Z",
            "2027-01-01T00:00:00Z",
            None,
        );
        let report = human_report(Some(&valid), Some(&fp), Some("low"), vec![], vec![]);
        assert_eq!(report.decision, AdmissionDecision::Admit);
        assert_eq!(report.outcomes[0].status, CriterionStatus::Verified);
        assert_eq!(
            report.outcomes[0].evidence_ref.as_deref(),
            Some("discharge-1")
        );

        // INV-086: an automated VERIFIED verdict never discharges a
        // HUMAN_APPROVAL requirement.
        let agent_claim = claim("claim-1", "agent-1", &fp);
        let automated = verdict(
            "verification-1",
            &agent_claim,
            RequirementKind::HumanApproval,
            &fp,
            ClaimVerdict::Verified,
        );
        let report = human_report(
            None,
            Some(&fp),
            Some("low"),
            vec![&automated],
            vec![&agent_claim],
        );
        assert_eq!(report.decision, AdmissionDecision::Refuse);
        assert_eq!(report.outcomes[0].status, CriterionStatus::Unverified);

        // Expired window.
        let expired = discharge(
            "discharge-2",
            &fp,
            "low",
            "2020-01-01T00:00:00Z",
            "2021-01-01T00:00:00Z",
            None,
        );
        let report = human_report(Some(&expired), Some(&fp), Some("low"), vec![], vec![]);
        assert_eq!(report.decision, AdmissionDecision::Refuse);
        // Already consumed (single-use).
        let consumed = discharge(
            "discharge-3",
            &fp,
            "low",
            "2026-01-01T00:00:00Z",
            "2027-01-01T00:00:00Z",
            Some(AdmissionId::new("admission-1").unwrap()),
        );
        let report = human_report(Some(&consumed), Some(&fp), Some("low"), vec![], vec![]);
        assert_eq!(report.decision, AdmissionDecision::Refuse);
        // Risk mismatch against the task's risk.
        let report = human_report(Some(&valid), Some(&fp), Some("high"), vec![], vec![]);
        assert_eq!(report.decision, AdmissionDecision::Refuse);
        // Approval bound to another subject state.
        let moved = fingerprint('b');
        let report = human_report(Some(&valid), Some(&moved), Some("low"), vec![], vec![]);
        assert_eq!(report.decision, AdmissionDecision::Refuse);
    }

    #[test]
    fn verdict_vocabulary_round_trips_fail_closed() {
        for (verdict, name) in [
            (ClaimVerdict::Verified, "VERIFIED"),
            (ClaimVerdict::Refuted, "REFUTED"),
            (ClaimVerdict::Unverified, "UNVERIFIED"),
            (ClaimVerdict::Blocked, "BLOCKED"),
            (ClaimVerdict::Exempt, "EXEMPT"),
        ] {
            assert_eq!(verdict.as_str(), name);
            assert_eq!(ClaimVerdict::parse(name).unwrap(), verdict);
            assert_eq!(CriterionStatus::parse(name).unwrap().as_str(), name);
        }
        assert!(ClaimVerdict::parse("PASS").is_err());
        assert!(ClaimVerdict::parse("verified").is_err());
        assert!(CriterionStatus::parse("").is_err());
    }

    #[test]
    fn codecs_round_trip_and_fail_closed() {
        let refs = EvidenceRefs::new(
            vec!["receipt:r-1".to_owned(), "receipt:r-2:x".to_owned()],
            vec!["effect-9".to_owned()],
            vec![],
        )
        .unwrap();
        let decoded = EvidenceRefs::decode(
            &refs.encode_receipts(),
            &refs.encode_effects(),
            &refs.encode_sessions(),
        )
        .unwrap();
        assert_eq!(decoded, refs);
        assert!(EvidenceRefs::decode("6:short", "", "").is_err());
        assert!(EvidenceRefs::decode("x:y", "", "").is_err());

        let report = AdmissionReport {
            decision: AdmissionDecision::Refuse,
            outcomes: vec![
                CriterionOutcome {
                    criterion_id: "criterion:a".to_owned(),
                    status: CriterionStatus::Verified,
                    evidence_ref: Some("verification-1".to_owned()),
                    detail: "ok: fine".to_owned(),
                },
                CriterionOutcome {
                    criterion_id: "criterion-b".to_owned(),
                    status: CriterionStatus::Unverified,
                    evidence_ref: None,
                    detail: String::new(),
                },
            ],
        };
        let decoded = AdmissionReport::decode_outcomes(&report.encode_outcomes()).unwrap();
        assert_eq!(decoded, report.outcomes);
        assert!(AdmissionReport::decode_outcomes("3:ab").is_err());
    }

    #[test]
    fn requirement_revision_is_stable_and_shape_sensitive() {
        let first = candidate_requirement();
        let second = candidate_requirement();
        assert_eq!(requirement_revision(&first), requirement_revision(&second));
        assert!(requirement_revision(&first).starts_with("sha256:"));
        let plain = requirement(RequirementKind::TestSuite);
        assert_ne!(requirement_revision(&first), requirement_revision(&plain));
    }

    #[test]
    fn cv9_verified_requires_a_nonempty_resolved_effect_basis() {
        let req = requirement(RequirementKind::TestSuite);
        let fp = fingerprint('a');
        let agent_claim = claim("claim-1", "agent-1", &fp);
        let expected = requirement_revision(&req);
        let mut bare = verdict(
            "verification-1",
            &agent_claim,
            RequirementKind::TestSuite,
            &fp,
            ClaimVerdict::Verified,
        );
        bare.requirement_revision = expected.clone();
        bare.policy_fingerprint = None;
        // Empty basis: ineligible even though every other binding matches.
        let report = single(&req, Some(&fp), vec![&agent_claim], vec![&bare]);
        assert_eq!(report.decision, AdmissionDecision::Refuse);
        assert_eq!(report.outcomes[0].status, CriterionStatus::Unverified);

        // Nonempty but unresolvable basis: still ineligible.
        with_basis(&mut bare, "effect-missing");
        let report = single(&req, Some(&fp), vec![&agent_claim], vec![&bare]);
        assert_eq!(report.decision, AdmissionDecision::Refuse);

        // Resolved against a current-subject effect: admitted.
        let report = single_full(
            &req,
            Some(&fp),
            vec![&agent_claim],
            vec![&bare],
            None,
            vec![resolved("effect-1", &fp)],
            None,
        );
        assert_eq!(
            report.decision,
            AdmissionDecision::Refuse,
            "effect id mismatch must fail closed"
        );
        with_basis(&mut bare, "effect-1");
        let report = single_full(
            &req,
            Some(&fp),
            vec![&agent_claim],
            vec![&bare],
            None,
            vec![resolved("effect-1", &fp)],
            None,
        );
        assert_eq!(report.decision, AdmissionDecision::Admit);
        assert_eq!(report.outcomes[0].status, CriterionStatus::Verified);
    }

    #[test]
    fn cv9_verified_requires_matching_requirement_revision_and_policy() {
        let req = candidate_requirement();
        let fp = fingerprint('a');
        let agent_claim = claim("claim-1", "agent-1", &fp);
        let expected = requirement_revision(&req);
        let mut good = verdict(
            "verification-1",
            &agent_claim,
            RequirementKind::TestSuite,
            &fp,
            ClaimVerdict::Verified,
        );
        good.requirement_revision = expected.clone();
        good.policy_fingerprint = req.policy_fingerprint.clone();
        with_basis(&mut good, "effect-1");
        let accept = |verdict: &ClaimVerification| {
            single_full(
                &req,
                Some(&fp),
                vec![&agent_claim],
                vec![verdict],
                Some(integrity_for(&expected)),
                vec![resolved("effect-1", &fp)],
                Some(REQUIREMENT_FP),
            )
        };
        assert_eq!(accept(&good).decision, AdmissionDecision::Admit);

        let mut stale_revision = good.clone();
        stale_revision.requirement_revision =
            "sha256:ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff".to_owned();
        assert_eq!(accept(&stale_revision).decision, AdmissionDecision::Refuse);

        let mut stale_policy = good.clone();
        stale_policy.policy_fingerprint = Some("policy:other".to_owned());
        assert_eq!(accept(&stale_policy).decision, AdmissionDecision::Refuse);
    }

    #[test]
    fn cv9_candidate_requires_a_clean_matching_integrity_assessment() {
        let req = candidate_requirement();
        let fp = fingerprint('a');
        let agent_claim = claim("claim-1", "agent-1", &fp);
        let expected = requirement_revision(&req);
        let mut good = verdict(
            "verification-1",
            &agent_claim,
            RequirementKind::TestSuite,
            &fp,
            ClaimVerdict::Verified,
        );
        good.requirement_revision = expected.clone();
        good.policy_fingerprint = req.policy_fingerprint.clone();
        with_basis(&mut good, "effect-1");
        let resolved_effects = vec![resolved("effect-1", &fp)];
        let decide = |integrity: Option<IntegrityBasis>| {
            single_full(
                &req,
                Some(&fp),
                vec![&agent_claim],
                vec![&good],
                integrity,
                resolved_effects.clone(),
                Some(REQUIREMENT_FP),
            )
            .decision
        };
        // Missing assessment: CV-9 applies, so no admission.
        assert_eq!(decide(None), AdmissionDecision::Refuse);
        // Tampered assessment: UNVERIFIED, never VERIFIED.
        let mut tampered = integrity_for(&expected);
        tampered.clean = false;
        assert_eq!(decide(Some(tampered)), AdmissionDecision::Refuse);
        // Assessment evaluated another requirement revision.
        let other_revision = integrity_for(
            "sha256:ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
        );
        assert_eq!(decide(Some(other_revision)), AdmissionDecision::Refuse);
        // Assessment's claimed base differs from the execution's bound
        // base row: cross-row mismatch fails closed.
        let mut other_base = integrity_for(&expected);
        other_base.policy_fingerprint = format!("sha256:{}", "e".repeat(64));
        assert_eq!(decide(Some(other_base)), AdmissionDecision::Refuse);
        // Full match: admitted.
        assert_eq!(
            decide(Some(integrity_for(&expected))),
            AdmissionDecision::Admit
        );
    }
}
