//! Password-protected file vault for brokered provider credentials
//! (ADR-0039 decision 3).
//!
//! A random data key encrypts the stored credentials. The data key is wrapped
//! twice: under an Argon2id key derived from the operator password and under
//! an Argon2id key derived from a generated recovery phrase. Primitives come
//! from audited RustCrypto crates; this crate only composes them.

use std::fmt;

use zeroize::Zeroizing;

mod envelope;
mod phrase;

pub use phrase::{RecoveryPhrase, DEFAULT_PHRASE_WORDS, MAX_PHRASE_WORDS, MIN_PHRASE_WORDS};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum VaultError {
    /// The password is shorter than [`Password::MIN_CHARS`].
    WeakPassword,
    /// The recovery phrase has a bad word count or a word not on the list.
    InvalidPhrase,
    /// Key-derivation parameters are outside the accepted bounds.
    InvalidParams,
    /// Wrong password or phrase, or a tampered key slot (indistinguishable).
    UnlockFailed,
    /// Vault data is malformed, tampered or of an unknown format version.
    Corrupt,
    /// An entry exceeds the format limits and was not written.
    EntryRejected,
    /// The platform random source or a primitive failed.
    Unavailable,
}

impl fmt::Display for VaultError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::WeakPassword => "vault password is too short",
            Self::InvalidPhrase => "recovery phrase is invalid",
            Self::InvalidParams => "vault key-derivation parameters are invalid",
            Self::UnlockFailed => "vault unlock failed",
            Self::Corrupt => "vault data is invalid",
            Self::EntryRejected => "vault entry exceeds format limits",
            Self::Unavailable => "vault cryptography unavailable",
        };
        f.write_str(message)
    }
}

impl std::error::Error for VaultError {}

/// Operator vault password. Held zeroizing; never rendered by `Debug`.
pub struct Password(Zeroizing<String>);

impl Password {
    pub const MIN_CHARS: usize = 12;

    pub fn new(value: String) -> Result<Self, VaultError> {
        let value = Zeroizing::new(value);
        if value.chars().count() < Self::MIN_CHARS {
            return Err(VaultError::WeakPassword);
        }
        Ok(Self(value))
    }

    pub(crate) fn as_bytes(&self) -> &[u8] {
        self.0.as_bytes()
    }
}

impl fmt::Debug for Password {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Password(..)")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn password_shorter_than_twelve_chars_is_rejected() {
        assert_eq!(
            Password::new("a".repeat(11)).unwrap_err(),
            VaultError::WeakPassword
        );
        assert!(Password::new("a".repeat(12)).is_ok());
    }

    #[test]
    fn password_length_counts_chars_not_bytes() {
        assert_eq!(
            Password::new("é".repeat(11)).unwrap_err(),
            VaultError::WeakPassword
        );
    }

    #[test]
    fn password_debug_never_renders_the_value() {
        let password = Password::new("correct-horse-battery".to_owned()).unwrap();
        assert_eq!(format!("{password:?}"), "Password(..)");
    }
}
