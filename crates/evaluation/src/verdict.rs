//! Independent evaluation verdict. Exactly two G3 verdicts — no score,
//! no ranking, no provider-specific approval type.

/// Trusted verdict of the independent evaluation authority.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EvaluationVerdict {
    /// The submitted evidence satisfies the acceptance contract. Only this
    /// verdict, applied through the trusted verification-success operation,
    /// may complete the Task.
    Verified,
    /// The submitted evidence does not satisfy the acceptance contract. The
    /// trusted rejection operation routes the Task to its canonical retry
    /// state; the successful Attempt history is preserved.
    Rejected,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exactly_two_verdicts() {
        assert_ne!(EvaluationVerdict::Verified, EvaluationVerdict::Rejected);
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<EvaluationVerdict>();
    }
}
