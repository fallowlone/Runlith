//! Case-fold-safe managed workspace identity v2 (TASK-0053, G5 B-CF).
//!
//! TASK-0052 proved the TASK-0051 `%`-escaping derivation unsafe on
//! case-insensitive filesystems: it preserved ASCII case, so `E-CASE` and
//! `E-case` derived exact-distinct refs whose loose-ref files collide and
//! whose packed refs alias. This module replaces it with a versioned,
//! canonical-lowercase encoding.
//!
//! ```text
//! readable:  e2-<escaped>          (reversible, injective over exact bytes)
//! digest:    h2-<sha256-lowerhex>  (deterministic, collision-resistant)
//! ```
//!
//! Escape rule over the exact UTF-8 bytes (no Unicode normalization —
//! managed identity follows `ExecutionId` exact-equality semantics):
//! `[a-z0-9-_]` pass through; every other byte becomes `%` + two
//! **lowercase** hex digits (`A` → `%41`, `/` → `%2f`, `%` → `%25`).
//! The output alphabet is therefore `[a-z0-9%\-_]` plus the `e2-`/`h2-`
//! prefixes — already ASCII-casefold-stable
//! (`ascii_casefold(output) == output`), so case-insensitive filesystem
//! semantics cannot merge two outputs.
//!
//! Namespace properties that matter more than syntax:
//!
//! - readable outputs are exactly decodable (`decode_readable_slug` inverts
//!   `escape_body`), hence injective over the supported exact-byte domain;
//! - digest outputs are collision-resistant, NOT mathematically injective —
//!   a bounded hash of a larger domain cannot be injective (pigeonhole).
//!   Digest collisions fail closed through the durable reservation
//!   backstop (migration 0007 + atomic admission), never by silent reuse;
//! - readable and digest ranges are disjoint by prefix (`e2-` vs `h2-`), so
//!   a literal digest-shaped identity can never be mistaken for a digest;
//! - every output starts with a letter prefix, so no output is a Windows
//!   reserved device basename (`CON`, `PRN`, `AUX`, `NUL`, `COM1`–`COM9`,
//!   `LPT1`–`LPT9` require an exact match); the alphabet additionally
//!   excludes every Windows-invalid character and trailing space/dot;
//! - outputs hold no `/`, never form `.`/`..`, and satisfy `BranchRef`
//!   validation, so refs stay inside `refs/heads/agent/` by construction.
//!
//! Provider-neutral: no provider, model, or protocol data travels here.

use std::path::Path;

use agent_runtime::ExecutionId;
use workspace::{BranchRef, WorkspaceFailure, WorkspaceId};

/// Versioned prefix for the reversible readable form.
pub const MANAGED_READABLE_PREFIX: &str = "e2-";
/// Versioned prefix for the digest fallback form.
pub const MANAGED_DIGEST_PREFIX: &str = "h2-";

/// Budget for the escaped body of the readable form. Escaped bodies at or
/// below this length stay human-readable; longer ones use the digest form.
/// Total slug length stays bounded (readable ≤ 67, digest = 67).
const MAX_READABLE_ESCAPED: usize = 64;
const LEGACY_READABLE_BUDGET: usize = 64;

/// Lowercase hex digits — the single canonical rendering.
const HEX: &[u8; 16] = b"0123456789abcdef";

/// Whether a source byte passes through unescaped: canonical lowercase
/// ASCII only, so the output can never contain an uppercase character.
fn is_passthrough(byte: u8) -> bool {
    byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-' || byte == b'_'
}

/// Appends the canonical encoding of one source byte: passthrough or
/// `%` + two lowercase hex digits. Exhaustive over all 256 byte values.
fn escape_byte(byte: u8, out: &mut String) {
    if is_passthrough(byte) {
        out.push(byte as char);
    } else {
        out.push('%');
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
}

/// Canonical escape of the exact source bytes (no normalization).
fn escape_body(raw: &str) -> String {
    let mut escaped = String::with_capacity(raw.len());
    for byte in raw.bytes() {
        escape_byte(byte, &mut escaped);
    }
    escaped
}

/// SHA-256 rendered as 64 lowercase hex characters (digest fallback).
/// Small pure-Rust `sha2` dependency — no hand-rolled crypto.
fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::Digest as _;
    let digest = sha2::Sha256::digest(bytes);
    let mut out = String::with_capacity(64);
    for byte in digest {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

fn legacy_fnv1a_64(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf29ce484222325;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

/// Historical G4 derivation shipped before TASK-0051.
fn legacy_g4_slug(raw: &str) -> String {
    if raw.len() > LEGACY_READABLE_BUDGET {
        return format!("h{:016X}", legacy_fnv1a_64(raw.as_bytes()));
    }
    let mut out = String::with_capacity(raw.len());
    for byte in raw.bytes() {
        if byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_' {
            out.push(byte as char);
        } else {
            out.push('-');
            out.push_str(&format!("{byte:02X}"));
        }
    }
    out
}

/// Historical TASK-0051 derivation immediately preceding v2.
fn legacy_task0051_slug(raw: &str) -> String {
    let mut escaped = String::with_capacity(raw.len());
    for byte in raw.bytes() {
        if byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_' {
            escaped.push(byte as char);
        } else if byte == b'%' {
            escaped.push_str("%%");
        } else {
            escaped.push('%');
            escaped.push_str(&format!("{byte:02X}"));
        }
    }
    if escaped.len() <= LEGACY_READABLE_BUDGET {
        escaped
    } else {
        format!("h{}-", sha256_hex(raw.as_bytes()))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PersistedManagedIdentityFormat {
    CurrentV2,
    Legacy,
}

fn identity_pair_matches(
    root: &Path,
    branch: &BranchRef,
    worktree_ref: &str,
    branch_slug: String,
    workspace_slug: String,
) -> bool {
    let expected_branch = format!("refs/heads/agent/{branch_slug}");
    let expected_worktree = root.join(format!("ws-{workspace_slug}"));
    branch.as_str() == expected_branch
        && expected_worktree
            .to_str()
            .is_some_and(|path| path == worktree_ref)
}

/// Classifies a persisted lease from its complete durable identity pair.
/// Prefixes alone are insufficient because historical ExecutionIds could
/// themselves begin with `e2-`/`h2-`.
pub(crate) fn classify_persisted_identity(
    root: &Path,
    owner: &ExecutionId,
    workspace_id: &WorkspaceId,
    branch: &BranchRef,
    worktree_ref: &str,
) -> Result<PersistedManagedIdentityFormat, WorkspaceFailure> {
    if identity_pair_matches(
        root,
        branch,
        worktree_ref,
        managed_slug(owner.as_str()),
        managed_slug(workspace_id.as_str()),
    ) {
        return Ok(PersistedManagedIdentityFormat::CurrentV2);
    }

    let legacy_g4 = identity_pair_matches(
        root,
        branch,
        worktree_ref,
        legacy_g4_slug(owner.as_str()),
        legacy_g4_slug(workspace_id.as_str()),
    );
    let legacy_task0051 = identity_pair_matches(
        root,
        branch,
        worktree_ref,
        legacy_task0051_slug(owner.as_str()),
        legacy_task0051_slug(workspace_id.as_str()),
    );
    if legacy_g4 || legacy_task0051 {
        return Ok(PersistedManagedIdentityFormat::Legacy);
    }

    Err(WorkspaceFailure::OwnershipMismatch {
        detail: format!(
            "lease {} persisted branch/worktree pair matches neither current v2 nor a supported historical identity",
            workspace_id.as_str()
        ),
    })
}

/// Derives the managed slug for one validated identity with an injectable
/// digest function. Production uses [`managed_slug`] (SHA-256); tests force
/// a deterministic digest collision through this seam without faking any
/// cryptographic weakness in production.
pub(crate) fn managed_slug_with_digest(raw: &str, digest: impl Fn(&[u8]) -> String) -> String {
    let escaped = escape_body(raw);
    if escaped.len() <= MAX_READABLE_ESCAPED {
        format!("{MANAGED_READABLE_PREFIX}{escaped}")
    } else {
        format!("{MANAGED_DIGEST_PREFIX}{}", digest(raw.as_bytes()))
    }
}

/// Derives the managed slug for one validated identity (TASK-0053 v2).
pub fn managed_slug(raw: &str) -> String {
    managed_slug_with_digest(raw, sha256_hex)
}

/// Derives the managed branch for one execution
/// (`refs/heads/agent/<v2-slug>`). Deterministic, so reopen and
/// reconciliation derive the same branch without new input.
///
/// Readable outputs are reversible/injective over exact source bytes;
/// digest outputs are deterministic and collision-resistant with a
/// fail-closed durable backstop — NOT mathematically injective.
pub fn branch_for_execution(owner: &ExecutionId) -> Result<BranchRef, WorkspaceFailure> {
    BranchRef::new(format!("refs/heads/agent/{}", managed_slug(owner.as_str()))).map_err(|error| {
        WorkspaceFailure::StorageFailure {
            detail: format!("invalid workspace value: {error}"),
        }
    })
}

/// Test-seam branch derivation with an injectable digest (see
/// [`managed_slug_with_digest`]).
#[cfg(test)]
pub(crate) fn branch_for_execution_with_digest(
    owner: &ExecutionId,
    digest: impl Fn(&[u8]) -> String,
) -> Result<BranchRef, WorkspaceFailure> {
    BranchRef::new(format!(
        "refs/heads/agent/{}",
        managed_slug_with_digest(owner.as_str(), digest)
    ))
    .map_err(|error| WorkspaceFailure::StorageFailure {
        detail: format!("invalid workspace value: {error}"),
    })
}

/// Decodes a readable (`e2-`) slug back to its exact source bytes.
/// Returns `None` for digest slugs (not reversible) and for malformed
/// input (a `%` not followed by two hex digits, or a literal outside the
/// passthrough set). Decoding inverts [`escape_body`], which is what makes
/// the readable form injective.
pub fn decode_readable_slug(slug: &str) -> Option<Vec<u8>> {
    let body = slug.strip_prefix(MANAGED_READABLE_PREFIX)?;
    let bytes = body.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        let byte = bytes[index];
        if byte == b'%' {
            let high = bytes.get(index + 1)?;
            let low = bytes.get(index + 2)?;
            out.push((hex_value(*high)? << 4) | hex_value(*low)?);
            index += 3;
        } else if is_passthrough(byte) {
            out.push(byte);
            index += 1;
        } else {
            return None;
        }
    }
    Some(out)
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        _ => None,
    }
}

/// Whether a managed slug is already ASCII-casefold-stable
/// (`to_ascii_lowercase` is a fixed point). Every v2 output satisfies this
/// by construction; the predicate lets keepers assert the invariant as a
/// property rather than by example.
pub fn is_ascii_casefold_stable(slug: &str) -> bool {
    slug.bytes().all(|byte| !byte.is_ascii_uppercase())
}

/// Whether a slug uses only the canonical v2 output alphabet with a valid
/// version prefix and well-formed escapes (readable) or a 64-char
/// lowercase digest (digest form).
pub fn is_canonical_v2_slug(slug: &str) -> bool {
    if let Some(body) = slug.strip_prefix(MANAGED_READABLE_PREFIX) {
        if body.is_empty() || body.len() > MAX_READABLE_ESCAPED {
            return false;
        }
        return decode_readable_slug(slug).is_some();
    }
    if let Some(body) = slug.strip_prefix(MANAGED_DIGEST_PREFIX) {
        return body.len() == 64
            && body
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase());
    }
    false
}

/// Windows reserved device basenames (case-insensitive exact match).
/// Every v2 slug starts with the `e2-`/`h2-` letter prefix and every
/// worktree basename adds the outer `ws-` prefix, so no generated basename
/// can equal one of these — demonstrated by
/// `generated_basenames_never_hit_windows_reserved_names`.
pub const WINDOWS_RESERVED_BASENAMES: [&str; 22] = [
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

/// Whether a basename is a Windows reserved device name (ASCII
/// case-insensitive exact match, per Win32 rules).
pub fn is_windows_reserved_basename(name: &str) -> bool {
    WINDOWS_RESERVED_BASENAMES
        .iter()
        .any(|reserved| reserved.eq_ignore_ascii_case(name))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn slug(raw: &str) -> String {
        managed_slug(raw)
    }

    #[test]
    fn persisted_classifier_distinguishes_v2_legacy_and_mixed_pairs() {
        let root = Path::new("/managed");
        let workspace = WorkspaceId::new("ws-e2-legacy").unwrap();
        let owner = ExecutionId::new("e2-legacy").unwrap();

        let v2_branch = branch_for_execution(&owner).unwrap();
        let v2_path = root
            .join(format!("ws-{}", managed_slug(workspace.as_str())))
            .to_string_lossy()
            .into_owned();
        assert_eq!(
            classify_persisted_identity(root, &owner, &workspace, &v2_branch, &v2_path).unwrap(),
            PersistedManagedIdentityFormat::CurrentV2
        );

        let legacy_branch = BranchRef::new(format!(
            "refs/heads/agent/{}",
            legacy_task0051_slug(owner.as_str())
        ))
        .unwrap();
        let legacy_path = root
            .join(format!("ws-{}", legacy_task0051_slug(workspace.as_str())))
            .to_string_lossy()
            .into_owned();
        assert_eq!(legacy_branch.as_str(), "refs/heads/agent/e2-legacy");
        assert_eq!(legacy_path, "/managed/ws-ws-e2-legacy");
        assert_eq!(
            classify_persisted_identity(root, &owner, &workspace, &legacy_branch, &legacy_path)
                .unwrap(),
            PersistedManagedIdentityFormat::Legacy
        );

        assert!(
            classify_persisted_identity(root, &owner, &workspace, &v2_branch, &legacy_path)
                .is_err()
        );
        assert!(
            classify_persisted_identity(root, &owner, &workspace, &legacy_branch, &v2_path)
                .is_err()
        );
    }

    #[test]
    fn historical_prefix_collision_corpus_is_legacy_when_the_full_pair_matches() {
        let root = Path::new("/managed");
        for raw in [
            "e2-",
            "e2-x",
            "e2-legacy",
            "e2-e2-x",
            "e2-%41",
            "h2-",
            "h2-x",
            "h2-legacy",
            "h2-0123456789abcdef",
            "E2-X",
            "H2-X",
        ] {
            let owner = ExecutionId::new(raw).unwrap();
            let workspace = WorkspaceId::new(format!("ws-{raw}")).unwrap();
            for legacy_slug in [
                legacy_g4_slug as fn(&str) -> String,
                legacy_task0051_slug as fn(&str) -> String,
            ] {
                let branch =
                    BranchRef::new(format!("refs/heads/agent/{}", legacy_slug(owner.as_str())))
                        .unwrap();
                let path = root
                    .join(format!("ws-{}", legacy_slug(workspace.as_str())))
                    .to_string_lossy()
                    .into_owned();
                assert_eq!(
                    classify_persisted_identity(root, &owner, &workspace, &branch, &path).unwrap(),
                    PersistedManagedIdentityFormat::Legacy,
                    "{raw:?} must classify from the complete historical pair"
                );
            }
        }
    }

    #[test]
    fn escape_covers_every_byte_value_canonically() {
        for value in 0u8..=255 {
            let mut out = String::new();
            escape_byte(value, &mut out);
            if value.is_ascii_lowercase()
                || value.is_ascii_digit()
                || value == b'-'
                || value == b'_'
            {
                assert_eq!(out, (value as char).to_string(), "byte {value:#04x}");
            } else {
                assert_eq!(out.len(), 3, "byte {value:#04x} must escape to 3 chars");
                assert!(out.starts_with('%'), "byte {value:#04x}");
                assert!(
                    out.bytes()
                        .skip(1)
                        .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()),
                    "byte {value:#04x} hex must be canonical lowercase"
                );
                assert_eq!(u8::from_str_radix(&out[1..], 16).unwrap(), value);
            }
        }
    }

    #[test]
    fn exact_reference_examples() {
        // Uppercase is encoded, never normalized away: `A` and `a` stay
        // distinct source identities with distinct outputs.
        assert_eq!(slug("a"), "e2-a");
        assert_eq!(slug("A"), "e2-%41");
        // The TASK-0051 exact pair stays distinct under the new scheme.
        assert_eq!(slug("E/S"), "e2-%45%2f%53");
        assert_eq!(slug("E-2FS"), "e2-%45-2%46%53");
        // `%` itself is unambiguous: `%` → `%25`, never a bare escape.
        assert_eq!(slug("%"), "e2-%25");
        assert_eq!(slug("a%2Fb"), "e2-a%252%46b");
        assert_eq!(slug("a/b"), "e2-a%2fb");
        // Dots, spaces, backslashes never leak into the output.
        assert_eq!(slug(".."), "e2-%2e%2e");
        assert_eq!(slug("."), "e2-%2e");
        assert_eq!(slug("a b"), "e2-a%20b");
        assert_eq!(slug("a\\b"), "e2-a%5cb");
    }

    #[test]
    fn outputs_are_canonical_lowercase_and_short() {
        for raw in [
            "E-CASE",
            "E-case",
            "ABC",
            "Con",
            "COM1",
            "café-日本語-🎉",
            "../..",
            "%41",
        ] {
            let encoded = slug(raw);
            assert!(is_ascii_casefold_stable(&encoded), "{raw:?} → {encoded:?}");
            assert!(is_canonical_v2_slug(&encoded), "{raw:?} → {encoded:?}");
            assert!(encoded.len() <= 67, "{raw:?} → {encoded:?}");
        }
    }

    #[test]
    fn readable_form_round_trips_exact_bytes() {
        let corpus = [
            "E-CASE",
            "E-case",
            "a",
            "A",
            "E/S",
            "E-2FS",
            "100%25",
            "dot.name",
            "semi;colon",
            "a b",
            "caf\u{e9}",
            "cafe\u{301}",
            "日本語",
            "🎉",
            "..",
            ".",
            "@{1}",
            "trailing.",
            ".leading",
            "name.lock",
            "COM1",
            "con",
            "-leading",
            "under_score",
            "mix-2F-escape",
        ];
        for raw in corpus {
            let encoded = slug(raw);
            assert!(encoded.starts_with(MANAGED_READABLE_PREFIX), "{raw:?}");
            let decoded = decode_readable_slug(&encoded).expect("readable must decode");
            assert_eq!(
                decoded,
                raw.as_bytes(),
                "{raw:?} must round-trip byte-exact"
            );
            assert_eq!(String::from_utf8(decoded).unwrap(), raw);
        }
        // Digest slugs are not decodable — reversibility is honestly scoped.
        let long = "z".repeat(200);
        let digested = slug(&long);
        assert!(digested.starts_with(MANAGED_DIGEST_PREFIX));
        assert_eq!(decode_readable_slug(&digested), None);
        assert!(!is_canonical_v2_slug("e2-%4G"));
        assert!(!is_canonical_v2_slug("e2-%4"));
        assert!(!is_canonical_v2_slug("e2-A"));
        assert!(!is_canonical_v2_slug("h2-ABCDEF"));
        assert!(!is_canonical_v2_slug("agent/e-1"));
    }

    #[test]
    fn escaped_length_boundary_routes_readable_vs_digest() {
        // Escaped bodies of 63/64 stay readable; 65 falls to the digest.
        assert_eq!(slug(&"a".repeat(63)), format!("e2-{}", "a".repeat(63)));
        assert_eq!(slug(&"a".repeat(64)), format!("e2-{}", "a".repeat(64)));
        let digested = slug(&"a".repeat(65));
        assert!(digested.starts_with(MANAGED_DIGEST_PREFIX));
        assert_eq!(digested.len(), 67);
        // One multibyte char over the edge hashes rather than truncates.
        let edge = format!("{}é", "a".repeat(63));
        assert_eq!(slug(&edge).len(), 67);
        assert!(slug(&edge).starts_with(MANAGED_DIGEST_PREFIX));
    }

    #[test]
    fn digest_range_is_disjoint_from_readable_range() {
        // A literal digest-shaped identity is itself hashed — it can never
        // be mistaken for a digest output.
        let literal = format!("h2-{}", "a".repeat(64));
        let derived = slug(&literal);
        assert!(derived.starts_with(MANAGED_DIGEST_PREFIX));
        assert_ne!(derived, literal);
        // Same length, one byte apart → different digests.
        let mut first = "z".repeat(200);
        let mut second = "z".repeat(200);
        second.replace_range(100..101, "y");
        assert_ne!(slug(&first), slug(&second));
        first.push('q');
        assert_ne!(slug(&first), slug(&second));
    }

    #[test]
    fn forced_digest_collision_is_observable_through_the_seam() {
        // Narrow test seam (§16): production SHA-256 is untouched; a stub
        // digest lets the collision backstop prove fail-closed behavior on
        // two different long identities sharing one digest output.
        let stub = |_bytes: &[u8]| "ab".repeat(32);
        let first = ExecutionId::new("x".repeat(100)).unwrap();
        let mut second_raw = "x".repeat(100);
        second_raw.replace_range(50..51, "y");
        let second = ExecutionId::new(second_raw).unwrap();
        let a = branch_for_execution_with_digest(&first, stub).unwrap();
        let b = branch_for_execution_with_digest(&second, stub).unwrap();
        assert_eq!(a, b, "stub digest must force one shared managed identity");
    }

    #[test]
    fn generated_basenames_never_hit_windows_reserved_names() {
        // Every v2 worktree basename is `ws-<slug>` with a letter-led slug:
        // an exact reserved-device match is impossible by construction.
        let mut candidates = Vec::new();
        for reserved in WINDOWS_RESERVED_BASENAMES {
            for variant in [
                reserved.to_lowercase(),
                reserved.to_uppercase(),
                reserved.to_owned(),
            ] {
                candidates.push(variant);
            }
            candidates.push(format!("{reserved}.txt"));
        }
        candidates.extend(
            ["con", "COM1", "lpt9", "aux", "NUL", "A", "a"]
                .iter()
                .map(|raw| format!("ws-{}", slug(raw))),
        );
        for name in &candidates {
            if name.starts_with("ws-e2-") || name.starts_with("ws-h2-") {
                assert!(
                    !is_windows_reserved_basename(name),
                    "{name:?} must not be reserved"
                );
            }
        }
        for reserved in WINDOWS_RESERVED_BASENAMES {
            assert!(is_windows_reserved_basename(reserved));
            assert!(is_windows_reserved_basename(&reserved.to_lowercase()));
            // The generated form for the same spelling is never the bare name.
            assert_ne!(slug(reserved), reserved.to_lowercase());
            assert!(slug(reserved).starts_with(MANAGED_READABLE_PREFIX));
            assert!(!is_windows_reserved_basename(&format!(
                "ws-{}",
                slug(reserved)
            )));
            // No generated component ends in space/dot or holds invalid chars.
            for byte in slug(reserved).bytes() {
                assert!(!matches!(
                    byte,
                    b' ' | b'.' | b'<' | b'>' | b':' | b'"' | b'/' | b'\\' | b'|' | b'?' | b'*'
                ));
            }
        }
    }

    #[test]
    fn unicode_pairs_stay_byte_exact_without_normalization() {
        // NFC ≠ NFD at the byte level and must stay distinct identities.
        let composed = "caf\u{e9}";
        let decomposed = "cafe\u{301}";
        assert_ne!(composed.as_bytes(), decomposed.as_bytes());
        assert_ne!(slug(composed), slug(decomposed));
        for raw in [composed, decomposed, "日本語", "🎉", "fëature-日本語"] {
            let encoded = slug(raw);
            assert!(is_ascii_casefold_stable(&encoded), "{raw:?}");
            let decoded = decode_readable_slug(&encoded).expect("unicode must round-trip");
            assert_eq!(decoded, raw.as_bytes());
        }
    }

    #[test]
    fn corpus_has_no_collisions_raw_or_folded() {
        let mut corpus = vec![
            "E-1".to_owned(),
            "E-CASE".to_owned(),
            "E-case".to_owned(),
            "E-PACK".to_owned(),
            "E-pack".to_owned(),
            "A".to_owned(),
            "a".to_owned(),
            "ABC".to_owned(),
            "abc".to_owned(),
            "Execution".to_owned(),
            "execution".to_owned(),
            "E/S".to_owned(),
            "E-2FS".to_owned(),
            "E%2FS".to_owned(),
            "a".repeat(63),
            "a".repeat(64),
            "a".repeat(65),
            "x".repeat(256),
            "y".repeat(255),
            "-".to_owned(),
            "--".to_owned(),
            "%".to_owned(),
            "%%".to_owned(),
            "%%%".to_owned(),
            "h2-".to_owned() + &"a".repeat(64),
            "e2-foo".to_owned(),
            "日本語-日本語".to_owned(),
            "é".repeat(100),
            "Mixed_Case-123.ok".to_owned(),
            "..".to_owned(),
            ".".to_owned(),
            "HEAD".to_owned(),
            "main".to_owned(),
            "refs".to_owned(),
            "CON".to_owned(),
            "con".to_owned(),
            "COM1".to_owned(),
            "name.lock".to_owned(),
            "trailing.".to_owned(),
            ".leading".to_owned(),
            "a@{b}".to_owned(),
            "a~b".to_owned(),
            "a^b".to_owned(),
            "a:b".to_owned(),
            "a?b".to_owned(),
            "a*b".to_owned(),
            "a[b".to_owned(),
            "a\\b".to_owned(),
            "a b".to_owned(),
            "caf\u{e9}".to_owned(),
            "cafe\u{301}".to_owned(),
            "🎉".to_owned(),
        ];
        for byte in [0x00u8, 0x01, 0x09, 0x0a, 0x1f, 0x7f] {
            // 0x20 (space) is already covered literally above.
            corpus.push(format!("a{}b", byte as char));
        }
        for prefix in ["E", "E-", "E-2", "E-2F", "E-2FS", "E-2FS-"] {
            corpus.push(prefix.to_owned());
            corpus.push(format!("{prefix}/x"));
        }
        corpus.sort();
        corpus.dedup();
        let mut seen_raw = std::collections::BTreeMap::new();
        let mut seen_folded = std::collections::BTreeMap::new();
        for raw in &corpus {
            let id = ExecutionId::new(raw).unwrap();
            let branch = branch_for_execution(&id).unwrap();
            assert!(branch.is_managed(), "{raw:?} must stay managed");
            let short = branch.short_name().to_owned();
            let slug_part = short.strip_prefix("agent/").unwrap();
            assert!(is_canonical_v2_slug(slug_part), "{raw:?} → {slug_part:?}");
            assert!(
                !slug_part.contains('/'),
                "{raw:?} slug must be one component"
            );
            assert!(
                branch.as_str().len() <= 128,
                "{raw:?} branch must stay bounded"
            );
            if let Some(previous) = seen_raw.insert(branch.as_str().to_owned(), raw.clone()) {
                panic!("branch collision: {previous:?} and {raw:?} share a branch");
            }
            let folded = branch.as_str().to_ascii_lowercase();
            if let Some(previous) = seen_folded.insert(folded.clone(), raw.clone()) {
                panic!("case-folded branch collision: {previous:?} and {raw:?} share {folded:?}");
            }
            // Readable outputs reverse exactly.
            if slug_part.starts_with(MANAGED_READABLE_PREFIX) {
                let decoded = decode_readable_slug(slug_part).expect("readable must decode");
                assert_eq!(decoded, raw.as_bytes(), "{raw:?} must round-trip");
            }
        }
    }
}
