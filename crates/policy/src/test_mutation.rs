use sha2::{Digest, Sha256};

/// Closed v1 policy for changes to test and check inputs.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TestMutationPolicy(PolicyKind);

#[derive(Clone, Debug, Eq, PartialEq)]
enum PolicyKind {
    Forbidden,
    ScopedReviewed {
        paths: Vec<String>,
        checks: Vec<String>,
    },
}

#[allow(non_upper_case_globals)]
impl TestMutationPolicy {
    pub const Forbidden: Self = Self(PolicyKind::Forbidden);
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TestMutationPolicyError(pub String);

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TestMutationPolicyRevision {
    revision: u64,
    policy: TestMutationPolicy,
    fingerprint: String,
}

impl TestMutationPolicyRevision {
    pub fn new(revision: u64, policy: TestMutationPolicy) -> Result<Self, TestMutationPolicyError> {
        if revision == 0 {
            return Err(TestMutationPolicyError(
                "policy revision must be positive".into(),
            ));
        }
        let fingerprint = policy.fingerprint();
        Ok(Self {
            revision,
            policy,
            fingerprint,
        })
    }

    pub fn restore(
        revision: u64,
        serialized: &str,
        fingerprint: &str,
    ) -> Result<Self, TestMutationPolicyError> {
        let policy = TestMutationPolicy::deserialize(serialized)?;
        let value = Self::new(revision, policy)?;
        if value.fingerprint != fingerprint {
            return Err(TestMutationPolicyError(
                "stored policy fingerprint mismatch".into(),
            ));
        }
        Ok(value)
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }
    pub fn policy(&self) -> &TestMutationPolicy {
        &self.policy
    }
    pub fn fingerprint(&self) -> &str {
        &self.fingerprint
    }
    pub fn serialize(&self) -> String {
        self.policy.serialize()
    }
}

impl std::fmt::Display for TestMutationPolicyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for TestMutationPolicyError {}

impl From<TestMutationPolicyError> for crate::PolicyContractError {
    fn from(error: TestMutationPolicyError) -> Self {
        Self::InvalidTestMutationPolicy(error.0)
    }
}

impl TestMutationPolicy {
    pub fn scoped_reviewed(
        paths: Vec<String>,
        checks: Vec<String>,
    ) -> Result<Self, TestMutationPolicyError> {
        let paths = canonical_set(paths, "path", valid_path)?;
        let checks = canonical_set(checks, "check", valid_check)?;
        if paths.is_empty() || checks.is_empty() {
            return Err(TestMutationPolicyError(
                "ScopedReviewed requires at least one exact path and check".into(),
            ));
        }
        Ok(Self(PolicyKind::ScopedReviewed { paths, checks }))
    }

    pub fn allows(&self, path: &str, check: &str) -> bool {
        match &self.0 {
            PolicyKind::Forbidden => false,
            PolicyKind::ScopedReviewed { paths, checks } => {
                paths.iter().any(|allowed| allowed == path)
                    && checks.iter().any(|allowed| allowed == check)
            }
        }
    }

    /// Canonical, versioned representation owned by Policy Engine.
    pub fn serialize(&self) -> String {
        match &self.0 {
            PolicyKind::Forbidden => "forbidden/v1".into(),
            PolicyKind::ScopedReviewed { paths, checks } => {
                let mut out = String::from("scoped-reviewed/v1\n");
                for path in paths {
                    out.push_str("p:");
                    out.push_str(&hex(path.as_bytes()));
                    out.push('\n');
                }
                for check in checks {
                    out.push_str("c:");
                    out.push_str(&hex(check.as_bytes()));
                    out.push('\n');
                }
                out
            }
        }
    }

    pub fn deserialize(serialized: &str) -> Result<Self, TestMutationPolicyError> {
        if serialized == "forbidden/v1" {
            return Ok(Self::Forbidden);
        }
        let mut lines = serialized
            .strip_prefix("scoped-reviewed/v1\n")
            .ok_or_else(|| TestMutationPolicyError("unknown test mutation policy encoding".into()))?
            .lines();
        let mut paths = Vec::new();
        let mut checks = Vec::new();
        for line in &mut lines {
            let (kind, value) = line
                .split_once(':')
                .ok_or_else(|| TestMutationPolicyError("malformed policy field".into()))?;
            let value = String::from_utf8(unhex(value)?)
                .map_err(|_| TestMutationPolicyError("policy field is not UTF-8".into()))?;
            match kind {
                "p" => paths.push(value),
                "c" => checks.push(value),
                _ => return Err(TestMutationPolicyError("unknown policy field".into())),
            }
        }
        let policy = Self::scoped_reviewed(paths, checks)?;
        if policy.serialize() != serialized {
            return Err(TestMutationPolicyError(
                "non-canonical policy encoding".into(),
            ));
        }
        Ok(policy)
    }

    pub fn fingerprint(&self) -> String {
        let mut hash = Sha256::new();
        hash.update(b"test-mutation-policy\0");
        hash.update(self.serialize().as_bytes());
        format!("sha256:{:x}", hash.finalize())
    }
}

fn canonical_set(
    mut values: Vec<String>,
    field: &str,
    validate: fn(&str) -> bool,
) -> Result<Vec<String>, TestMutationPolicyError> {
    if values.len() > 128 {
        return Err(TestMutationPolicyError(format!(
            "too many exact {field} entries"
        )));
    }
    if values.iter().any(|value| !validate(value)) {
        return Err(TestMutationPolicyError(format!("invalid exact {field}")));
    }
    values.sort();
    if values.windows(2).any(|pair| pair[0] == pair[1]) {
        return Err(TestMutationPolicyError(format!("duplicate exact {field}")));
    }
    Ok(values)
}

fn valid_path(path: &str) -> bool {
    !path.is_empty()
        && path.len() <= 1024
        && !path.starts_with('/')
        && !path.contains('\\')
        && !path.chars().any(char::is_control)
        && path
            .split('/')
            .all(|part| !part.is_empty() && part != "." && part != "..")
}

fn valid_check(check: &str) -> bool {
    !check.trim().is_empty() && check.len() <= 256 && !check.chars().any(char::is_control)
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(DIGITS[(byte >> 4) as usize] as char);
        out.push(DIGITS[(byte & 0xf) as usize] as char);
    }
    out
}

fn unhex(value: &str) -> Result<Vec<u8>, TestMutationPolicyError> {
    if !value.len().is_multiple_of(2) {
        return Err(TestMutationPolicyError("malformed hex policy field".into()));
    }
    value
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            let digit = |b: u8| match b {
                b'0'..=b'9' => Some(b - b'0'),
                b'a'..=b'f' => Some(b - b'a' + 10),
                _ => None,
            };
            match (digit(pair[0]), digit(pair[1])) {
                (Some(high), Some(low)) => Ok((high << 4) | low),
                _ => Err(TestMutationPolicyError("malformed hex policy field".into())),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scoped_policy_is_exact_canonical_and_round_trips() {
        let policy = TestMutationPolicy::scoped_reviewed(
            vec!["tests/unit.rs".into(), "tests/api.rs".into()],
            vec!["cargo-test-unit".into()],
        )
        .unwrap();
        assert!(policy.allows("tests/unit.rs", "cargo-test-unit"));
        assert!(!policy.allows("tests/unit.rs/child", "cargo-test-unit"));
        assert!(!policy.allows("tests/api.rs", "other-check"));
        assert_eq!(
            TestMutationPolicy::deserialize(&policy.serialize()).unwrap(),
            policy
        );
        assert_eq!(policy.fingerprint(), policy.clone().fingerprint());
    }

    #[test]
    fn forbidden_default_and_invalid_scope_fail_closed() {
        assert!(!TestMutationPolicy::Forbidden.allows("tests/a.rs", "test"));
        assert!(
            TestMutationPolicy::scoped_reviewed(vec!["../x".into()], vec!["test".into()]).is_err()
        );
        assert!(TestMutationPolicy::scoped_reviewed(vec!["tests/a.rs".into()], vec![]).is_err());
        assert!(TestMutationPolicy::scoped_reviewed(
            vec!["tests/a.rs".into(), "tests/a.rs".into()],
            vec!["test".into()]
        )
        .is_err());
        assert!(
            TestMutationPolicy::deserialize("scoped-reviewed/v1\np:2e2e2f78\nc:74657374\n")
                .is_err()
        );
        assert!(TestMutationPolicyRevision::restore(1, "forbidden/v1", "sha256:stale").is_err());
    }
}
