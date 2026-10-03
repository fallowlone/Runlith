//! Vault file format v1 and envelope encryption.
//!
//! ```text
//! magic "RLVAULT\0" | version u16 | m_cost_kib u32 | t_cost u32 | p_cost u32
//! password slot:  salt[16] nonce[24] wrapped_dek[32 + 16 tag]
//! recovery slot:  salt[16] nonce[24] wrapped_dek[32 + 16 tag]
//! payload:        nonce[24] ciphertext(entries) + tag[16]
//! entries:        count u32, each { name_len u16, name, created_at u64,
//!                 value_len u32, value }
//! ```
//!
//! Integers are little-endian. Every AEAD call (XChaCha20-Poly1305) binds the
//! magic, version and KDF parameters plus a per-slot label as associated
//! data. Salts are bound implicitly: a changed salt derives a different key.

use argon2::{Algorithm, Argon2, Params, Version};
use chacha20poly1305::{AeadInOut, KeyInit, XChaCha20Poly1305, XNonce};
use zeroize::Zeroizing;

use crate::{Password, RecoveryPhrase, VaultError};

const MAGIC: &[u8; 8] = b"RLVAULT\0";
const FORMAT_VERSION: u16 = 1;
const KEY_LEN: usize = 32;
const SALT_LEN: usize = 16;
const NONCE_LEN: usize = 24;
const TAG_LEN: usize = 16;
const PREFIX_LEN: usize = MAGIC.len() + 2 + 3 * 4;
const SLOT_LEN: usize = SALT_LEN + NONCE_LEN + KEY_LEN + TAG_LEN;

const PASSWORD_LABEL: &[u8] = b"password";
const RECOVERY_LABEL: &[u8] = b"recovery";
const PAYLOAD_LABEL: &[u8] = b"payload";

/// Upper bounds checked before any derivation so a tampered file cannot
/// demand unbounded memory or time.
const MAX_M_COST_KIB: u32 = 1 << 20;
const MAX_T_COST: u32 = 16;
const MAX_P_COST: u32 = 4;

const MAX_ENTRIES: usize = 256;
const MAX_NAME_LEN: usize = 256;
const MAX_VALUE_LEN: usize = 64 * 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct KdfParams {
    m_cost_kib: u32,
    t_cost: u32,
    p_cost: u32,
}

impl KdfParams {
    /// Production Argon2id parameters for format v1 (ADR-0039 addendum).
    pub(crate) const V1: Self = Self {
        m_cost_kib: 262_144,
        t_cost: 3,
        p_cost: 1,
    };

    pub(crate) fn new(m_cost_kib: u32, t_cost: u32, p_cost: u32) -> Result<Self, VaultError> {
        let params = Self {
            m_cost_kib,
            t_cost,
            p_cost,
        };
        if m_cost_kib > MAX_M_COST_KIB || t_cost > MAX_T_COST || p_cost > MAX_P_COST {
            return Err(VaultError::InvalidParams);
        }
        params.argon2_params()?;
        Ok(params)
    }

    fn argon2_params(&self) -> Result<Params, VaultError> {
        Params::new(self.m_cost_kib, self.t_cost, self.p_cost, Some(KEY_LEN))
            .map_err(|_| VaultError::InvalidParams)
    }

    fn derive(&self, secret: &[u8], salt: &[u8; SALT_LEN]) -> Result<Key32, VaultError> {
        let mut key = Zeroizing::new([0_u8; KEY_LEN]);
        Argon2::new(Algorithm::Argon2id, Version::V0x13, self.argon2_params()?)
            .hash_password_into(secret, salt, key.as_mut())
            .map_err(|_| VaultError::Unavailable)?;
        Ok(key)
    }
}

type Key32 = Zeroizing<[u8; KEY_LEN]>;

/// The random data key. Zeroized on drop; never rendered.
pub(crate) struct Dek(Key32);

impl Dek {
    fn generate() -> Result<Self, VaultError> {
        let mut key = Zeroizing::new([0_u8; KEY_LEN]);
        getrandom::fill(key.as_mut()).map_err(|_| VaultError::Unavailable)?;
        Ok(Self(key))
    }
}

pub(crate) struct Entry {
    pub(crate) name: String,
    pub(crate) created_at: u64,
    pub(crate) value: Zeroizing<Vec<u8>>,
}

#[derive(Default)]
pub(crate) struct Entries(pub(crate) Vec<Entry>);

impl Entries {
    /// Shared by write and read so a file this code writes always reads back.
    fn within_limits(&self) -> bool {
        self.0.len() <= MAX_ENTRIES
            && self.0.iter().enumerate().all(|(index, entry)| {
                !entry.name.is_empty()
                    && entry.name.len() <= MAX_NAME_LEN
                    && entry.value.len() <= MAX_VALUE_LEN
                    && self.0[..index].iter().all(|other| other.name != entry.name)
            })
    }

    fn encode(&self) -> Result<Zeroizing<Vec<u8>>, VaultError> {
        if !self.within_limits() {
            return Err(VaultError::EntryRejected);
        }
        let len = 4 + self
            .0
            .iter()
            .map(|entry| 2 + entry.name.len() + 8 + 4 + entry.value.len())
            .sum::<usize>();
        // Exact capacity: no reallocation leaves a plaintext copy behind.
        let mut out = Zeroizing::new(Vec::with_capacity(len));
        out.extend_from_slice(&(self.0.len() as u32).to_le_bytes());
        for entry in &self.0 {
            out.extend_from_slice(&(entry.name.len() as u16).to_le_bytes());
            out.extend_from_slice(entry.name.as_bytes());
            out.extend_from_slice(&entry.created_at.to_le_bytes());
            out.extend_from_slice(&(entry.value.len() as u32).to_le_bytes());
            out.extend_from_slice(&entry.value);
        }
        Ok(out)
    }

    fn decode(plaintext: &[u8]) -> Result<Self, VaultError> {
        let mut reader = Reader(plaintext);
        let count = reader.u32()? as usize;
        if count > MAX_ENTRIES {
            return Err(VaultError::Corrupt);
        }
        let mut entries = Vec::with_capacity(count);
        for _ in 0..count {
            let name_len = usize::from(reader.u16()?);
            let name = std::str::from_utf8(reader.take(name_len)?)
                .map_err(|_| VaultError::Corrupt)?
                .to_owned();
            let created_at = reader.u64()?;
            let value_len = reader.u32()? as usize;
            let value = Zeroizing::new(reader.take(value_len)?.to_vec());
            entries.push(Entry {
                name,
                created_at,
                value,
            });
        }
        reader.finish()?;
        let entries = Self(entries);
        if entries.within_limits() {
            Ok(entries)
        } else {
            Err(VaultError::Corrupt)
        }
    }
}

#[derive(Clone)]
struct Slot {
    salt: [u8; SALT_LEN],
    nonce: [u8; NONCE_LEN],
    wrapped: [u8; KEY_LEN + TAG_LEN],
}

impl Slot {
    fn seal(
        params: &KdfParams,
        secret: &[u8],
        label: &[u8],
        dek: &Dek,
    ) -> Result<Self, VaultError> {
        let salt = random_array()?;
        let kek = params.derive(secret, &salt)?;
        let (nonce, wrapped) = seal(&kek, &aad(params, label), dek.0.as_slice())?;
        let wrapped = wrapped.try_into().map_err(|_| VaultError::Unavailable)?;
        Ok(Self {
            salt,
            nonce,
            wrapped,
        })
    }

    fn open(&self, params: &KdfParams, secret: &[u8], label: &[u8]) -> Result<Dek, VaultError> {
        let kek = params.derive(secret, &self.salt)?;
        let plaintext = open(&kek, &self.nonce, &aad(params, label), &self.wrapped)
            .map_err(|_| VaultError::UnlockFailed)?;
        let mut key = Zeroizing::new([0_u8; KEY_LEN]);
        key.copy_from_slice(&plaintext);
        Ok(Dek(key))
    }

    fn read(reader: &mut Reader<'_>) -> Result<Self, VaultError> {
        Ok(Self {
            salt: reader.array()?,
            nonce: reader.array()?,
            wrapped: reader.array()?,
        })
    }

    fn write(&self, out: &mut Vec<u8>) {
        out.extend_from_slice(&self.salt);
        out.extend_from_slice(&self.nonce);
        out.extend_from_slice(&self.wrapped);
    }
}

#[derive(Clone)]
pub(crate) struct Envelope {
    params: KdfParams,
    password_slot: Slot,
    recovery_slot: Slot,
    payload_nonce: [u8; NONCE_LEN],
    payload: Vec<u8>,
}

impl Envelope {
    /// New vault with a fresh data key and no entries.
    pub(crate) fn create(
        password: &Password,
        phrase: &RecoveryPhrase,
        params: KdfParams,
    ) -> Result<(Self, Dek), VaultError> {
        let dek = Dek::generate()?;
        let password_slot = Slot::seal(&params, password.as_bytes(), PASSWORD_LABEL, &dek)?;
        let recovery_slot = Slot::seal(&params, phrase.as_bytes(), RECOVERY_LABEL, &dek)?;
        let plaintext = Entries::default().encode()?;
        let (payload_nonce, payload) = seal(&dek.0, &aad(&params, PAYLOAD_LABEL), &plaintext)?;
        let envelope = Self {
            params,
            password_slot,
            recovery_slot,
            payload_nonce,
            payload,
        };
        Ok((envelope, dek))
    }

    pub(crate) fn parse(bytes: &[u8]) -> Result<Self, VaultError> {
        let mut reader = Reader(bytes);
        if reader.take(MAGIC.len())? != MAGIC || reader.u16()? != FORMAT_VERSION {
            return Err(VaultError::Corrupt);
        }
        let params = KdfParams::new(reader.u32()?, reader.u32()?, reader.u32()?)
            .map_err(|_| VaultError::Corrupt)?;
        let password_slot = Slot::read(&mut reader)?;
        let recovery_slot = Slot::read(&mut reader)?;
        let payload_nonce = reader.array()?;
        let payload = reader.0.to_vec();
        if payload.len() < TAG_LEN {
            return Err(VaultError::Corrupt);
        }
        Ok(Self {
            params,
            password_slot,
            recovery_slot,
            payload_nonce,
            payload,
        })
    }

    pub(crate) fn to_bytes(&self) -> Vec<u8> {
        let mut out =
            Vec::with_capacity(PREFIX_LEN + 2 * SLOT_LEN + NONCE_LEN + self.payload.len());
        out.extend_from_slice(&prefix(&self.params));
        self.password_slot.write(&mut out);
        self.recovery_slot.write(&mut out);
        out.extend_from_slice(&self.payload_nonce);
        out.extend_from_slice(&self.payload);
        out
    }

    pub(crate) fn unlock_with_password(&self, password: &Password) -> Result<Dek, VaultError> {
        self.password_slot
            .open(&self.params, password.as_bytes(), PASSWORD_LABEL)
    }

    pub(crate) fn unlock_with_recovery(&self, phrase: &RecoveryPhrase) -> Result<Dek, VaultError> {
        self.recovery_slot
            .open(&self.params, phrase.as_bytes(), RECOVERY_LABEL)
    }

    /// Re-wraps the data key under a new password. Payload and recovery slot
    /// are unchanged.
    pub(crate) fn rewrap_password(
        &self,
        dek: &Dek,
        password: &Password,
    ) -> Result<Self, VaultError> {
        Ok(Self {
            password_slot: Slot::seal(&self.params, password.as_bytes(), PASSWORD_LABEL, dek)?,
            ..self.clone()
        })
    }

    pub(crate) fn entries(&self, dek: &Dek) -> Result<Entries, VaultError> {
        let aad = aad(&self.params, PAYLOAD_LABEL);
        let plaintext = open(&dek.0, &self.payload_nonce, &aad, &self.payload)
            .map_err(|_| VaultError::Corrupt)?;
        Entries::decode(&plaintext)
    }

    pub(crate) fn with_entries(&self, dek: &Dek, entries: &Entries) -> Result<Self, VaultError> {
        let plaintext = entries.encode()?;
        let aad = aad(&self.params, PAYLOAD_LABEL);
        let (payload_nonce, payload) = seal(&dek.0, &aad, &plaintext)?;
        Ok(Self {
            params: self.params,
            password_slot: self.password_slot.clone(),
            recovery_slot: self.recovery_slot.clone(),
            payload_nonce,
            payload,
        })
    }
}

fn prefix(params: &KdfParams) -> [u8; PREFIX_LEN] {
    let mut out = [0_u8; PREFIX_LEN];
    let fields = [
        &MAGIC[..],
        &FORMAT_VERSION.to_le_bytes(),
        &params.m_cost_kib.to_le_bytes(),
        &params.t_cost.to_le_bytes(),
        &params.p_cost.to_le_bytes(),
    ];
    let mut at = 0;
    for field in fields {
        out[at..at + field.len()].copy_from_slice(field);
        at += field.len();
    }
    out
}

fn aad(params: &KdfParams, label: &[u8]) -> Vec<u8> {
    [&prefix(params)[..], label].concat()
}

pub(crate) fn random_array<const N: usize>() -> Result<[u8; N], VaultError> {
    let mut out = [0_u8; N];
    getrandom::fill(&mut out).map_err(|_| VaultError::Unavailable)?;
    Ok(out)
}

fn cipher(key: &[u8; KEY_LEN]) -> XChaCha20Poly1305 {
    XChaCha20Poly1305::new(key.into())
}

/// Encrypts under a fresh random nonce. Returns the nonce and ciphertext+tag.
fn seal(
    key: &[u8; KEY_LEN],
    aad: &[u8],
    plaintext: &[u8],
) -> Result<([u8; NONCE_LEN], Vec<u8>), VaultError> {
    let nonce = random_array()?;
    // Exact capacity: appending the tag never reallocates the plaintext.
    let mut buffer = Zeroizing::new(Vec::with_capacity(plaintext.len() + TAG_LEN));
    buffer.extend_from_slice(plaintext);
    cipher(key)
        .encrypt_in_place(&XNonce::from(nonce), aad, &mut *buffer)
        .map_err(|_| VaultError::Unavailable)?;
    Ok((nonce, std::mem::take(&mut *buffer)))
}

fn open(
    key: &[u8; KEY_LEN],
    nonce: &[u8; NONCE_LEN],
    aad: &[u8],
    ciphertext: &[u8],
) -> Result<Zeroizing<Vec<u8>>, chacha20poly1305::Error> {
    let mut buffer = Zeroizing::new(ciphertext.to_vec());
    cipher(key).decrypt_in_place(&XNonce::from(*nonce), aad, &mut *buffer)?;
    Ok(buffer)
}

/// Bounds-checked little-endian reader; every short read is `Corrupt`.
struct Reader<'a>(&'a [u8]);

impl<'a> Reader<'a> {
    fn take(&mut self, len: usize) -> Result<&'a [u8], VaultError> {
        let (head, tail) = self.0.split_at_checked(len).ok_or(VaultError::Corrupt)?;
        self.0 = tail;
        Ok(head)
    }

    fn array<const N: usize>(&mut self) -> Result<[u8; N], VaultError> {
        self.take(N)?.try_into().map_err(|_| VaultError::Corrupt)
    }

    fn u16(&mut self) -> Result<u16, VaultError> {
        self.array().map(u16::from_le_bytes)
    }

    fn u32(&mut self) -> Result<u32, VaultError> {
        self.array().map(u32::from_le_bytes)
    }

    fn u64(&mut self) -> Result<u64, VaultError> {
        self.array().map(u64::from_le_bytes)
    }

    fn finish(&self) -> Result<(), VaultError> {
        if self.0.is_empty() {
            Ok(())
        } else {
            Err(VaultError::Corrupt)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PASSWORD_SLOT: std::ops::Range<usize> = PREFIX_LEN..PREFIX_LEN + SLOT_LEN;
    const RECOVERY_SLOT: std::ops::Range<usize> = PASSWORD_SLOT.end..PASSWORD_SLOT.end + SLOT_LEN;

    fn fast_params() -> KdfParams {
        KdfParams::new(8, 1, 1).unwrap()
    }

    fn password(value: &str) -> Password {
        Password::new(value.to_owned()).unwrap()
    }

    fn entry(name: &str, value: &[u8]) -> Entry {
        Entry {
            name: name.to_owned(),
            created_at: 1_790_000_000,
            value: Zeroizing::new(value.to_vec()),
        }
    }

    fn sample() -> (Envelope, Dek, Password, RecoveryPhrase) {
        let password = password("correct-horse-battery");
        let phrase = RecoveryPhrase::generate(6).unwrap();
        let (envelope, dek) = Envelope::create(&password, &phrase, fast_params()).unwrap();
        let envelope = envelope
            .with_entries(
                &dek,
                &Entries(vec![entry("claude-token", b"material-0123")]),
            )
            .unwrap();
        (envelope, dek, password, phrase)
    }

    fn decrypts_with(envelope: &Envelope, password: &Password) -> bool {
        envelope
            .unlock_with_password(password)
            .and_then(|dek| envelope.entries(&dek))
            .is_ok()
    }

    #[test]
    fn password_and_recovery_unwrap_the_same_data_key() {
        let (envelope, dek, password, phrase) = sample();

        let by_password = envelope.unlock_with_password(&password).unwrap();
        let by_phrase = envelope.unlock_with_recovery(&phrase).unwrap();

        assert_eq!(*by_password.0, *dek.0);
        assert_eq!(*by_phrase.0, *dek.0);
    }

    #[test]
    fn wrong_password_or_phrase_fails_unlock() {
        let (envelope, _dek, _password, _phrase) = sample();

        assert_eq!(
            envelope
                .unlock_with_password(&password("incorrect-horse"))
                .err(),
            Some(VaultError::UnlockFailed)
        );
        assert_eq!(
            envelope
                .unlock_with_recovery(&RecoveryPhrase::generate(6).unwrap())
                .err(),
            Some(VaultError::UnlockFailed)
        );
    }

    #[test]
    fn serialized_envelope_round_trips_entries() {
        let (envelope, _dek, password, _phrase) = sample();

        let parsed = Envelope::parse(&envelope.to_bytes()).unwrap();
        let entries = parsed
            .entries(&parsed.unlock_with_password(&password).unwrap())
            .unwrap();

        assert_eq!(entries.0.len(), 1);
        assert_eq!(entries.0[0].name, "claude-token");
        assert_eq!(entries.0[0].created_at, 1_790_000_000);
        assert_eq!(entries.0[0].value.as_slice(), b"material-0123");
    }

    #[test]
    fn stored_bytes_never_contain_plaintext_material() {
        let (envelope, _dek, _password, _phrase) = sample();
        let bytes = envelope.to_bytes();
        assert!(!bytes.windows(13).any(|window| window == b"material-0123"));
    }

    #[test]
    fn password_change_rewraps_only_the_password_slot() {
        let (envelope, dek, old, phrase) = sample();
        let new = password("a-new-long-password");

        let changed = envelope.rewrap_password(&dek, &new).unwrap();
        let (before, after) = (envelope.to_bytes(), changed.to_bytes());

        assert_eq!(before[..PREFIX_LEN], after[..PREFIX_LEN]);
        assert_ne!(before[PASSWORD_SLOT], after[PASSWORD_SLOT]);
        assert_eq!(before[RECOVERY_SLOT.start..], after[RECOVERY_SLOT.start..]);
        assert!(!decrypts_with(&changed, &old));
        assert!(decrypts_with(&changed, &new));
        assert!(changed.unlock_with_recovery(&phrase).is_ok());
    }

    #[test]
    fn every_payload_write_uses_a_fresh_nonce() {
        let (envelope, dek, _password, _phrase) = sample();
        let entries = envelope.entries(&dek).unwrap();

        let first = envelope.with_entries(&dek, &entries).unwrap();
        let second = envelope.with_entries(&dek, &entries).unwrap();

        assert_ne!(first.payload_nonce, second.payload_nonce);
    }

    #[test]
    fn a_flipped_byte_in_any_region_is_detected() {
        let (envelope, _dek, password, phrase) = sample();
        let bytes = envelope.to_bytes();

        for offset in 0..bytes.len() {
            let mut tampered = bytes.clone();
            tampered[offset] ^= 0x01;
            let parsed = Envelope::parse(&tampered);
            let by_password = || {
                let envelope = parsed.as_ref().unwrap();
                envelope
                    .unlock_with_password(&password)
                    .and_then(|dek| envelope.entries(&dek).map(|_| ()))
            };
            let by_phrase = || parsed.as_ref().unwrap().unlock_with_recovery(&phrase);

            match offset {
                o if o < MAGIC.len() + 2 => {
                    assert_eq!(parsed.err(), Some(VaultError::Corrupt), "offset {o}")
                }
                o if o < PREFIX_LEN => assert!(
                    parsed.is_err() || (by_password().is_err() && by_phrase().is_err()),
                    "offset {o}"
                ),
                o if PASSWORD_SLOT.contains(&o) => {
                    assert_eq!(
                        by_password().err(),
                        Some(VaultError::UnlockFailed),
                        "offset {o}"
                    );
                    assert!(by_phrase().is_ok(), "offset {o}");
                }
                o if RECOVERY_SLOT.contains(&o) => {
                    assert_eq!(
                        by_phrase().err(),
                        Some(VaultError::UnlockFailed),
                        "offset {o}"
                    );
                    assert!(by_password().is_ok(), "offset {o}");
                }
                o => assert_eq!(by_password().err(), Some(VaultError::Corrupt), "offset {o}"),
            }
        }
    }

    #[test]
    fn truncated_or_extended_files_are_rejected() {
        let (envelope, _dek, password, _phrase) = sample();
        let bytes = envelope.to_bytes();
        let decrypts = |candidate: &[u8]| {
            Envelope::parse(candidate)
                .and_then(|envelope| {
                    let dek = envelope.unlock_with_password(&password)?;
                    envelope.entries(&dek).map(|_| ())
                })
                .is_ok()
        };

        for length in 0..bytes.len() {
            assert!(!decrypts(&bytes[..length]), "length {length}");
        }
        let mut extended = bytes.clone();
        extended.push(0);
        assert!(!decrypts(&extended));
    }

    #[test]
    fn kdf_params_above_bounds_are_rejected_before_derivation() {
        for (m, t, p) in [
            (MAX_M_COST_KIB + 1, 1, 1),
            (8, MAX_T_COST + 1, 1),
            (32, 1, MAX_P_COST + 1),
            (8, 0, 1),
            (4, 1, 1),
        ] {
            assert_eq!(KdfParams::new(m, t, p), Err(VaultError::InvalidParams));
        }
        assert!(KdfParams::new(MAX_M_COST_KIB, MAX_T_COST, MAX_P_COST).is_ok());

        let (envelope, _dek, _password, _phrase) = sample();
        let mut bytes = envelope.to_bytes();
        bytes[MAGIC.len() + 2..MAGIC.len() + 6].copy_from_slice(&u32::MAX.to_le_bytes());
        assert_eq!(Envelope::parse(&bytes).err(), Some(VaultError::Corrupt));
    }

    #[test]
    fn production_params_are_within_bounds() {
        let v1 = KdfParams::V1;
        assert_eq!(KdfParams::new(v1.m_cost_kib, v1.t_cost, v1.p_cost), Ok(v1));
    }

    #[test]
    fn entries_over_format_limits_are_not_written() {
        let (envelope, dek, _password, _phrase) = sample();
        let oversized = [
            Entries(vec![entry("big", &vec![0; MAX_VALUE_LEN + 1])]),
            Entries(vec![entry(&"n".repeat(MAX_NAME_LEN + 1), b"v")]),
            Entries(vec![entry("", b"v")]),
            Entries(vec![entry("dup", b"a"), entry("dup", b"b")]),
            Entries(
                (0..=MAX_ENTRIES)
                    .map(|i| entry(&format!("e{i}"), b"v"))
                    .collect(),
            ),
        ];
        for entries in oversized {
            assert_eq!(
                envelope.with_entries(&dek, &entries).err(),
                Some(VaultError::EntryRejected)
            );
        }
    }

    #[test]
    fn malformed_entry_plaintext_is_corrupt() {
        let well_formed = Entries(vec![entry("a", b"v")]).encode().unwrap();
        let mut trailing = well_formed.to_vec();
        trailing.push(0);
        let mut duplicate = Vec::from(2_u32.to_le_bytes());
        for _ in 0..2 {
            duplicate.extend_from_slice(&well_formed[4..]);
        }
        let mut too_many = well_formed.to_vec();
        too_many[..4].copy_from_slice(&(MAX_ENTRIES as u32 + 1).to_le_bytes());

        for plaintext in [trailing, duplicate, too_many, vec![1, 0, 0]] {
            assert_eq!(Entries::decode(&plaintext).err(), Some(VaultError::Corrupt));
        }
    }
}
