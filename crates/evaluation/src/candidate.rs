//! Verified immutable Git candidate bytes (ADR-0035).

use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

pub const MAX_CANDIDATE_ENTRIES: usize = 16_384;
pub const MAX_CANDIDATE_TREE_DEPTH: usize = 64;
pub const MAX_CANDIDATE_BLOB_BYTES: u64 = 256 * 1024 * 1024;

pub fn candidate_sha256(bytes: &[u8]) -> String {
    format!("sha256:{:x}", Sha256::digest(bytes))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GitObjectFormat {
    Sha1,
    Sha256,
}

impl GitObjectFormat {
    pub fn from_repository_format(name: &str) -> Result<Self, CandidateError> {
        match name {
            "sha1" => Ok(Self::Sha1),
            "sha256" => Ok(Self::Sha256),
            _ => Err(CandidateError::UnsupportedObjectFormat),
        }
    }

    pub fn from_oid(oid: &str) -> Result<Self, CandidateError> {
        match oid.len() {
            40 if lowercase_hex(oid) => Ok(Self::Sha1),
            64 if lowercase_hex(oid) => Ok(Self::Sha256),
            _ => Err(CandidateError::InvalidObjectId),
        }
    }

    pub const fn oid_len(self) -> usize {
        match self {
            Self::Sha1 => 20,
            Self::Sha256 => 32,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CandidateFile {
    pub path: Vec<u8>,
    pub mode: u32,
    pub git_oid: Option<String>,
    pub bytes: Vec<u8>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CandidateManifestEntry {
    pub path: Vec<u8>,
    pub mode: u32,
    pub length: u64,
    pub sha256: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CandidateManifest {
    pub version: u32,
    pub entries: Vec<CandidateManifestEntry>,
    pub sha256: String,
    pub total_bytes: u64,
}

/// A committed path present in the evaluated base tree and absent from the
/// selected candidate tree. Raw path bytes and the base blob digest are kept
/// so the deletion remains independently auditable after restart.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CandidateDeletion {
    pub path: Vec<u8>,
    pub base_mode: u32,
    pub base_blob_sha256: String,
}

impl CandidateDeletion {
    pub fn between(base: &CandidateManifest, candidate: &CandidateManifest) -> Vec<Self> {
        base.entries
            .iter()
            .filter(|entry| {
                candidate
                    .entries
                    .binary_search_by(|candidate_entry| candidate_entry.path.cmp(&entry.path))
                    .is_err()
            })
            .map(|entry| Self {
                path: entry.path.clone(),
                base_mode: entry.mode,
                base_blob_sha256: entry.sha256.clone(),
            })
            .collect()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerifiedCandidate {
    pub commit_oid: String,
    pub tree_oid: String,
    pub manifest: CandidateManifest,
    pub files: Vec<CandidateFile>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CandidateTestResult {
    pub selector: String,
    pub passed: u32,
    pub failed: u32,
    pub skipped: u32,
    pub result_sha256: String,
}

/// Durable product assessment for the single catalogued candidate-input
/// selector. The two manifests bind both immutable commits and all observed
/// boundary checkpoints; this type deliberately makes no claim about builds,
/// tests, or runtime behavior.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CandidateIntegrityAssessment {
    pub task_id: String,
    pub execution_id: String,
    pub attempt_id: String,
    pub subject_ref: String,
    pub requirement_revision: String,
    pub policy_revision: u64,
    pub policy_fingerprint: String,
    pub criterion_id: String,
    pub selector: String,
    pub selector_path: String,
    pub selector_literal: String,
    pub invocation_id: String,
    pub evaluator_ref: String,
    pub runtime_ref: String,
    pub enforcement_ref: String,
    pub invocation_descriptor_version: u32,
    pub invocation_descriptor_sha256: String,
    pub evaluator_executable: String,
    pub toolchain_ref: String,
    pub runner_backend: String,
    pub runner_plan_sha256: String,
    /// Runner enforcement axes: filesystem=1, network=2, process=4.
    pub runner_enforced_axes: u8,
    pub runner_env_isolated: bool,
    pub base_commit_oid: String,
    pub base_tree_oid: String,
    pub base_manifest_sha256: String,
    pub candidate_commit_oid: String,
    pub candidate_tree_oid: String,
    pub candidate_manifest_sha256: String,
    pub deletions: Vec<CandidateDeletion>,
    pub checkpoints: Vec<CandidateFreshnessCheckpoint>,
    pub outcome: CandidateIntegrityOutcome,
    pub started_at_ms: u64,
    pub completed_at_ms: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CandidateFreshnessCheckpoint {
    pub phase: CandidateCheckpointPhase,
    pub observed_at_ms: u64,
    pub git_metadata_fingerprint: String,
    pub live_worktree_manifest_sha256: String,
    pub matched: bool,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum CandidateCheckpointPhase {
    Selection,
    BeforeEvaluation,
    AfterEvaluation,
    ResultAdmission,
    Completion,
    RestartReuse,
}

impl CandidateCheckpointPhase {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Selection => "selection",
            Self::BeforeEvaluation => "before-evaluation",
            Self::AfterEvaluation => "after-evaluation",
            Self::ResultAdmission => "result-admission",
            Self::Completion => "completion",
            Self::RestartReuse => "restart-reuse",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CandidateIntegrityOutcome {
    Intact,
    Mismatch,
}

impl CandidateIntegrityAssessment {
    pub fn refresh_invocation_descriptor_digest(&mut self) {
        self.invocation_descriptor_sha256 = self.canonical_invocation_descriptor_digest();
    }

    #[must_use]
    pub fn canonical_invocation_descriptor_digest(&self) -> String {
        let mut hash = Sha256::new();
        hash.update(b"runlith:candidate-invocation-descriptor:v1\0");
        for value in [
            self.task_id.as_str(),
            self.execution_id.as_str(),
            self.attempt_id.as_str(),
            self.subject_ref.as_str(),
            self.base_commit_oid.as_str(),
            self.base_tree_oid.as_str(),
            self.base_manifest_sha256.as_str(),
            self.candidate_commit_oid.as_str(),
            self.candidate_tree_oid.as_str(),
            self.candidate_manifest_sha256.as_str(),
            self.requirement_revision.as_str(),
            self.policy_fingerprint.as_str(),
            self.criterion_id.as_str(),
            self.selector.as_str(),
            self.selector_path.as_str(),
            self.selector_literal.as_str(),
            self.invocation_id.as_str(),
            self.evaluator_ref.as_str(),
            self.runtime_ref.as_str(),
            self.evaluator_executable.as_str(),
            self.toolchain_ref.as_str(),
            self.enforcement_ref.as_str(),
            self.runner_backend.as_str(),
            self.runner_plan_sha256.as_str(),
        ] {
            field(&mut hash, value.as_bytes());
        }
        hash.update(self.policy_revision.to_be_bytes());
        hash.update(self.invocation_descriptor_version.to_be_bytes());
        hash.update([
            self.runner_enforced_axes,
            u8::from(self.runner_env_isolated),
        ]);
        hash.update((self.deletions.len() as u64).to_be_bytes());
        for deletion in &self.deletions {
            field(&mut hash, &deletion.path);
            hash.update(deletion.base_mode.to_be_bytes());
            field(&mut hash, deletion.base_blob_sha256.as_bytes());
            hash.update([0]); // Candidate path is absent by deletion definition.
        }
        format!("sha256:{:x}", hash.finalize())
    }

    pub fn validate(&self) -> Result<(), CandidateError> {
        let phases = self
            .checkpoints
            .iter()
            .map(|checkpoint| checkpoint.phase)
            .collect::<BTreeSet<_>>();
        let unique_non_reuse_phases = self
            .checkpoints
            .iter()
            .filter(|checkpoint| checkpoint.phase != CandidateCheckpointPhase::RestartReuse)
            .map(|checkpoint| checkpoint.phase)
            .collect::<BTreeSet<_>>();
        if self.task_id.is_empty()
            || self.execution_id.is_empty()
            || self.attempt_id.is_empty()
            || self.subject_ref.is_empty()
            || self.requirement_revision.is_empty()
            || self.policy_revision == 0
            || self.policy_fingerprint.is_empty()
            || self.criterion_id.is_empty()
            || self.selector != "candidate.file-contains.v1"
            || self.selector_path.is_empty()
            || self.selector_path.len() > 4096
            || validate_candidate_path(self.selector_path.as_bytes()).is_err()
            || self.selector_literal.is_empty()
            || self.selector_literal.len() > 4096
            || !valid_sha256(&self.requirement_revision)
            || !valid_sha256(&self.policy_fingerprint)
            || self.invocation_id.is_empty()
            || self.evaluator_ref.is_empty()
            || self.runtime_ref.is_empty()
            || self.enforcement_ref.is_empty()
            || self.invocation_descriptor_version != 1
            || !valid_sha256(&self.invocation_descriptor_sha256)
            || self.evaluator_executable.is_empty()
            || self.toolchain_ref.is_empty()
            || self.runner_backend.is_empty()
            || !valid_sha256(&self.runner_plan_sha256)
            || self.runner_enforced_axes & 0b011 != 0b011
            || self.runner_enforced_axes & !0b111 != 0
            || !self.runner_env_isolated
            || !valid_sha256(&self.base_manifest_sha256)
            || !valid_sha256(&self.candidate_manifest_sha256)
            || !valid_oid(&self.base_commit_oid)
            || !valid_oid(&self.base_tree_oid)
            || !valid_oid(&self.candidate_commit_oid)
            || !valid_oid(&self.candidate_tree_oid)
            || self
                .deletions
                .windows(2)
                .any(|pair| pair[0].path >= pair[1].path)
            || self.deletions.iter().any(|deletion| {
                validate_candidate_path(&deletion.path).is_err()
                    || (deletion.base_mode != 0o100644 && deletion.base_mode != 0o100755)
                    || !valid_sha256(&deletion.base_blob_sha256)
            })
            || self.invocation_descriptor_sha256 != self.canonical_invocation_descriptor_digest()
            || self.started_at_ms > self.completed_at_ms
            || unique_non_reuse_phases.len()
                != self
                    .checkpoints
                    .iter()
                    .filter(|checkpoint| checkpoint.phase != CandidateCheckpointPhase::RestartReuse)
                    .count()
            || self.checkpoints.iter().any(|checkpoint| {
                checkpoint.observed_at_ms < self.started_at_ms
                    || (!matches!(
                        checkpoint.phase,
                        CandidateCheckpointPhase::Completion
                            | CandidateCheckpointPhase::RestartReuse
                    ) && checkpoint.observed_at_ms > self.completed_at_ms)
                    || (matches!(
                        checkpoint.phase,
                        CandidateCheckpointPhase::Completion
                            | CandidateCheckpointPhase::RestartReuse
                    ) && checkpoint.observed_at_ms < self.completed_at_ms)
                    || !valid_sha256(&checkpoint.git_metadata_fingerprint)
                    || !valid_sha256(&checkpoint.live_worktree_manifest_sha256)
            })
        {
            return Err(CandidateError::InvalidAssessment);
        }
        for required in [
            CandidateCheckpointPhase::Selection,
            CandidateCheckpointPhase::BeforeEvaluation,
            CandidateCheckpointPhase::AfterEvaluation,
            CandidateCheckpointPhase::ResultAdmission,
        ] {
            if !phases.contains(&required) {
                return Err(CandidateError::InvalidAssessment);
            }
        }
        if self.outcome == CandidateIntegrityOutcome::Intact
            && self
                .checkpoints
                .iter()
                .any(|checkpoint| !checkpoint.matched)
        {
            return Err(CandidateError::InvalidAssessment);
        }
        if phases.contains(&CandidateCheckpointPhase::RestartReuse)
            && !phases.contains(&CandidateCheckpointPhase::Completion)
        {
            return Err(CandidateError::InvalidAssessment);
        }
        Ok(())
    }
}

pub(crate) fn valid_sha256(value: &str) -> bool {
    value.strip_prefix("sha256:").is_some_and(|hex| {
        hex.len() == 64
            && hex
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    })
}

fn valid_oid(value: &str) -> bool {
    matches!(value.len(), 40 | 64)
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CandidateSelector {
    pub criterion_id: String,
    pub selector: String,
    pub path: String,
    pub literal: String,
    pub requirement_revision: String,
}

impl CandidateSelector {
    /// Resolves only the one catalogued selector from the exact current,
    /// required TestSuite requirement. Process arguments never grant scope.
    pub fn from_requirement(
        requirement: &task_domain::EvidenceRequirement,
    ) -> Result<Self, CandidateError> {
        if requirement.requirement_kind != task_domain::RequirementKind::TestSuite
            || !requirement.required
            || requirement.subject_binding != task_domain::SubjectBinding::ContentToolchainAndConfig
            || requirement.policy_fingerprint.is_none()
            || requirement.params.len() != 3
            || requirement.params[0].key() != "literal"
            || requirement.params[1].key() != "path"
            || requirement.params[2].key() != "selector"
            || requirement.params[2].value() != "candidate.file-contains.v1"
        {
            return Err(CandidateError::InvalidRequirement);
        }
        let literal = requirement.params[0].value();
        let path = requirement.params[1].value();
        validate_candidate_path(path.as_bytes())?;
        if literal.len() > 4096 {
            return Err(CandidateError::InvalidRequirement);
        }
        let policy_fingerprint = requirement
            .policy_fingerprint
            .as_deref()
            .ok_or(CandidateError::InvalidRequirement)?;
        if !valid_sha256(policy_fingerprint) {
            return Err(CandidateError::InvalidRequirement);
        }
        Ok(Self {
            criterion_id: requirement.criterion_id.clone(),
            selector: requirement.params[2].value().to_owned(),
            path: path.to_owned(),
            literal: literal.to_owned(),
            requirement_revision: requirement_revision(requirement),
        })
    }

    pub fn matches_invocation(&self, selector: &str, path: &str, literal: &str) -> bool {
        self.selector == selector && self.path == path && self.literal == literal
    }
}

impl CandidateTestResult {
    /// The only initial selector: a bounded literal must occur in one exact
    /// regular-file blob from the verified candidate snapshot.
    pub fn file_contains(
        files: &[CandidateFile],
        path: &str,
        literal: &str,
    ) -> Result<Self, CandidateError> {
        if path.is_empty()
            || path.starts_with('/')
            || path.contains('\\')
            || path.split('/').any(|part| {
                part.is_empty() || part == "." || part == ".." || part.eq_ignore_ascii_case(".git")
            })
            || !path.is_ascii()
            || literal.is_empty()
            || literal.len() > 4096
        {
            return Err(CandidateError::UnsupportedPath);
        }
        let needle = literal.as_bytes();
        let passed = files
            .iter()
            .find(|file| file.path == path.as_bytes())
            .is_some_and(|file| {
                file.bytes
                    .windows(needle.len())
                    .any(|window| window == needle)
            });
        let result = format!(
            "candidate-test-result/v1\nselector=candidate.file-contains.v1\npath-sha256=sha256:{:x}\npassed={}\nfailed={}\nskipped=0\n",
            Sha256::digest(path.as_bytes()),
            u8::from(passed),
            u8::from(!passed),
        );
        Ok(Self {
            selector: "candidate.file-contains.v1".to_owned(),
            passed: u32::from(passed),
            failed: u32::from(!passed),
            skipped: 0,
            result_sha256: format!("sha256:{:x}", Sha256::digest(result.as_bytes())),
        })
    }

    pub fn encode(&self, manifest_sha256: &str) -> String {
        format!(
            "candidate-test-result/v1\nselector={}\nmanifest={}\npassed={}\nfailed={}\nskipped={}\nresult={}\n",
            self.selector,
            manifest_sha256,
            self.passed,
            self.failed,
            self.skipped,
            self.result_sha256,
        )
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CandidateError {
    InvalidObjectId,
    InvalidCommit,
    InvalidRequirement,
    ObjectHashMismatch,
    InvalidTree,
    UnsupportedPath,
    PathCollision,
    LimitExceeded,
    MissingObject,
    UnsupportedObject,
    UnsupportedObjectFormat,
    InvalidAssessment,
}

impl std::fmt::Display for CandidateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "candidate input error: {self:?}")
    }
}

impl std::error::Error for CandidateError {}

impl CandidateManifest {
    pub fn from_files(files: &[CandidateFile]) -> Result<Self, CandidateError> {
        if files.len() > MAX_CANDIDATE_ENTRIES {
            return Err(CandidateError::LimitExceeded);
        }
        let mut entries = Vec::with_capacity(files.len());
        let mut normalized = BTreeSet::new();
        let mut previous: Option<&[u8]> = None;
        let mut total = 0u64;
        for file in files {
            validate_candidate_path(&file.path)?;
            if file.mode != 0o100644 && file.mode != 0o100755 {
                return Err(CandidateError::UnsupportedObject);
            }
            if previous.is_some_and(|prior| prior >= file.path.as_slice()) {
                return Err(CandidateError::InvalidTree);
            }
            previous = Some(&file.path);
            let key = file
                .path
                .iter()
                .map(u8::to_ascii_lowercase)
                .collect::<Vec<_>>();
            if !normalized.insert(key) {
                return Err(CandidateError::PathCollision);
            }
            total = total
                .checked_add(file.bytes.len() as u64)
                .ok_or(CandidateError::LimitExceeded)?;
            if total > MAX_CANDIDATE_BLOB_BYTES {
                return Err(CandidateError::LimitExceeded);
            }
            entries.push(CandidateManifestEntry {
                path: file.path.clone(),
                mode: file.mode,
                length: file.bytes.len() as u64,
                sha256: format!("sha256:{:x}", Sha256::digest(&file.bytes)),
            });
        }
        let sha256 = manifest_digest(&entries);
        Ok(Self {
            version: 1,
            entries,
            sha256,
            total_bytes: total,
        })
    }

    /// Walks and independently verifies every raw Git object, then hashes the
    /// exact regular-file bytes into the versioned canonical SHA-256 manifest.
    pub(crate) fn from_tree(
        root_tree_oid: &str,
        format: GitObjectFormat,
        mut read_object: impl FnMut(&str) -> Result<(String, Vec<u8>), CandidateError>,
    ) -> Result<(Self, Vec<CandidateFile>), CandidateError> {
        if GitObjectFormat::from_oid(root_tree_oid)? != format {
            return Err(CandidateError::InvalidObjectId);
        }
        let mut files = BTreeMap::<Vec<u8>, CandidateFile>::new();
        let mut dirs = BTreeSet::<Vec<u8>>::new();
        let mut normalized_paths = BTreeSet::<Vec<u8>>::new();
        let mut total = 0u64;
        {
            let mut walker = TreeWalker {
                format,
                read_object: &mut read_object,
                files: &mut files,
                dirs: &mut dirs,
                normalized_paths: &mut normalized_paths,
                total: &mut total,
            };
            walker.walk(root_tree_oid, &[], 0)?;
        }
        let entries = files
            .values()
            .map(|file| CandidateManifestEntry {
                path: file.path.clone(),
                mode: file.mode,
                length: file.bytes.len() as u64,
                sha256: format!("sha256:{:x}", Sha256::digest(&file.bytes)),
            })
            .collect::<Vec<_>>();
        let sha256 = manifest_digest(&entries);
        Ok((
            Self {
                version: 1,
                entries,
                sha256,
                total_bytes: total,
            },
            files.into_values().collect(),
        ))
    }
}

impl VerifiedCandidate {
    /// Validates the selected commit object's Git framing and OID before
    /// resolving its root tree and every nested tree and blob.
    pub fn from_commit(
        commit_oid: &str,
        format: GitObjectFormat,
        mut read_object: impl FnMut(&str) -> Result<(String, Vec<u8>), CandidateError>,
    ) -> Result<Self, CandidateError> {
        if GitObjectFormat::from_oid(commit_oid)? != format {
            return Err(CandidateError::InvalidObjectId);
        }
        let (kind, commit) = read_object(commit_oid)?;
        verify_git_object(format, commit_oid, &kind, &commit)?;
        if kind != "commit" {
            return Err(CandidateError::UnsupportedObject);
        }
        let tree_oid = commit_tree_oid(&commit, format)?;
        let (manifest, files) = CandidateManifest::from_tree(&tree_oid, format, read_object)?;
        Ok(Self {
            commit_oid: commit_oid.to_owned(),
            tree_oid,
            manifest,
            files,
        })
    }

    /// Resolves a complete commit/tree/blob graph in bounded batches. This is
    /// the production Git-reader contract; the resolver must return exactly
    /// one framed object for every requested OID and no additional objects.
    pub fn from_commit_with_batch(
        commit_oid: &str,
        format: GitObjectFormat,
        mut read_objects: impl FnMut(
            &[String],
        ) -> Result<Vec<(String, String, Vec<u8>)>, CandidateError>,
    ) -> Result<Self, CandidateError> {
        if GitObjectFormat::from_oid(commit_oid)? != format {
            return Err(CandidateError::InvalidObjectId);
        }
        let mut objects = BTreeMap::<String, (String, Vec<u8>)>::new();
        fetch_verified_batch(
            &[commit_oid.to_owned()],
            format,
            "commit",
            &mut read_objects,
            &mut objects,
        )?;
        let commit = &objects
            .get(commit_oid)
            .ok_or(CandidateError::MissingObject)?
            .1;
        let tree_oid = commit_tree_oid(commit, format)?;

        let mut layer = vec![tree_oid.clone()];
        let mut blob_oids = BTreeSet::new();
        let mut tree_count = 0usize;
        let mut tree_bytes = 0u64;
        let mut depth = 0usize;
        while !layer.is_empty() {
            depth += 1;
            if depth > MAX_CANDIDATE_TREE_DEPTH {
                return Err(CandidateError::LimitExceeded);
            }
            let requested = layer
                .iter()
                .filter(|oid| !objects.contains_key(*oid))
                .cloned()
                .collect::<Vec<_>>();
            for chunk in requested.chunks(256) {
                fetch_verified_batch(chunk, format, "tree", &mut read_objects, &mut objects)?;
            }
            let mut next = BTreeSet::new();
            for oid in &layer {
                let (kind, bytes) = objects.get(oid).ok_or(CandidateError::MissingObject)?;
                if kind != "tree" {
                    return Err(CandidateError::UnsupportedObject);
                }
                tree_bytes = tree_bytes
                    .checked_add(bytes.len() as u64)
                    .ok_or(CandidateError::LimitExceeded)?;
                if tree_bytes > 64 * 1024 * 1024 {
                    return Err(CandidateError::LimitExceeded);
                }
                let (trees, blobs, count) = tree_child_oids(bytes, format)?;
                tree_count = tree_count
                    .checked_add(count)
                    .ok_or(CandidateError::LimitExceeded)?;
                if tree_count > MAX_CANDIDATE_ENTRIES {
                    return Err(CandidateError::LimitExceeded);
                }
                next.extend(trees);
                blob_oids.extend(blobs);
            }
            layer = next.into_iter().collect();
        }
        if blob_oids.len() + tree_count > MAX_CANDIDATE_ENTRIES {
            return Err(CandidateError::LimitExceeded);
        }
        let missing_blobs = blob_oids
            .into_iter()
            .filter(|oid| !objects.contains_key(oid))
            .collect::<Vec<_>>();
        let mut blob_bytes = 0u64;
        for chunk in missing_blobs.chunks(256) {
            fetch_verified_batch(chunk, format, "blob", &mut read_objects, &mut objects)?;
            for oid in chunk {
                let (_, bytes) = objects.get(oid).ok_or(CandidateError::MissingObject)?;
                blob_bytes = blob_bytes
                    .checked_add(bytes.len() as u64)
                    .ok_or(CandidateError::LimitExceeded)?;
                if blob_bytes > MAX_CANDIDATE_BLOB_BYTES {
                    return Err(CandidateError::LimitExceeded);
                }
            }
        }
        let (manifest, files) = CandidateManifest::from_tree(&tree_oid, format, |oid| {
            objects
                .get(oid)
                .cloned()
                .ok_or(CandidateError::MissingObject)
        })?;
        Ok(Self {
            commit_oid: commit_oid.to_owned(),
            tree_oid,
            manifest,
            files,
        })
    }
}

fn fetch_verified_batch(
    requested: &[String],
    format: GitObjectFormat,
    expected_kind: &str,
    read_objects: &mut impl FnMut(&[String]) -> Result<Vec<(String, String, Vec<u8>)>, CandidateError>,
    objects: &mut BTreeMap<String, (String, Vec<u8>)>,
) -> Result<(), CandidateError> {
    if requested.is_empty() {
        return Ok(());
    }
    let response = read_objects(requested)?;
    if response.len() != requested.len() {
        return Err(CandidateError::MissingObject);
    }
    let requested_set = requested.iter().collect::<BTreeSet<_>>();
    for (oid, kind, bytes) in response {
        if !requested_set.contains(&oid) || kind != expected_kind {
            return Err(CandidateError::UnsupportedObject);
        }
        verify_git_object(format, &oid, &kind, &bytes)?;
        if objects.insert(oid, (kind, bytes)).is_some() {
            return Err(CandidateError::InvalidTree);
        }
    }
    Ok(())
}

fn commit_tree_oid(commit: &[u8], format: GitObjectFormat) -> Result<String, CandidateError> {
    let header_end = commit
        .windows(2)
        .position(|window| window == b"\n\n")
        .ok_or(CandidateError::InvalidCommit)?;
    let mut tree_oid = None;
    for (index, line) in commit[..header_end]
        .split(|byte| *byte == b'\n')
        .enumerate()
    {
        if let Some(value) = line.strip_prefix(b"tree ") {
            if tree_oid.is_some() || index != 0 {
                return Err(CandidateError::InvalidCommit);
            }
            let oid = std::str::from_utf8(value).map_err(|_| CandidateError::InvalidCommit)?;
            if GitObjectFormat::from_oid(oid)? != format {
                return Err(CandidateError::InvalidObjectId);
            }
            tree_oid = Some(oid.to_owned());
        } else if let Some(value) = line.strip_prefix(b"parent ") {
            let oid = std::str::from_utf8(value).map_err(|_| CandidateError::InvalidCommit)?;
            if GitObjectFormat::from_oid(oid)? != format {
                return Err(CandidateError::InvalidObjectId);
            }
        } else if line.is_empty() {
            return Err(CandidateError::InvalidCommit);
        }
    }
    tree_oid.ok_or(CandidateError::InvalidCommit)
}

fn tree_child_oids(
    bytes: &[u8],
    format: GitObjectFormat,
) -> Result<(BTreeSet<String>, BTreeSet<String>, usize), CandidateError> {
    let mut trees = BTreeSet::new();
    let mut blobs = BTreeSet::new();
    let mut count = 0usize;
    let mut rest = bytes;
    while !rest.is_empty() {
        let space = rest
            .iter()
            .position(|byte| *byte == b' ')
            .ok_or(CandidateError::InvalidTree)?;
        let mode = parse_mode(&rest[..space])?;
        rest = &rest[space + 1..];
        let nul = rest
            .iter()
            .position(|byte| *byte == 0)
            .ok_or(CandidateError::InvalidTree)?;
        let name = &rest[..nul];
        if name.is_empty() || name.contains(&b'/') || name == b"." || name == b".." {
            return Err(CandidateError::UnsupportedPath);
        }
        let oid_start = nul + 1;
        let oid_end = oid_start
            .checked_add(format.oid_len())
            .ok_or(CandidateError::InvalidTree)?;
        if rest.len() < oid_end {
            return Err(CandidateError::InvalidTree);
        }
        let oid = hex_encode(&rest[oid_start..oid_end]);
        if mode == 0o40000 {
            trees.insert(oid);
        } else {
            blobs.insert(oid);
        }
        count += 1;
        rest = &rest[oid_end..];
    }
    Ok((trees, blobs, count))
}

struct TreeWalker<'a, F> {
    format: GitObjectFormat,
    read_object: &'a mut F,
    files: &'a mut BTreeMap<Vec<u8>, CandidateFile>,
    dirs: &'a mut BTreeSet<Vec<u8>>,
    normalized_paths: &'a mut BTreeSet<Vec<u8>>,
    total: &'a mut u64,
}

impl<F> TreeWalker<'_, F>
where
    F: FnMut(&str) -> Result<(String, Vec<u8>), CandidateError>,
{
    fn walk(&mut self, oid: &str, prefix: &[u8], depth: usize) -> Result<(), CandidateError> {
        if depth > MAX_CANDIDATE_TREE_DEPTH {
            return Err(CandidateError::LimitExceeded);
        }
        let (kind, bytes) = (self.read_object)(oid)?;
        verify_git_object(self.format, oid, &kind, &bytes)?;
        if kind != "tree" {
            return Err(CandidateError::UnsupportedObject);
        }
        let mut rest = bytes.as_slice();
        let mut previous_sort_key: Option<Vec<u8>> = None;
        while !rest.is_empty() {
            let space = rest
                .iter()
                .position(|byte| *byte == b' ')
                .ok_or(CandidateError::InvalidTree)?;
            let mode = parse_mode(&rest[..space])?;
            rest = &rest[space + 1..];
            let nul = rest
                .iter()
                .position(|byte| *byte == 0)
                .ok_or(CandidateError::InvalidTree)?;
            let name = &rest[..nul];
            if name.is_empty() || name.contains(&b'/') || name == b"." || name == b".." {
                return Err(CandidateError::UnsupportedPath);
            }
            let oid_start = nul + 1;
            let oid_end = oid_start
                .checked_add(self.format.oid_len())
                .ok_or(CandidateError::InvalidTree)?;
            if rest.len() < oid_end {
                return Err(CandidateError::InvalidTree);
            }
            let child_oid = hex_encode(&rest[oid_start..oid_end]);
            rest = &rest[oid_end..];
            if !name.is_ascii() || name.iter().any(|b| *b == b'\\' || *b < 0x20 || *b == 0x7f) {
                return Err(CandidateError::UnsupportedPath);
            }
            let mut sort_key = name.to_vec();
            sort_key.push(if mode == 0o40000 { b'/' } else { 0 });
            if previous_sort_key
                .as_ref()
                .is_some_and(|previous| previous >= &sort_key)
            {
                return Err(CandidateError::InvalidTree);
            }
            previous_sort_key = Some(sort_key);
            let mut path = prefix.to_vec();
            if !path.is_empty() {
                path.push(b'/');
            }
            path.extend_from_slice(name);
            if path.len() > 4096 || name.eq_ignore_ascii_case(b".git") {
                return Err(CandidateError::UnsupportedPath);
            }
            let collision_key = path.iter().map(u8::to_ascii_lowercase).collect::<Vec<_>>();
            if !self.normalized_paths.insert(collision_key) {
                return Err(CandidateError::PathCollision);
            }
            if mode == 0o40000 {
                if self.files.contains_key(&path) || self.dirs.contains(&path) {
                    return Err(CandidateError::PathCollision);
                }
                self.dirs.insert(path.clone());
                self.walk(&child_oid, &path, depth + 1)?;
            } else {
                if self.files.contains_key(&path) || self.dirs.contains(&path) {
                    return Err(CandidateError::PathCollision);
                }
                let (kind, blob) = (self.read_object)(&child_oid)?;
                verify_git_object(self.format, &child_oid, &kind, &blob)?;
                if kind != "blob" {
                    return Err(CandidateError::UnsupportedObject);
                }
                *self.total = self
                    .total
                    .checked_add(blob.len() as u64)
                    .ok_or(CandidateError::LimitExceeded)?;
                if *self.total > MAX_CANDIDATE_BLOB_BYTES {
                    return Err(CandidateError::LimitExceeded);
                }
                self.files.insert(
                    path.clone(),
                    CandidateFile {
                        path,
                        mode,
                        git_oid: Some(child_oid),
                        bytes: blob,
                    },
                );
            }
            if self.files.len() + self.dirs.len() > MAX_CANDIDATE_ENTRIES {
                return Err(CandidateError::LimitExceeded);
            }
        }
        Ok(())
    }
}

fn parse_mode(raw: &[u8]) -> Result<u32, CandidateError> {
    match raw {
        b"100644" => Ok(0o100644),
        b"100755" => Ok(0o100755),
        b"40000" => Ok(0o40000),
        _ => Err(CandidateError::UnsupportedObject),
    }
}

fn validate_candidate_path(path: &[u8]) -> Result<(), CandidateError> {
    if path.is_empty() || path.len() > 4096 || !path.is_ascii() {
        return Err(CandidateError::UnsupportedPath);
    }
    for component in path.split(|byte| *byte == b'/') {
        if component.is_empty()
            || component == b"."
            || component == b".."
            || component.eq_ignore_ascii_case(b".git")
            || component
                .iter()
                .any(|byte| *byte == b'\\' || *byte < 0x20 || *byte == 0x7f)
        {
            return Err(CandidateError::UnsupportedPath);
        }
    }
    Ok(())
}

pub fn verify_git_object(
    format: GitObjectFormat,
    oid: &str,
    kind: &str,
    bytes: &[u8],
) -> Result<(), CandidateError> {
    if GitObjectFormat::from_oid(oid)? != format || !matches!(kind, "commit" | "tree" | "blob") {
        return Err(CandidateError::InvalidObjectId);
    }
    let header = format!("{kind} {}\0", bytes.len());
    let matches = match format {
        GitObjectFormat::Sha1 => {
            use sha1::{Digest as _, Sha1};
            let mut hash = Sha1::new();
            hash.update(header.as_bytes());
            hash.update(bytes);
            hex_encode(&hash.finalize()) == oid
        }
        GitObjectFormat::Sha256 => {
            let mut hash = Sha256::new();
            hash.update(header.as_bytes());
            hash.update(bytes);
            format!("{:x}", hash.finalize()) == oid
        }
    };
    matches
        .then_some(())
        .ok_or(CandidateError::ObjectHashMismatch)
}

fn manifest_digest(entries: &[CandidateManifestEntry]) -> String {
    let mut hash = Sha256::new();
    hash.update(b"runlith:candidate-manifest:v1\0");
    for entry in entries {
        field(&mut hash, &entry.path);
        hash.update(entry.mode.to_be_bytes());
        field(&mut hash, b"blob");
        hash.update(entry.length.to_be_bytes());
        field(&mut hash, entry.sha256.as_bytes());
    }
    format!("sha256:{:x}", hash.finalize())
}

fn field(hash: &mut Sha256, bytes: &[u8]) {
    hash.update((bytes.len() as u64).to_be_bytes());
    hash.update(bytes);
}

/// Canonical requirement revision (ADR-0032 rule 4): domain-separated
/// `sha256:` hash over criterion id, kind, subject binding, policy
/// fingerprint (empty when absent), required flag and sorted params.
/// `CandidateSelector::from_requirement` uses this exact function, so the
/// candidate revision equals the generic revision for candidate-shaped
/// requirements; other shapes get a stable revision without a selector.
#[must_use]
pub fn requirement_revision(requirement: &task_domain::EvidenceRequirement) -> String {
    let mut revision = Sha256::new();
    revision.update(b"runlith:evidence-requirement:v1\0");
    field(&mut revision, requirement.criterion_id.as_bytes());
    field(
        &mut revision,
        requirement.requirement_kind.as_str().as_bytes(),
    );
    field(
        &mut revision,
        requirement.subject_binding.as_str().as_bytes(),
    );
    field(
        &mut revision,
        requirement
            .policy_fingerprint
            .as_deref()
            .unwrap_or("")
            .as_bytes(),
    );
    revision.update([u8::from(requirement.required)]);
    for param in &requirement.params {
        field(&mut revision, param.key().as_bytes());
        field(&mut revision, param.value().as_bytes());
    }
    format!("sha256:{:x}", revision.finalize())
}

fn lowercase_hex(value: &str) -> bool {
    value
        .bytes()
        .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn committed_deletions_bind_raw_path_mode_and_base_blob_digest() {
        let digest = candidate_sha256(b"base bytes");
        let base = CandidateManifest {
            version: 1,
            entries: vec![
                CandidateManifestEntry {
                    path: b"keep.txt".to_vec(),
                    mode: 0o100644,
                    length: 4,
                    sha256: candidate_sha256(b"keep"),
                },
                CandidateManifestEntry {
                    path: b"raw-\xff-name".to_vec(),
                    mode: 0o100755,
                    length: 10,
                    sha256: digest.clone(),
                },
            ],
            sha256: candidate_sha256(b"base manifest"),
            total_bytes: 14,
        };
        let candidate = CandidateManifest {
            version: 1,
            entries: vec![base.entries[0].clone()],
            sha256: candidate_sha256(b"candidate manifest"),
            total_bytes: 4,
        };

        assert_eq!(
            CandidateDeletion::between(&base, &candidate),
            vec![CandidateDeletion {
                path: b"raw-\xff-name".to_vec(),
                base_mode: 0o100755,
                base_blob_sha256: digest,
            }]
        );
    }
    use std::collections::HashMap;

    fn object(kind: &str, bytes: &[u8]) -> (String, String, Vec<u8>) {
        let framed = [format!("{kind} {}\0", bytes.len()).as_bytes(), bytes].concat();
        use sha1::{Digest as _, Sha1};
        let oid = hex_encode(&Sha1::digest(&framed));
        (oid, kind.to_owned(), bytes.to_vec())
    }

    #[test]
    fn candidate_manifest_uses_verified_raw_blob_bytes_and_rejects_bad_oids() {
        let (blob_oid, blob_kind, blob) = object("blob", b"needle in candidate");
        let mut tree = b"100644 file.txt\0".to_vec();
        tree.extend_from_slice(&hex_decode(&blob_oid));
        let (tree_oid, tree_kind, tree_bytes) = object("tree", &tree);
        let objects = HashMap::from([
            (blob_oid.clone(), (blob_kind, blob)),
            (tree_oid.clone(), (tree_kind, tree_bytes)),
        ]);
        let (manifest, files) =
            CandidateManifest::from_tree(&tree_oid, GitObjectFormat::Sha1, |oid| {
                objects
                    .get(oid)
                    .cloned()
                    .ok_or(CandidateError::MissingObject)
            })
            .unwrap();
        assert_eq!(files[0].path, b"file.txt");
        assert_eq!(files[0].bytes, b"needle in candidate");
        assert_eq!(manifest.entries[0].length, 19);
        assert!(manifest.sha256.starts_with("sha256:"));
        assert_eq!(
            CandidateManifest::from_tree("not-an-oid", GitObjectFormat::Sha1, |_| {
                unreachable!()
            }),
            Err(CandidateError::InvalidObjectId)
        );
    }

    #[test]
    fn candidate_tree_rejects_case_collisions_and_symlink_modes() {
        let (first_oid, kind, first) = object("blob", b"A");
        let (second_oid, _, second) = object("blob", b"B");
        let mut tree = Vec::new();
        for (name, oid) in [
            (b"README".as_slice(), &second_oid),
            (b"Readme".as_slice(), &first_oid),
        ] {
            tree.extend_from_slice(b"100644 ");
            tree.extend_from_slice(name);
            tree.push(0);
            tree.extend_from_slice(&hex_decode(oid));
        }
        let (tree_oid, _, tree) = object("tree", &tree);
        let objects = HashMap::from([
            (first_oid, (kind.clone(), first)),
            (second_oid, (kind, second)),
            (tree_oid.clone(), ("tree".to_owned(), tree)),
        ]);
        assert_eq!(
            CandidateManifest::from_tree(&tree_oid, GitObjectFormat::Sha1, |oid| {
                objects
                    .get(oid)
                    .cloned()
                    .ok_or(CandidateError::MissingObject)
            }),
            Err(CandidateError::PathCollision)
        );
        assert_eq!(
            parse_mode(b"120000"),
            Err(CandidateError::UnsupportedObject)
        );
    }

    #[test]
    fn candidate_selector_reads_only_the_exact_manifest_bound_file() {
        let files = vec![CandidateFile {
            path: b"src/example.rs".to_vec(),
            mode: 0o100644,
            git_oid: None,
            bytes: b"fn exact_candidate() {}".to_vec(),
        }];
        let pass = CandidateTestResult::file_contains(&files, "src/example.rs", "exact_candidate")
            .unwrap();
        let fail =
            CandidateTestResult::file_contains(&files, "src/other.rs", "exact_candidate").unwrap();
        assert_eq!((pass.passed, pass.failed), (1, 0));
        assert_eq!((fail.passed, fail.failed), (0, 1));
        assert!(CandidateTestResult::file_contains(&files, "../outside", "x").is_err());
    }

    #[test]
    fn candidate_selector_requires_exact_current_requirement_parameters() {
        use task_domain::{EvidenceRequirement, RequirementKind, RequirementParam, SubjectBinding};

        let requirement = EvidenceRequirement::new(
            "criterion-a",
            RequirementKind::TestSuite,
            SubjectBinding::ContentToolchainAndConfig,
            vec![
                RequirementParam::new("selector", "candidate.file-contains.v1").unwrap(),
                RequirementParam::new("path", "src/example.rs").unwrap(),
                RequirementParam::new("literal", "accepted_code").unwrap(),
            ],
            Some(format!("sha256:{}", "a".repeat(64))),
            true,
        )
        .unwrap();
        let selector = CandidateSelector::from_requirement(&requirement).unwrap();
        assert!(selector.matches_invocation(
            "candidate.file-contains.v1",
            "src/example.rs",
            "accepted_code"
        ));
        assert!(!selector.matches_invocation(
            "candidate.file-contains.v1",
            "src/other.rs",
            "accepted_code"
        ));

        let missing = EvidenceRequirement::new(
            "criterion-a",
            RequirementKind::TestSuite,
            SubjectBinding::Content,
            vec![],
            Some("sha256:current-evidence-policy".to_owned()),
            true,
        )
        .unwrap();
        assert_eq!(
            CandidateSelector::from_requirement(&missing),
            Err(CandidateError::InvalidRequirement)
        );

        let extra = EvidenceRequirement::new(
            "criterion-a",
            RequirementKind::TestSuite,
            SubjectBinding::Content,
            vec![
                RequirementParam::new("selector", "candidate.file-contains.v1").unwrap(),
                RequirementParam::new("path", "src/example.rs").unwrap(),
                RequirementParam::new("literal", "accepted_code").unwrap(),
                RequirementParam::new("unreviewed", "value").unwrap(),
            ],
            Some("sha256:current-evidence-policy".to_owned()),
            true,
        )
        .unwrap();
        assert_eq!(
            CandidateSelector::from_requirement(&extra),
            Err(CandidateError::InvalidRequirement)
        );
    }

    #[test]
    fn verified_candidate_checks_commit_tree_and_blob_oid_framing() {
        let format = GitObjectFormat::Sha256;
        let (blob_oid, blob) = sha256_object("blob", b"commit-bound bytes");
        let mut tree = b"100644 file.txt\0".to_vec();
        tree.extend_from_slice(&hex_decode(&blob_oid));
        let (tree_oid, tree) = sha256_object("tree", &tree);
        let commit = format!(
            "tree {tree_oid}\nauthor A <a@b> 1 +0000\ncommitter A <a@b> 1 +0000\n\nsubject\n"
        );
        let (commit_oid, commit) = sha256_object("commit", commit.as_bytes());
        let objects = HashMap::from([
            (blob_oid.clone(), ("blob".to_owned(), blob)),
            (tree_oid.clone(), ("tree".to_owned(), tree)),
            (commit_oid.clone(), ("commit".to_owned(), commit)),
        ]);
        let candidate = VerifiedCandidate::from_commit(&commit_oid, format, |oid| {
            objects
                .get(oid)
                .cloned()
                .ok_or(CandidateError::MissingObject)
        })
        .unwrap();
        assert_eq!(candidate.tree_oid, tree_oid);
        assert_eq!(candidate.files[0].bytes, b"commit-bound bytes");

        let batched = VerifiedCandidate::from_commit_with_batch(&commit_oid, format, |oids| {
            oids.iter()
                .map(|oid| {
                    let (kind, bytes) = objects
                        .get(oid)
                        .cloned()
                        .ok_or(CandidateError::MissingObject)?;
                    Ok((oid.clone(), kind, bytes))
                })
                .collect()
        })
        .unwrap();
        assert_eq!(batched, candidate);

        let mut bad_objects = objects;
        bad_objects.insert(
            commit_oid.clone(),
            ("commit".to_owned(), b"changed".to_vec()),
        );
        assert_eq!(
            VerifiedCandidate::from_commit(&commit_oid, format, |oid| {
                bad_objects
                    .get(oid)
                    .cloned()
                    .ok_or(CandidateError::MissingObject)
            }),
            Err(CandidateError::ObjectHashMismatch)
        );
        assert_eq!(
            GitObjectFormat::from_repository_format("sha1"),
            Ok(GitObjectFormat::Sha1)
        );
        assert_eq!(
            GitObjectFormat::from_repository_format("unknown"),
            Err(CandidateError::UnsupportedObjectFormat)
        );
    }

    fn hex_decode(value: &str) -> Vec<u8> {
        value
            .as_bytes()
            .chunks_exact(2)
            .map(|pair| {
                let digit = |byte: u8| match byte {
                    b'0'..=b'9' => byte - b'0',
                    b'a'..=b'f' => byte - b'a' + 10,
                    _ => unreachable!(),
                };
                digit(pair[0]) << 4 | digit(pair[1])
            })
            .collect()
    }

    fn sha256_object(kind: &str, bytes: &[u8]) -> (String, Vec<u8>) {
        let framed = [format!("{kind} {}\0", bytes.len()).as_bytes(), bytes].concat();
        let oid = format!("{:x}", Sha256::digest(&framed));
        (oid, bytes.to_vec())
    }
}
