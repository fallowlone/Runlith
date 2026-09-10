//! Canonical FakeAgent scenario identities (fake-agent.md §3).
//!
//! Adapter/test-owned simulation selectors — exactly these eleven, no
//! renames, no additions. They are never durable vocabulary: they must not
//! appear in `AttemptOutcome`, `EventType`, Task lifecycle, or SQLite.

/// Deterministic simulation selector for one FakeAgent execution.
///
/// The same scenario + request + configuration always yields equivalent
/// observable behavior. No random selection, no wall-clock races, no
/// network, no machine-specific behavior.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FakeScenario {
    /// Valid successful execution evidence (never Task completion).
    Success,
    /// Provider/runtime termination before a valid result.
    Crash,
    /// No terminal result until test-controlled release or cancellation.
    Hang,
    /// Bounded timeout result (promptly returned, never blocking).
    Timeout,
    /// Deliberately invalid provider output (never trusted success).
    MalformedResult,
    /// Simulated filesystem capability violation (no write performed).
    ForbiddenPathWrite,
    /// Simulated forbidden command attempt (no shell launched).
    ForbiddenCommand,
    /// Simulated provider quota/capacity exhaustion (no retry).
    QuotaReached,
    /// Simulated provider/network transport failure (no network used).
    NetworkFailure,
    /// Structurally valid but explicitly incomplete result.
    PartialResult,
    /// Valid work carrying a rejectable-evidence fixture for TASK-0037
    /// evaluation. FakeAgent never reviews; it only marks the fixture.
    ReviewRejection,
}

impl FakeScenario {
    /// Canonical snake_case identity (fake-agent.md §3 spelling).
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Success => "success",
            Self::Crash => "crash",
            Self::Hang => "hang",
            Self::Timeout => "timeout",
            Self::MalformedResult => "malformed_result",
            Self::ForbiddenPathWrite => "forbidden_path_write",
            Self::ForbiddenCommand => "forbidden_command",
            Self::QuotaReached => "quota_reached",
            Self::NetworkFailure => "network_failure",
            Self::PartialResult => "partial_result",
            Self::ReviewRejection => "review_rejection",
        }
    }

    /// Parses one canonical scenario name; `None` for anything else
    /// (including durable `AttemptOutcome` names, which are not scenarios).
    #[must_use]
    pub fn parse(raw: &str) -> Option<Self> {
        Some(match raw {
            "success" => Self::Success,
            "crash" => Self::Crash,
            "hang" => Self::Hang,
            "timeout" => Self::Timeout,
            "malformed_result" => Self::MalformedResult,
            "forbidden_path_write" => Self::ForbiddenPathWrite,
            "forbidden_command" => Self::ForbiddenCommand,
            "quota_reached" => Self::QuotaReached,
            "network_failure" => Self::NetworkFailure,
            "partial_result" => Self::PartialResult,
            "review_rejection" => Self::ReviewRejection,
            _ => return None,
        })
    }

    /// All eleven canonical scenarios in contract order.
    #[must_use]
    pub fn all() -> [Self; 11] {
        [
            Self::Success,
            Self::Crash,
            Self::Hang,
            Self::Timeout,
            Self::MalformedResult,
            Self::ForbiddenPathWrite,
            Self::ForbiddenCommand,
            Self::QuotaReached,
            Self::NetworkFailure,
            Self::PartialResult,
            Self::ReviewRejection,
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exactly_eleven_scenarios_with_fixed_names() {
        let all = FakeScenario::all();
        assert_eq!(all.len(), 11);
        let names: Vec<&str> = all.iter().map(|scenario| scenario.as_str()).collect();
        assert_eq!(
            names,
            [
                "success",
                "crash",
                "hang",
                "timeout",
                "malformed_result",
                "forbidden_path_write",
                "forbidden_command",
                "quota_reached",
                "network_failure",
                "partial_result",
                "review_rejection",
            ]
        );
        for scenario in all {
            assert_eq!(FakeScenario::parse(scenario.as_str()), Some(scenario));
        }
    }

    #[test]
    fn durable_outcome_names_are_not_scenarios() {
        for name in ["succeeded", "failed", "timed_out", "cancelled", ""] {
            assert_eq!(FakeScenario::parse(name), None, "must reject {name:?}");
        }
    }
}
