//! TASK-0157 (ADR-0036 IMPL-A): closed selector catalog contract tests.
//!
//! The catalog is the only lawful source of candidate-input selectors for
//! `EvidenceRequirement` params. v1 contains exactly
//! `candidate.file-contains.v1` (ADR-0034 §2, ADR-0036 §5/Option 5a).

use policy::{CandidateSelectorCatalog, CandidateSelectorCatalogRevision};

const SELECTOR_V1: &str = "candidate.file-contains.v1";

fn catalog_v1() -> CandidateSelectorCatalog {
    CandidateSelectorCatalog::v1()
}

#[test]
fn catalog_v1_contains_exactly_the_accepted_selector() {
    assert_eq!(catalog_v1().selectors(), vec![SELECTOR_V1]);
}

#[test]
fn uncatalogued_selector_is_rejected() {
    let error = catalog_v1()
        .validate_binding("candidate.evil.v1", "src/lib.rs", "needle")
        .expect_err("uncatalogued selector must fail closed");
    // F4: the error must not echo attacker-controlled input; membership is
    // already public via selectors().
    assert_eq!(
        error.to_string(),
        "selector is not in the closed v1 catalog (accepts: candidate.file-contains.v1)"
    );
}

#[test]
fn empty_selector_is_rejected() {
    catalog_v1()
        .validate_binding("", "src/lib.rs", "needle")
        .expect_err("empty selector must fail closed");
}

#[test]
fn revision_must_be_positive() {
    CandidateSelectorCatalogRevision::new(0, catalog_v1())
        .expect_err("revision 0 must be rejected");
}

#[test]
fn revision_fingerprint_is_stable_and_restore_round_trips() {
    let first = CandidateSelectorCatalogRevision::new(1, catalog_v1()).unwrap();
    let second = CandidateSelectorCatalogRevision::new(1, catalog_v1()).unwrap();
    assert_eq!(first.fingerprint(), second.fingerprint());

    let restored =
        CandidateSelectorCatalogRevision::restore(1, &first.serialize(), first.fingerprint())
            .unwrap();
    assert_eq!(restored.fingerprint(), first.fingerprint());
    assert_eq!(restored.catalog().selectors(), vec![SELECTOR_V1]);
}

#[test]
fn restore_rejects_tampered_fingerprint() {
    let revision = CandidateSelectorCatalogRevision::new(1, catalog_v1()).unwrap();
    CandidateSelectorCatalogRevision::restore(1, &revision.serialize(), "sha256:tampered")
        .expect_err("tampered fingerprint must fail closed");
}

#[test]
fn restore_rejects_unknown_encoding() {
    CandidateSelectorCatalogRevision::restore(1, "forbidden/v1", "sha256:anything")
        .expect_err("unknown encoding must fail closed");
}

#[test]
fn valid_binding_returns_bound_params() {
    let bound = catalog_v1()
        .validate_binding(SELECTOR_V1, "src/lib.rs", "needle")
        .unwrap();
    assert_eq!(bound.selector(), SELECTOR_V1);
    assert_eq!(bound.path(), "src/lib.rs");
    assert_eq!(bound.literal(), "needle");
}

#[test]
fn hostile_paths_are_rejected() {
    let hostile = [
        "",
        "/absolute/path.rs",
        "a/../../escape.rs",
        "..",
        " ..",
        " .git",
        ".",
        "a/./b.rs",
        "src//lib.rs",
        "src/lib.rs/",
        ".git/config",
        "src/.Git/hooks.rs",
        "src\\windows.rs",
        "src/lib\0.rs",
        "src/li\nb.rs",
        "src/li\x7fb.rs",
        "src/ünicode.rs",
    ];
    for path in hostile {
        catalog_v1()
            .validate_binding(SELECTOR_V1, path, "needle")
            .unwrap_err();
    }
}

#[test]
fn overlong_path_is_rejected() {
    let long = format!("src/{}.rs", "a".repeat(4096));
    catalog_v1()
        .validate_binding(SELECTOR_V1, &long, "needle")
        .expect_err("path over 4096 bytes must fail closed");
}

#[test]
fn empty_or_blank_literal_is_rejected() {
    for literal in ["", "   ", "\t"] {
        catalog_v1()
            .validate_binding(SELECTOR_V1, "src/lib.rs", literal)
            .unwrap_err();
    }
}

#[test]
fn overlong_literal_is_rejected() {
    catalog_v1()
        .validate_binding(SELECTOR_V1, "src/lib.rs", &"a".repeat(4097))
        .expect_err("literal over 4096 bytes must fail closed");
}

#[test]
fn control_characters_in_literal_are_rejected() {
    catalog_v1()
        .validate_binding(SELECTOR_V1, "src/lib.rs", "need\nle")
        .expect_err("control characters must fail closed");
}

#[test]
fn secret_shaped_literals_are_rejected() {
    // DATA_CLASSIFICATION.md §SECRET: credential material must never reach
    // durable state (INV-008/INV-151); params persist in SQLite.
    let secrets = [
        "-----BEGIN PRIVATE KEY-----\nabc",
        "-----BEGIN RSA PRIVATE KEY-----",
        "password=hunter2",
        "Password: hunter2",
        "passwd = hunter2",
        "api_key=hunter2",
        "APIKEY: hunter2",
        "api-key = hunter2",
        "secret=hunter2",
        "token: hunter2",
        "\"password\": \"hunter2\"",
        "{\"api_key\": \"sk-test\"}",
    ];
    for literal in secrets {
        catalog_v1()
            .validate_binding(SELECTOR_V1, "src/lib.rs", literal)
            .unwrap_err();
    }
}

#[test]
fn benign_needles_containing_sensitive_words_are_accepted() {
    // The screen matches credential *forms*, not substrings: a file-contains
    // needle mentioning a sensitive word is not credential material.
    for literal in [
        "fix token ring handling",
        "password",
        "the secret of good code",
        "api_key",
    ] {
        catalog_v1()
            .validate_binding(SELECTOR_V1, "src/lib.rs", literal)
            .unwrap();
    }
}
