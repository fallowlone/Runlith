//! Provider-neutral usage evidence (TASK-0046, G5).
//!
//! When the CLI reports usage through the machine-readable protocol, the
//! adapter translates it into this provenance-bearing shape for future
//! consumers (Usage Ledger / Model Router are explicitly out of scope —
//! this type merely exposes the contract they will need). Values are
//! never fabricated: absent fields read as [`UsageProvenance::Unknown`],
//! established measurements as [`UsageProvenance::Reported`]. There is no
//! `Estimated` variant on purpose — this adapter never estimates.

/// Provenance of a usage observation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UsageProvenance {
    /// The provider reported this value through its structured protocol.
    Reported,
    /// The provider did not report a value; nothing was inferred.
    Unknown,
}

/// Provider-neutral usage observation for one execution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UsageSample {
    /// Uncached input tokens, when reported by the provider.
    pub input_tokens: Option<u64>,
    /// Input tokens written into the provider prompt cache, when reported.
    pub cache_creation_input_tokens: Option<u64>,
    /// Input tokens read from the provider prompt cache, when reported.
    pub cache_read_input_tokens: Option<u64>,
    /// Output tokens, when reported.
    pub output_tokens: Option<u64>,
    /// Where the numbers came from.
    pub provenance: UsageProvenance,
}

impl UsageSample {
    /// Unknown observation: the provider reported nothing usable.
    #[must_use]
    pub fn unknown() -> Self {
        Self {
            input_tokens: None,
            cache_creation_input_tokens: None,
            cache_read_input_tokens: None,
            output_tokens: None,
            provenance: UsageProvenance::Unknown,
        }
    }

    /// Provider-reported observation. Token counts travel through
    /// unchanged — never converted to cost, never rounded, never merged
    /// with estimates.
    #[must_use]
    pub fn reported(
        input_tokens: Option<u64>,
        cache_creation_input_tokens: Option<u64>,
        cache_read_input_tokens: Option<u64>,
        output_tokens: Option<u64>,
    ) -> Self {
        let provenance = if input_tokens.is_some()
            || cache_creation_input_tokens.is_some()
            || cache_read_input_tokens.is_some()
            || output_tokens.is_some()
        {
            UsageProvenance::Reported
        } else {
            UsageProvenance::Unknown
        };
        Self {
            input_tokens,
            cache_creation_input_tokens,
            cache_read_input_tokens,
            output_tokens,
            provenance,
        }
    }

    /// Total provider-reported input consumed by one call. Cache token
    /// classes are real billed/context input and therefore participate in
    /// G7's planner input envelope. Missing cache fields mean zero; a missing
    /// base input count or arithmetic overflow remains unknown/fail-closed.
    #[must_use]
    pub fn total_input_tokens(&self) -> Option<u64> {
        self.input_tokens?
            .checked_add(self.cache_creation_input_tokens.unwrap_or(0))?
            .checked_add(self.cache_read_input_tokens.unwrap_or(0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn absent_usage_is_unknown_never_zero() {
        // Zero is a measurement; absence must not become zero.
        let sample = UsageSample::reported(None, None, None, None);
        assert_eq!(sample.provenance, UsageProvenance::Unknown);
        assert_eq!(sample.input_tokens, None);
    }

    #[test]
    fn reported_values_keep_provenance() {
        let sample = UsageSample::reported(Some(10), None, None, Some(20));
        assert_eq!(sample.provenance, UsageProvenance::Reported);
        assert_eq!(
            (sample.input_tokens, sample.output_tokens),
            (Some(10), Some(20))
        );
        assert_eq!(sample.total_input_tokens(), Some(10));
    }

    #[test]
    fn total_input_includes_cache_creation_and_reads() {
        let sample = UsageSample::reported(Some(5), Some(40_000), Some(600_000), Some(20));
        assert_eq!(sample.total_input_tokens(), Some(640_005));
    }

    #[test]
    fn total_input_overflow_is_unknown() {
        let sample = UsageSample::reported(Some(u64::MAX), Some(1), None, Some(20));
        assert_eq!(sample.total_input_tokens(), None);
    }
}
