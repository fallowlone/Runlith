//! Deterministic risk-signal baseline (TASK-0136).
//!
//! ADVISORY ONLY — NOT AUTHORITY. [`classify_risk_signals`] is a pure,
//! dependency-free heuristic over task/diff text. Its output may inform
//! review and escalation policy as metadata; it must never become a
//! verification authority, a gate verdict, or an admission input. No
//! caller in a trusted decision path may consume it.
//!
//! Deterministic: same input always yields the same signals in enum
//! order with evidence in (signal, keyword) order. Matching is ASCII
//! case-insensitive substring search over a fixed keyword table —
//! deliberately coarse; precision is explicitly out of scope for an
//! advisory baseline.

/// One advisory risk signal class.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RiskSignal {
    /// Authentication, secrets, permissions, vulnerabilities.
    Security,
    /// Storage, schema, migrations, queries.
    Database,
    /// Public contracts, endpoints, wire formats.
    PublicApi,
    /// Threads, async, locking, races.
    Concurrency,
    /// Sandboxing, confinement, capability boundaries.
    Isolation,
    /// Layering, ports/adapters, large-scale restructuring.
    Architecture,
}

impl RiskSignal {
    /// Every signal in canonical advisory order.
    pub const ALL: [Self; 6] = [
        Self::Security,
        Self::Database,
        Self::PublicApi,
        Self::Concurrency,
        Self::Isolation,
        Self::Architecture,
    ];

    /// Stable machine name for advisory records.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Security => "security",
            Self::Database => "database",
            Self::PublicApi => "public-api",
            Self::Concurrency => "concurrency",
            Self::Isolation => "isolation",
            Self::Architecture => "architecture",
        }
    }

    /// Fixed lowercase keyword table for the signal, in match order.
    fn keywords(self) -> &'static [&'static str] {
        match self {
            Self::Security => &[
                "auth",
                "password",
                "secret",
                "token",
                "credential",
                "permission",
                "privilege",
                "vulnerab",
                "exploit",
                "crypto",
                "keychain",
                "signature",
            ],
            Self::Database => &[
                "database",
                "sql",
                "sqlite",
                "migration",
                "schema",
                "query",
                "transaction",
                "index",
            ],
            Self::PublicApi => &[
                "public api",
                "pub fn",
                "endpoint",
                "rest",
                "graphql",
                "proto",
                "ipc",
                "contract",
                "breaking change",
            ],
            Self::Concurrency => &[
                "concurren",
                "parallel",
                "thread",
                "async",
                "await",
                "mutex",
                "deadlock",
                "atomic",
            ],
            Self::Isolation => &[
                "isolat",
                "sandbox",
                "seatbelt",
                "container",
                "namespace",
                "seccomp",
                "capabilit",
            ],
            Self::Architecture => &[
                "architect",
                "adr",
                "refactor",
                "redesign",
                "layer",
                "boundary",
                "coupling",
                "crate",
            ],
        }
    }
}

/// Advisory classification result: present signals plus the matched
/// keywords behind each. Metadata only — never authority.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RiskAdvisory {
    /// Present signals in [`RiskSignal::ALL`] order.
    pub signals: Vec<RiskSignal>,
    /// `(signal, matched keyword)` evidence in scan order.
    pub evidence: Vec<(RiskSignal, &'static str)>,
}

/// Classifies `text` into advisory risk signals. Pure and total:
/// empty input yields an empty advisory, and output depends only on
/// the input bytes.
#[must_use]
pub fn classify_risk_signals(text: &str) -> RiskAdvisory {
    let lowered = text.to_ascii_lowercase();
    let mut signals = Vec::new();
    let mut evidence = Vec::new();
    for signal in RiskSignal::ALL {
        let mut present = false;
        for keyword in signal.keywords() {
            if lowered.contains(keyword) {
                evidence.push((signal, *keyword));
                present = true;
            }
        }
        if present {
            signals.push(signal);
        }
    }
    RiskAdvisory { signals, evidence }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_text_has_no_signals() {
        let advisory = classify_risk_signals("");
        assert!(advisory.signals.is_empty());
        assert!(advisory.evidence.is_empty());
    }

    #[test]
    fn security_sample() {
        let advisory = classify_risk_signals("rotate the auth token and password");
        assert_eq!(advisory.signals, vec![RiskSignal::Security]);
    }

    #[test]
    fn database_sample() {
        let advisory = classify_risk_signals("sqlite migration adds an index");
        assert_eq!(advisory.signals, vec![RiskSignal::Database]);
    }

    #[test]
    fn public_api_sample() {
        let advisory = classify_risk_signals("breaking change to the public api endpoint");
        assert_eq!(advisory.signals, vec![RiskSignal::PublicApi]);
    }

    #[test]
    fn concurrency_sample() {
        let advisory = classify_risk_signals("race in the async lock path");
        assert_eq!(advisory.signals, vec![RiskSignal::Concurrency]);
    }

    #[test]
    fn isolation_sample() {
        let advisory = classify_risk_signals("seatbelt sandbox profile update");
        assert_eq!(advisory.signals, vec![RiskSignal::Isolation]);
    }

    #[test]
    fn architecture_sample() {
        let advisory = classify_risk_signals("refactor the crate boundary");
        assert_eq!(advisory.signals, vec![RiskSignal::Architecture]);
    }

    #[test]
    fn multi_signal_text_lists_signals_in_enum_order() {
        let advisory = classify_risk_signals("auth migration under seatbelt");
        assert_eq!(
            advisory.signals,
            vec![
                RiskSignal::Security,
                RiskSignal::Database,
                RiskSignal::Isolation,
            ]
        );
    }

    #[test]
    fn matching_is_case_insensitive() {
        let advisory = classify_risk_signals("AUTH TOKEN");
        assert_eq!(advisory.signals, vec![RiskSignal::Security]);
    }

    #[test]
    fn classification_is_deterministic() {
        let text = "auth migration under seatbelt with an async race";
        let first = classify_risk_signals(text);
        let second = classify_risk_signals(text);
        assert_eq!(first.signals, second.signals);
        assert_eq!(first.evidence, second.evidence);
        assert!(!first.evidence.is_empty());
    }

    #[test]
    fn signal_names_are_stable() {
        assert_eq!(
            RiskSignal::ALL
                .iter()
                .map(|signal| signal.as_str())
                .collect::<Vec<_>>(),
            vec![
                "security",
                "database",
                "public-api",
                "concurrency",
                "isolation",
                "architecture",
            ]
        );
    }
}
