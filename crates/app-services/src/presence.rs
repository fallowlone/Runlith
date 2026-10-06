//! Trusted control-surface presence primitives (TASK-0145).
//!
//! A [`FreshPresenceProof`] binds a fresh interactive owner-presence
//! confirmation to one exact operation plus its input digest. Only a
//! [`PresenceAuthority`] mints proofs; production denies until the
//! trusted Runlith desktop/control host exists (fail closed, no
//! unsandboxed fallback). Freshness windows and nonce single-use are
//! enforced by the store/service layers at consumption time.

/// Minting failures. Production denial is terminal for the operation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PresenceError {
    ControlHostUnavailable,
    MalformedProof(String),
}

impl std::fmt::Display for PresenceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ControlHostUnavailable => {
                write!(f, "trusted control host is unavailable")
            }
            Self::MalformedProof(detail) => write!(f, "malformed presence proof: {detail}"),
        }
    }
}

impl std::error::Error for PresenceError {}

/// Sole minter boundary for presence proofs. The trusted Runlith
/// desktop/control host implements this in a product-owned process;
/// until it exists, production holds [`DenyPresence`].
pub trait PresenceAuthority {
    fn prove(
        &self,
        operation: &str,
        input_digest: &str,
    ) -> Result<FreshPresenceProof, PresenceError>;
}

/// Production authority until the trusted control host lands: every
/// mint refuses. There is no unsandboxed fallback.
#[derive(Clone, Copy, Debug, Default)]
pub struct DenyPresence;

impl PresenceAuthority for DenyPresence {
    fn prove(
        &self,
        _operation: &str,
        _input_digest: &str,
    ) -> Result<FreshPresenceProof, PresenceError> {
        Err(PresenceError::ControlHostUnavailable)
    }
}

/// Test minter with a fixed clock and unique nonces. No production
/// caller may use it.
#[derive(Debug)]
pub struct TestPresenceAuthority {
    now_ms: u64,
    next_nonce: std::sync::atomic::AtomicU64,
}

impl TestPresenceAuthority {
    pub fn new(now_ms: u64) -> Self {
        static NEXT_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        Self {
            now_ms,
            next_nonce: std::sync::atomic::AtomicU64::new(
                NEXT_ID.fetch_add(1_000_000, std::sync::atomic::Ordering::Relaxed),
            ),
        }
    }
}

impl PresenceAuthority for TestPresenceAuthority {
    fn prove(
        &self,
        operation: &str,
        input_digest: &str,
    ) -> Result<FreshPresenceProof, PresenceError> {
        let nonce = format!(
            "test-nonce-{}",
            self.next_nonce
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        );
        FreshPresenceProof::mint(operation, input_digest, &nonce, self.now_ms)
    }
}

/// B1-grade fresh interactive user-presence authority (ADR-0033/0036 D1).
///
/// Opaque: only a [`PresenceAuthority`] mints it. `for_test` exists so
/// the boundary stays testable; no production caller may use it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FreshPresenceProof {
    operation: String,
    input_digest: String,
    nonce: String,
    proven_at_ms: u64,
}

impl FreshPresenceProof {
    fn mint(
        operation: &str,
        input_digest: &str,
        nonce: &str,
        proven_at_ms: u64,
    ) -> Result<Self, PresenceError> {
        if !valid_operation(operation) {
            return Err(PresenceError::MalformedProof(
                "operation must be an exact bounded token".into(),
            ));
        }
        if !valid_sha256(input_digest) {
            return Err(PresenceError::MalformedProof(
                "input digest must be sha256".into(),
            ));
        }
        if !valid_nonce(nonce) {
            return Err(PresenceError::MalformedProof("nonce is invalid".into()));
        }
        Ok(Self {
            operation: operation.to_owned(),
            input_digest: input_digest.to_owned(),
            nonce: nonce.to_owned(),
            proven_at_ms,
        })
    }

    /// Test-only proof with a unique nonce and fixed test operation.
    /// Production minting requires the trusted control surface; this
    /// constructor must stay uncalled outside tests.
    pub fn for_test() -> Self {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        let nonce = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        Self {
            operation: "test-operation".to_owned(),
            input_digest: format!("sha256:{}", "0".repeat(64)),
            nonce: format!("test-nonce-{nonce}"),
            proven_at_ms: 1_700_000_000_000,
        }
    }

    /// Test-only proof for an exact operation. Production minting
    /// requires the trusted control surface; this constructor must stay
    /// uncalled outside tests.
    pub fn for_test_operation(operation: &str, input_digest: &str, now_ms: u64) -> Self {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        let nonce = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        Self::mint(
            operation,
            input_digest,
            &format!("test-nonce-{nonce}"),
            now_ms,
        )
        .expect("test operation proof is well-formed")
    }

    pub fn operation(&self) -> &str {
        &self.operation
    }

    pub fn input_digest(&self) -> &str {
        &self.input_digest
    }

    pub fn nonce(&self) -> &str {
        &self.nonce
    }

    pub fn proven_at_ms(&self) -> u64 {
        self.proven_at_ms
    }
}

fn valid_operation(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 1024
        && !value.chars().any(|c| c.is_control() || c.is_whitespace())
}

fn valid_nonce(value: &str) -> bool {
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
    fn mint_binds_operation_digest_and_unique_nonce() {
        let authority = TestPresenceAuthority::new(1_700_000_000_000);
        let digest = format!("sha256:{}", "a".repeat(64));
        let first = authority
            .prove("review-setting:rev=4:path=b1", &digest)
            .unwrap();
        let second = authority
            .prove("review-setting:rev=4:path=b1", &digest)
            .unwrap();
        assert_eq!(first.operation(), "review-setting:rev=4:path=b1");
        assert_eq!(first.input_digest(), digest);
        assert_ne!(first.nonce(), second.nonce());
        assert_eq!(first.proven_at_ms(), 1_700_000_000_000);
    }

    #[test]
    fn malformed_operation_or_digest_refuses_mint() {
        let authority = TestPresenceAuthority::new(1_700_000_000_000);
        let digest = format!("sha256:{}", "a".repeat(64));
        assert!(authority.prove("", &digest).is_err());
        assert!(authority.prove("op", "bogus").is_err());
        assert!(authority.prove("op with spaces", &digest).is_err());
    }

    #[test]
    fn production_authority_denies_until_control_host_exists() {
        let digest = format!("sha256:{}", "a".repeat(64));
        assert!(matches!(
            DenyPresence.prove("review-setting:rev=4:path=b1", &digest),
            Err(PresenceError::ControlHostUnavailable)
        ));
    }

    #[test]
    fn test_proof_carries_unique_nonce() {
        let first = FreshPresenceProof::for_test();
        let second = FreshPresenceProof::for_test();
        assert_ne!(first.nonce(), second.nonce());
    }
}
