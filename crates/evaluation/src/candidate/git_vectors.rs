//! SHA-1 object ids must equal what real git prints. The tests in `mod tests`
//! derive their expected ids with the same `sha1` crate, so they cannot notice a
//! dependency bump that changes the digest; these vectors come from
//! `git hash-object --stdin`, not from this code.

use super::{verify_git_object, GitObjectFormat};

#[test]
fn blob_ids_match_git_hash_object() {
    let vectors: [(&[u8], &str); 4] = [
        (
            b"needle in candidate",
            "04fce9163abbc089a0db555a52a71747552e01d4",
        ),
        (b"", "e69de29bb2d1d6434b8b29ae775ad8c2e48c5391"),
        (b"hello", "b6fc4c620b67d95f953a5c1c1230aaab5db5a1b0"),
        (&[b'x'; 70000], "2a19a886fed45ff5999d8c2a529ba35852fb32b8"),
    ];
    for (bytes, oid) in vectors {
        verify_git_object(GitObjectFormat::Sha1, oid, "blob", bytes)
            .unwrap_or_else(|error| panic!("git id {oid} refused: {error:?}"));
    }
}

#[test]
fn a_flipped_byte_is_refused() {
    let result = verify_git_object(
        GitObjectFormat::Sha1,
        "b6fc4c620b67d95f953a5c1c1230aaab5db5a1b0",
        "blob",
        b"hellp",
    );
    assert!(result.is_err());
}
