//! Adapter-scoped execution result and error model.
//!
//! [`ExecutionOutput`] is the smallest operational evidence a successful
//! provider execution returns: a bounded summary, a partial flag, and a
//! deterministic evidence reference. No chain-of-thought, no transcript,
//! no secrets, no unrestricted dumps.
//!
//! [`FakeAgentError`] is provider-neutral failure evidence reusing the
//! accepted vocabulary (providers.md §§9–10: quota exhaustion, network
//! failure, provider timeout, malformed output, process crash, capability
//! denial). It is deliberately distinct from both [`crate::FakeScenario`]
//! (simulation selector) and `AttemptOutcome` (durable conclusion owned by
//! future orchestration): three layers, never merged.

/// Maximum bounded summary length: operational evidence, never a dump.
pub const MAX_SUMMARY_LEN: usize = 1024;

/// Maximum evidence-reference length.
pub const MAX_EVIDENCE_REF_LEN: usize = 256;

/// Successful provider execution evidence.
///
/// `partial == false` means a valid complete result; `partial == true`
/// means structurally valid but explicitly incomplete — it must never
/// silently become full success. `rejectable_fixture == true` (only on
/// `review_rejection`) marks deterministic evidence the TASK-0037
/// evaluation fixture is expected to reject; it grants no review
/// authority and invokes no verification transition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionOutput {
    summary: String,
    partial: bool,
    evidence_ref: String,
    rejectable_fixture: bool,
}

impl ExecutionOutput {
    /// Builds validated output. Rejects blank summaries, over-long fields,
    /// and inconsistent flags (`rejectable_fixture` requires a complete
    /// result: a partial result is already non-success by construction).
    pub fn new(
        summary: impl Into<String>,
        partial: bool,
        evidence_ref: impl Into<String>,
        rejectable_fixture: bool,
    ) -> Result<Self, String> {
        let summary = summary.into();
        let evidence_ref = evidence_ref.into();
        if summary.trim().is_empty() {
            return Err("execution summary must not be blank".to_owned());
        }
        if summary.len() > MAX_SUMMARY_LEN {
            return Err(format!("execution summary exceeds {MAX_SUMMARY_LEN} bytes"));
        }
        if evidence_ref.trim().is_empty() {
            return Err("evidence ref must not be blank".to_owned());
        }
        if evidence_ref.len() > MAX_EVIDENCE_REF_LEN {
            return Err(format!("evidence ref exceeds {MAX_EVIDENCE_REF_LEN} bytes"));
        }
        if partial && rejectable_fixture {
            return Err("partial results cannot carry the review-rejection fixture".to_owned());
        }
        Ok(Self {
            summary: summary.trim().to_owned(),
            partial,
            evidence_ref: evidence_ref.trim().to_owned(),
            rejectable_fixture,
        })
    }

    /// Bounded operational summary (never reasoning, never secrets).
    #[must_use]
    pub fn summary(&self) -> &str {
        &self.summary
    }

    /// Whether this result is explicitly incomplete.
    #[must_use]
    pub fn is_partial(&self) -> bool {
        self.partial
    }

    /// Deterministic evidence reference minted by the adapter.
    #[must_use]
    pub fn evidence_ref(&self) -> &str {
        &self.evidence_ref
    }

    /// Whether the TASK-0037 evaluation fixture should reject this
    /// otherwise-valid work. Fixture marker only — not a verdict.
    #[must_use]
    pub fn is_rejectable_fixture(&self) -> bool {
        self.rejectable_fixture
    }
}

/// Provider-neutral execution failure evidence (adapter-scoped).
///
/// Variants reuse accepted provider/runtime failure concepts; scenario
/// names never appear here. Future orchestration interprets these into
/// canonical `AttemptOutcome`; this crate performs no such mapping with
/// authority.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FakeAgentError {
    /// Simulated provider/runtime termination before a valid result.
    Terminated { reason: String },
    /// Simulated bounded timeout (returned promptly, never blocking).
    TimedOut { budget: String },
    /// Execution cancelled through the canonical cancellation path.
    Cancelled,
    /// Deliberately invalid provider output; must never become trusted
    /// successful evidence.
    MalformedOutput { detail: String },
    /// Simulated capability violation attempt (never performed).
    /// `capability` distinguishes `fs.write` from `exec` denials.
    CapabilityDenied {
        capability: String,
        attempted: String,
    },
    /// Simulated quota/capacity exhaustion. No retry is performed.
    QuotaExhausted { detail: String },
    /// Simulated transport failure. No network is touched.
    NetworkFailure { detail: String },
}

impl FakeAgentError {
    /// Stable provider-neutral diagnostic class (never a scenario name).
    #[must_use]
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Terminated { .. } => "terminated",
            Self::TimedOut { .. } => "timed_out",
            Self::Cancelled => "cancelled",
            Self::MalformedOutput { .. } => "malformed_output",
            Self::CapabilityDenied { .. } => "capability_denied",
            Self::QuotaExhausted { .. } => "quota_exhausted",
            Self::NetworkFailure { .. } => "network_failure",
        }
    }
}

impl std::fmt::Display for FakeAgentError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Terminated { reason } => write!(f, "provider terminated: {reason}"),
            Self::TimedOut { budget } => write!(f, "provider timed out: {budget}"),
            Self::Cancelled => write!(f, "execution cancelled"),
            Self::MalformedOutput { detail } => write!(f, "malformed provider output: {detail}"),
            Self::CapabilityDenied {
                capability,
                attempted,
            } => write!(
                f,
                "capability denied ({capability}): {attempted} (not performed)"
            ),
            Self::QuotaExhausted { detail } => write!(f, "provider quota exhausted: {detail}"),
            Self::NetworkFailure { detail } => write!(f, "provider network failure: {detail}"),
        }
    }
}

impl std::error::Error for FakeAgentError {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::FakeScenario;

    fn output(partial: bool, rejectable: bool) -> ExecutionOutput {
        ExecutionOutput::new("did the work", partial, "fake-evidence:E-1:A-1", rejectable).unwrap()
    }

    #[test]
    fn complete_and_partial_outputs_are_distinct_types_not_prose() {
        let complete = output(false, false);
        let partial = output(true, false);
        assert!(!complete.is_partial());
        assert!(partial.is_partial());
        assert_ne!(complete, partial);
    }

    #[test]
    fn review_fixture_requires_a_complete_result() {
        assert!(output(false, true).is_rejectable_fixture());
        assert!(ExecutionOutput::new("x", true, "ref", true).is_err());
    }

    #[test]
    fn output_validation_rejects_blank_and_overlong() {
        assert!(ExecutionOutput::new("  ", false, "ref", false).is_err());
        assert!(ExecutionOutput::new("x", false, "  ", false).is_err());
        assert!(
            ExecutionOutput::new("x".repeat(MAX_SUMMARY_LEN + 1), false, "ref", false).is_err()
        );
        assert!(
            ExecutionOutput::new("x", false, "r".repeat(MAX_EVIDENCE_REF_LEN + 1), false).is_err()
        );
    }

    #[test]
    fn error_taxonomy_is_adapter_scoped_not_scenario_identity() {
        // Pins the adapter-scoped diagnostic classes. Layering is by
        // Rust type (`FakeScenario` selector vs `FakeAgentError`
        // evidence vs durable `AttemptOutcome`), not by spelling: six
        // of seven kinds are not even parseable as scenario names, and
        // the seventh (`network_failure`, reused from the accepted
        // providers.md failure vocabulary) still requires the adapter
        // to have run — matching on it never reinterprets a selector.
        let errors = [
            FakeAgentError::Terminated {
                reason: "gone".into(),
            },
            FakeAgentError::TimedOut {
                budget: "virtual".into(),
            },
            FakeAgentError::Cancelled,
            FakeAgentError::MalformedOutput {
                detail: "bad".into(),
            },
            FakeAgentError::CapabilityDenied {
                capability: "fs.write".into(),
                attempted: "/x".into(),
            },
            FakeAgentError::QuotaExhausted {
                detail: "empty".into(),
            },
            FakeAgentError::NetworkFailure {
                detail: "down".into(),
            },
        ];
        let kinds: Vec<&str> = errors.iter().map(|error| error.kind()).collect();
        assert_eq!(
            kinds,
            [
                "terminated",
                "timed_out",
                "cancelled",
                "malformed_output",
                "capability_denied",
                "quota_exhausted",
                "network_failure",
            ]
        );
        for scenario in ["crash", "hang", "timeout", "quota_reached"] {
            assert!(!kinds.contains(&scenario), "selector leaked: {scenario}");
        }
        for kind in [
            "terminated",
            "timed_out",
            "cancelled",
            "malformed_output",
            "capability_denied",
            "quota_exhausted",
        ] {
            assert!(
                FakeScenario::parse(kind).is_none(),
                "error kind must not double as scenario identity: {kind}"
            );
        }
    }
}
