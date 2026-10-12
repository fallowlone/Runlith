//! Policy-owned closed selector catalog (ADR-0036 §1a, IMPL-A).
//!
//! The catalog lists the accepted candidate-input selector ids together
//! with the path-normalization/confinement rules and literal bounds each
//! selector accepts. v1 contains exactly `candidate.file-contains.v1`
//! (ADR-0034 §2, ADR-0036 §5/Option 5a). Adding a selector id is a
//! reviewed catalog-revision event, never a runtime parameter.
//!
//! Shape mirrors [`crate::TestMutationPolicy`]: closed, versioned,
//! fingerprinted, with canonical serialization. The catalog validates
//! proposed values; it never reads provider, repository, or planner
//! state — untrusted inputs cannot widen it.

use sha2::{Digest, Sha256};

/// The only v1 catalogued selector (ADR-0034 §2).
pub const SELECTOR_FILE_CONTAINS_V1: &str = "candidate.file-contains.v1";

/// Canonical catalog encoding version.
const CATALOG_ENCODING_V1: &str = "candidate-selector-catalog/v1";

/// ADR-0035 path bound reused by ADR-0036 §1a. Note: bound values also
/// pass through `RequirementParam` (256-byte durable cap), so values in
/// (256, 4096] fail closed downstream at the binding boundary.
pub const MAX_CANDIDATE_PATH_LEN: usize = 4096;
/// ADR-0036 §1a literal bound (same durable-cap note as above).
pub const MAX_CANDIDATE_LITERAL_LEN: usize = 4096;

/// Closed v1 catalog of candidate-input selectors.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CandidateSelectorCatalog(CatalogKind);

#[derive(Clone, Debug, Eq, PartialEq)]
enum CatalogKind {
    V1,
}

impl CandidateSelectorCatalog {
    /// The v1 catalog: exactly [`SELECTOR_FILE_CONTAINS_V1`].
    pub fn v1() -> Self {
        Self(CatalogKind::V1)
    }

    /// Accepted selector ids, canonical order.
    pub fn selectors(&self) -> Vec<&'static str> {
        vec![SELECTOR_FILE_CONTAINS_V1]
    }

    /// Validates a proposed binding against the catalog: selector
    /// membership, ADR-0035-style hostile-path rules, and literal bounds
    /// including the SECRET-material screen (INV-008/INV-151: params
    /// persist in SQLite, so credential material must never bind).
    ///
    /// Error messages never echo the proposed literal.
    pub fn validate_binding(
        &self,
        selector: &str,
        path: &str,
        literal: &str,
    ) -> Result<BoundCandidateParams, SelectorCatalogError> {
        if selector != SELECTOR_FILE_CONTAINS_V1 {
            // F4: never echo the proposed (attacker-controlled) value;
            // membership is already public via selectors().
            return Err(SelectorCatalogError(format!(
                "selector is not in the closed v1 catalog (accepts: {SELECTOR_FILE_CONTAINS_V1})"
            )));
        }
        validate_candidate_path(path)?;
        validate_candidate_literal(literal)?;
        Ok(BoundCandidateParams {
            selector: selector.to_owned(),
            path: path.to_owned(),
            literal: literal.to_owned(),
        })
    }

    /// Canonical, versioned representation owned by Policy Engine.
    pub fn serialize(&self) -> String {
        format!("{CATALOG_ENCODING_V1}\ns:{SELECTOR_FILE_CONTAINS_V1}\n")
    }

    pub fn deserialize(serialized: &str) -> Result<Self, SelectorCatalogError> {
        let catalog = Self::v1();
        if serialized != catalog.serialize() {
            return Err(SelectorCatalogError(
                "unknown selector catalog encoding".into(),
            ));
        }
        Ok(catalog)
    }

    pub fn fingerprint(&self) -> String {
        let mut hash = Sha256::new();
        hash.update(b"candidate-selector-catalog\0");
        hash.update(self.serialize().as_bytes());
        format!("sha256:{:x}", hash.finalize())
    }
}

/// Catalog-validated binding values (membership, path, and literal rules
/// already checked).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BoundCandidateParams {
    selector: String,
    path: String,
    literal: String,
}

impl BoundCandidateParams {
    pub fn selector(&self) -> &str {
        &self.selector
    }
    pub fn path(&self) -> &str {
        &self.path
    }
    pub fn literal(&self) -> &str {
        &self.literal
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SelectorCatalogError(pub String);

impl std::fmt::Display for SelectorCatalogError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for SelectorCatalogError {}

impl From<SelectorCatalogError> for crate::PolicyContractError {
    fn from(error: SelectorCatalogError) -> Self {
        Self::InvalidSelectorCatalog(error.0)
    }
}

/// Immutable catalog revision with anchored fingerprint.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CandidateSelectorCatalogRevision {
    revision: u64,
    catalog: CandidateSelectorCatalog,
    fingerprint: String,
}

impl CandidateSelectorCatalogRevision {
    pub fn new(
        revision: u64,
        catalog: CandidateSelectorCatalog,
    ) -> Result<Self, SelectorCatalogError> {
        if revision == 0 {
            return Err(SelectorCatalogError(
                "catalog revision must be positive".into(),
            ));
        }
        let fingerprint = catalog.fingerprint();
        Ok(Self {
            revision,
            catalog,
            fingerprint,
        })
    }

    pub fn restore(
        revision: u64,
        serialized: &str,
        fingerprint: &str,
    ) -> Result<Self, SelectorCatalogError> {
        let catalog = CandidateSelectorCatalog::deserialize(serialized)?;
        let value = Self::new(revision, catalog)?;
        if value.fingerprint != fingerprint {
            return Err(SelectorCatalogError(
                "stored catalog fingerprint mismatch".into(),
            ));
        }
        Ok(value)
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }
    pub fn catalog(&self) -> &CandidateSelectorCatalog {
        &self.catalog
    }
    pub fn fingerprint(&self) -> &str {
        &self.fingerprint
    }
    pub fn serialize(&self) -> String {
        self.catalog.serialize()
    }
}

/// ADR-0035-style hostile-path validation (ADR-0036 §1a): relative,
/// ASCII-only, no empty components, no `.`/`..`, no `.git`, no
/// backslash/separator injection, bounded length.
fn validate_candidate_path(path: &str) -> Result<(), SelectorCatalogError> {
    // F1: validated must equal persisted — RequirementParam::new trims
    // before persisting, so any surrounding whitespace fails closed here.
    if path.is_empty()
        || path != path.trim()
        || path.len() > MAX_CANDIDATE_PATH_LEN
        || !path.is_ascii()
    {
        return Err(SelectorCatalogError("unsupported candidate path".into()));
    }
    for component in path.as_bytes().split(|byte| *byte == b'/') {
        if component.is_empty()
            || component == b"."
            || component == b".."
            || component.eq_ignore_ascii_case(b".git")
            || component
                .iter()
                .any(|byte| *byte == b'\\' || *byte < 0x20 || *byte == 0x7f)
        {
            return Err(SelectorCatalogError("unsupported candidate path".into()));
        }
    }
    Ok(())
}

fn validate_candidate_literal(literal: &str) -> Result<(), SelectorCatalogError> {
    if literal.trim().is_empty() {
        return Err(SelectorCatalogError(
            "candidate literal must not be blank".into(),
        ));
    }
    if literal.len() > MAX_CANDIDATE_LITERAL_LEN {
        return Err(SelectorCatalogError(
            "candidate literal exceeds 4096 bytes".into(),
        ));
    }
    if literal.chars().any(char::is_control) {
        return Err(SelectorCatalogError(
            "candidate literal must not contain control characters".into(),
        ));
    }
    if looks_like_secret_material(literal) {
        return Err(SelectorCatalogError(
            "candidate literal must not carry credential material".into(),
        ));
    }
    Ok(())
}

/// Narrow structural screen derived from `docs/security/DATA_CLASSIFICATION.md`
/// §SECRET (passwords, API keys, private tokens, private keys) and
/// INV-008/INV-151: bound params persist in SQLite plaintext, so
/// credential *forms* fail closed. It matches forms, not substrings — a
/// needle merely mentioning a sensitive word is not credential material.
fn looks_like_secret_material(literal: &str) -> bool {
    let normalized = literal.to_ascii_lowercase();
    if normalized.contains("-----begin") && normalized.contains("private key-----") {
        return true;
    }
    const ASSIGNMENT_KEYS: [&str; 7] = [
        "password", "passwd", "secret", "apikey", "api_key", "api-key", "token",
    ];
    ASSIGNMENT_KEYS.iter().any(|key| {
        normalized.match_indices(key).any(|(index, _)| {
            // F3: allow one optional quote between key and separator so
            // JSON-quoted assignments ("password": "...") also fail closed.
            let rest = normalized[index + key.len()..].trim_start();
            let rest = rest.strip_prefix(['\'', '"']).unwrap_or(rest).trim_start();
            rest.starts_with(['=', ':'])
        })
    })
}
