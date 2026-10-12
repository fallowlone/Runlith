//! Durable ADR-0033 review authority seam (TASK-0145).
//!
//! Append-only tables for owner presence proofs (one exact operation
//! each), per-task review-path settings (revisioned history consuming
//! proofs), product-managed reviewer profiles, and review records
//! binding every ADR-0033 field with proof-checked nonces, linear
//! supersession, and sticky invalidation. All reads rebuild through
//! the validated domain constructors, so stored content can never
//! bypass validation.

use evaluation::{ReviewPathName, ReviewRecord, ReviewRecordFields, ReviewVerdictName};
use policy::{ReviewPathSetting, ReviewerProfile};
use rusqlite::{params, OptionalExtension};

use crate::{StorageError, StorageResult};

fn corrupt(what: &str) -> StorageError {
    StorageError::Corrupt(format!("review authority store: {what}"))
}

fn domain(error: impl std::fmt::Display) -> StorageError {
    StorageError::DomainReconstruction(format!("review authority store: {error}"))
}

/// Length-prefixed list framing (`len:bytes` concatenated, empty list
/// as the empty string). No escaping rules to get wrong: lengths are
/// unambiguous by construction, and decoding is strict (any deviation
/// is corruption, never a guess).
pub(crate) fn encode_list(values: &[String]) -> String {
    let mut encoded = String::new();
    for value in values {
        encoded.push_str(&format!("{}:{value}", value.len()));
    }
    encoded
}

pub(crate) fn decode_list(encoded: &str) -> StorageResult<Vec<String>> {
    let mut values = Vec::new();
    let mut rest = encoded;
    while !rest.is_empty() {
        let colon = rest
            .find(':')
            .ok_or_else(|| corrupt("list entry lacks length"))?;
        let length: usize = rest[..colon]
            .parse()
            .map_err(|_| corrupt("list entry has non-numeric length"))?;
        let body = &rest.as_bytes()[colon + 1..];
        if body.len() < length {
            return Err(corrupt("list entry overruns its framing"));
        }
        let entry =
            std::str::from_utf8(&body[..length]).map_err(|_| corrupt("list entry is not UTF-8"))?;
        values.push(entry.to_owned());
        rest = &rest[colon + 1 + length..];
    }
    Ok(values)
}

type SettingRow = (i64, String, String, i64, String);

fn setting_from_row(row: SettingRow) -> StorageResult<ReviewPathSetting> {
    let (revision, path, fingerprint, _changed_at_ms, _nonce) = row;
    let revision = u64::try_from(revision).map_err(|_| corrupt("setting revision"))?;
    ReviewPathSetting::restore(revision, &path, &fingerprint).map_err(domain)
}

type RecordRow = (
    String,
    String,
    String,
    String,
    String,
    String,
    String,
    i64,
    String,
    String,
    i64,
    i64,
    String,
    String,
    String,
    String,
    String,
    String,
    String,
    String,
    String,
    String,
    i64,
    Option<String>,
    String,
);

#[allow(clippy::too_many_lines)]
fn record_from_row(row: RecordRow) -> StorageResult<ReviewRecord> {
    let (
        record_id,
        task_id,
        requirement_revision,
        base_manifest,
        candidate_manifest,
        changed_inputs,
        affected_checks,
        policy_revision,
        policy_fingerprint,
        profile_id,
        profile_revision,
        setting_revision,
        setting_path,
        reviewer_namespace,
        reviewer_nonce,
        reviewer_input_digest,
        reviewer_verdict,
        implementer_execution_id,
        implementer_attempt_id,
        implementer_admission_ref,
        rationale,
        evidence_refs,
        captured_at_ms,
        supersedes,
        fingerprint,
    ) = row;
    if reviewer_namespace != "human/v1" {
        return Err(corrupt("reviewer namespace drift"));
    }
    let fields = ReviewRecordFields {
        record_id,
        task_id,
        requirement_revision,
        base_manifest,
        candidate_manifest,
        changed_inputs: decode_list(&changed_inputs)?,
        affected_checks: decode_list(&affected_checks)?,
        policy_revision: u64::try_from(policy_revision).map_err(|_| corrupt("policy revision"))?,
        policy_fingerprint,
        profile_id,
        profile_revision: u64::try_from(profile_revision)
            .map_err(|_| corrupt("profile revision"))?,
        setting_revision: u64::try_from(setting_revision)
            .map_err(|_| corrupt("setting revision"))?,
        setting_path: ReviewPathName::deserialize(&setting_path).map_err(domain)?,
        reviewer_nonce,
        reviewer_input_digest,
        reviewer_verdict: ReviewVerdictName::deserialize(&reviewer_verdict).map_err(domain)?,
        implementer_execution_id,
        implementer_attempt_id,
        implementer_admission_ref,
        rationale,
        evidence_refs: decode_list(&evidence_refs)?,
        captured_at_ms: u64::try_from(captured_at_ms).map_err(|_| corrupt("capture timestamp"))?,
        supersedes,
    };
    ReviewRecord::restore(fields, &fingerprint).map_err(domain)
}

/// Mutable lifecycle state of an otherwise immutable review record.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReviewRecordState {
    pub invalidated_at_ms: Option<i64>,
    pub invalidation_code: Option<String>,
    pub superseded_by: Option<String>,
}

impl crate::SqliteStore {
    /// Persists one owner presence proof. The nonce is the primary key:
    /// a replayed nonce is a constraint violation, never a second
    /// proof.
    pub fn record_presence_proof(
        &mut self,
        nonce: &str,
        operation: &str,
        input_digest: &str,
        proven_at_ms: i64,
    ) -> StorageResult<()> {
        self.connection_mut()
            .execute(
                "INSERT INTO owner_presence_proofs (nonce, operation, input_digest, proven_at_ms)
                 VALUES (?1, ?2, ?3, ?4)",
                params![nonce, operation, input_digest, proven_at_ms],
            )
            .map_err(|error| crate::error::map_constraint(error, "store presence proof"))?;
        Ok(())
    }

    /// Appends one review-path setting revision for a task, consuming
    /// the named proof. The schema refuses a proof that binds any
    /// other operation and a nonce that is already consumed.
    pub fn record_review_path_setting(
        &mut self,
        task_id: &str,
        setting: &ReviewPathSetting,
        presence_nonce: &str,
        changed_at_ms: i64,
    ) -> StorageResult<ReviewPathSetting> {
        let revision =
            i64::try_from(setting.revision()).map_err(|_| corrupt("setting revision"))?;
        self.connection_mut()
            .execute(
                "INSERT INTO review_path_settings
                 (task_id, setting_revision, path, fingerprint, changed_at_ms, presence_nonce)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    task_id,
                    revision,
                    setting.path().serialize(),
                    setting.fingerprint(),
                    changed_at_ms,
                    presence_nonce,
                ],
            )
            .map_err(|error| crate::error::map_constraint(error, "store review path setting"))?;
        Ok(setting.clone())
    }

    /// Reads the current (highest-revision) review-path setting for a
    /// task, rebuilding through the validated constructor.
    pub fn get_review_path_setting(
        &self,
        task_id: &str,
    ) -> StorageResult<Option<ReviewPathSetting>> {
        let row: Option<SettingRow> = self
            .connection()
            .query_row(
                "SELECT setting_revision, path, fingerprint, changed_at_ms, presence_nonce
                 FROM review_path_settings WHERE task_id = ?1
                 ORDER BY setting_revision DESC LIMIT 1",
                params![task_id],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                    ))
                },
            )
            .optional()?;
        row.map(setting_from_row).transpose()
    }

    /// Persists one product-managed reviewer profile. Profiles are
    /// immutable; a new product release is a new row (same id is a
    /// duplicate, never an overwrite).
    pub fn record_reviewer_profile(
        &mut self,
        profile: &ReviewerProfile,
    ) -> StorageResult<ReviewerProfile> {
        let revision =
            i64::try_from(profile.revision()).map_err(|_| corrupt("profile revision"))?;
        self.connection_mut()
            .execute(
                "INSERT INTO reviewer_profiles (profile_id, profile_revision, key_ref, fingerprint)
                 VALUES (?1, ?2, ?3, ?4)",
                params![
                    profile.profile_id(),
                    revision,
                    profile.key_ref(),
                    profile.fingerprint(),
                ],
            )
            .map_err(|error| crate::error::map_constraint(error, "store reviewer profile"))?;
        Ok(profile.clone())
    }

    /// Appends one review record. One SQL statement: the schema
    /// checks the proof operation, the nonce single-use, and the
    /// relational bindings, and links the superseded parent (if any)
    /// inside the same statement — linear history without a
    /// read-modify-write race.
    pub fn record_review(&mut self, record: &ReviewRecord) -> StorageResult<ReviewRecord> {
        let policy_revision =
            i64::try_from(record.policy_revision()).map_err(|_| corrupt("policy revision"))?;
        let profile_revision =
            i64::try_from(record.profile_revision()).map_err(|_| corrupt("profile revision"))?;
        let setting_revision =
            i64::try_from(record.setting_revision()).map_err(|_| corrupt("setting revision"))?;
        let captured_at_ms =
            i64::try_from(record.captured_at_ms()).map_err(|_| corrupt("capture timestamp"))?;
        self.connection_mut()
            .execute(
                "INSERT INTO review_records
                 (record_id, task_id, requirement_revision, base_manifest, candidate_manifest,
                  changed_inputs, affected_checks, policy_revision, policy_fingerprint,
                  profile_id, profile_revision, setting_revision, setting_path,
                  reviewer_namespace, reviewer_nonce, reviewer_input_digest, reviewer_verdict,
                  implementer_execution_id, implementer_attempt_id, implementer_admission_ref,
                  rationale, evidence_refs, captured_at_ms, supersedes, fingerprint)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15,
                         ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24, ?25)",
                params![
                    record.record_id(),
                    record.task_id(),
                    record.requirement_revision(),
                    record.base_manifest(),
                    record.candidate_manifest(),
                    encode_list(record.changed_inputs()),
                    encode_list(record.affected_checks()),
                    policy_revision,
                    record.policy_fingerprint(),
                    record.profile_id(),
                    profile_revision,
                    setting_revision,
                    record.setting_path().serialize(),
                    "human/v1",
                    record.reviewer_nonce(),
                    record.reviewer_input_digest(),
                    record.reviewer_verdict().serialize(),
                    record.implementer_execution_id(),
                    record.implementer_attempt_id(),
                    record.implementer_admission_ref(),
                    record.rationale(),
                    encode_list(record.evidence_refs()),
                    captured_at_ms,
                    record.supersedes(),
                    record.fingerprint(),
                ],
            )
            .map_err(|error| crate::error::map_constraint(error, "store review record"))?;
        Ok(record.clone())
    }

    /// Reads one review record by id, rebuilding through the validated
    /// constructor (fingerprint verified).
    pub fn get_review_record(&self, record_id: &str) -> StorageResult<Option<ReviewRecord>> {
        let row: Option<RecordRow> = self
            .connection()
            .query_row(
                "SELECT record_id, task_id, requirement_revision, base_manifest, candidate_manifest,
                        changed_inputs, affected_checks, policy_revision, policy_fingerprint,
                        profile_id, profile_revision, setting_revision, setting_path,
                        reviewer_namespace, reviewer_nonce, reviewer_input_digest, reviewer_verdict,
                        implementer_execution_id, implementer_attempt_id, implementer_admission_ref,
                        rationale, evidence_refs, captured_at_ms, supersedes, fingerprint
                 FROM review_records WHERE record_id = ?1",
                params![record_id],
                |row| {
                    Ok((
                        row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?,
                        row.get(5)?, row.get(6)?, row.get(7)?, row.get(8)?, row.get(9)?,
                        row.get(10)?, row.get(11)?, row.get(12)?, row.get(13)?, row.get(14)?,
                        row.get(15)?, row.get(16)?, row.get(17)?, row.get(18)?, row.get(19)?,
                        row.get(20)?, row.get(21)?, row.get(22)?, row.get(23)?, row.get(24)?,
                    ))
                },
            )
            .optional()?;
        row.map(record_from_row).transpose()
    }

    /// Lists a task's review records in insertion order.
    pub fn list_review_records_for_task(&self, task_id: &str) -> StorageResult<Vec<ReviewRecord>> {
        let connection = self.connection();
        let mut statement = connection.prepare(
            "SELECT record_id, task_id, requirement_revision, base_manifest, candidate_manifest,
                    changed_inputs, affected_checks, policy_revision, policy_fingerprint,
                    profile_id, profile_revision, setting_revision, setting_path,
                    reviewer_namespace, reviewer_nonce, reviewer_input_digest, reviewer_verdict,
                    implementer_execution_id, implementer_attempt_id, implementer_admission_ref,
                    rationale, evidence_refs, captured_at_ms, supersedes, fingerprint
             FROM review_records WHERE task_id = ?1 ORDER BY rowid",
        )?;
        let rows = statement.query_map(params![task_id], |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
                row.get(5)?,
                row.get(6)?,
                row.get(7)?,
                row.get(8)?,
                row.get(9)?,
                row.get(10)?,
                row.get(11)?,
                row.get(12)?,
                row.get(13)?,
                row.get(14)?,
                row.get(15)?,
                row.get(16)?,
                row.get(17)?,
                row.get(18)?,
                row.get(19)?,
                row.get(20)?,
                row.get(21)?,
                row.get(22)?,
                row.get(23)?,
                row.get(24)?,
            ))
        })?;
        let mut records = Vec::new();
        for row in rows {
            records.push(record_from_row(row?)?);
        }
        Ok(records)
    }

    /// Reads the live head for a task: the newest record that is
    /// neither invalidated nor superseded. Linear history has at most
    /// one; the service chains new reviews onto it.
    pub fn live_review_record_head(&self, task_id: &str) -> StorageResult<Option<ReviewRecord>> {
        let connection = self.connection();
        let mut statement = connection.prepare(
            "SELECT record_id, task_id, requirement_revision, base_manifest, candidate_manifest,
                    changed_inputs, affected_checks, policy_revision, policy_fingerprint,
                    profile_id, profile_revision, setting_revision, setting_path,
                    reviewer_namespace, reviewer_nonce, reviewer_input_digest, reviewer_verdict,
                    implementer_execution_id, implementer_attempt_id, implementer_admission_ref,
                    rationale, evidence_refs, captured_at_ms, supersedes, fingerprint
             FROM review_records
             WHERE task_id = ?1 AND invalidated_at_ms IS NULL AND superseded_by IS NULL
             ORDER BY rowid DESC LIMIT 1",
        )?;
        let row: Option<RecordRow> = statement
            .query_row(params![task_id], |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                    row.get(6)?,
                    row.get(7)?,
                    row.get(8)?,
                    row.get(9)?,
                    row.get(10)?,
                    row.get(11)?,
                    row.get(12)?,
                    row.get(13)?,
                    row.get(14)?,
                    row.get(15)?,
                    row.get(16)?,
                    row.get(17)?,
                    row.get(18)?,
                    row.get(19)?,
                    row.get(20)?,
                    row.get(21)?,
                    row.get(22)?,
                    row.get(23)?,
                    row.get(24)?,
                ))
            })
            .optional()?;
        row.map(record_from_row).transpose()
    }

    /// Reads the mutable lifecycle state of one review record.
    pub fn review_record_state(&self, record_id: &str) -> StorageResult<Option<ReviewRecordState>> {
        let row: Option<(Option<i64>, Option<String>, Option<String>)> = self
            .connection()
            .query_row(
                "SELECT invalidated_at_ms, invalidation_code, superseded_by
                 FROM review_records WHERE record_id = ?1",
                params![record_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()?;
        Ok(row.map(
            |(invalidated_at_ms, invalidation_code, superseded_by)| ReviewRecordState {
                invalidated_at_ms,
                invalidation_code,
                superseded_by,
            },
        ))
    }

    /// Marks one review record invalidated. The transition is sticky:
    /// a second invalidation with different values refuses; an
    /// identical retry is a harmless no-op.
    pub fn invalidate_review_record(
        &mut self,
        record_id: &str,
        invalidation_code: &str,
        invalidated_at_ms: i64,
    ) -> StorageResult<()> {
        let changed = self
            .connection_mut()
            .execute(
                "UPDATE review_records
                 SET invalidated_at_ms = ?1, invalidation_code = ?2
                 WHERE record_id = ?3",
                params![invalidated_at_ms, invalidation_code, record_id],
            )
            .map_err(|error| crate::error::map_constraint(error, "invalidate review record"))?;
        if changed == 0 {
            return Err(StorageError::NotFound(format!(
                "review record not found: {record_id}"
            )));
        }
        Ok(())
    }
}
