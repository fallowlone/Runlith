//! Canonical product-version identity (TASK-0029).
//!
//! Single source of truth: `product/version.toml`. Both binaries report
//! this identity as their IPC `build_version` through the helpers below,
//! so no second editable version string exists anywhere.
//!
//! Zero external dependencies by design — including no TOML parser: the
//! file has exactly four `key = "value"` lines, parsed deterministically
//! here and pinned by tests. Public SemVer (`semver`) moves only on
//! explicit release decisions; `development` advances once per completed
//! task (`<semver>-dev.taskNNNN`).

/// The canonical version file, embedded at compile time. The checked-in
/// file is the authority; this embedding only reads it.
const VERSION_TOML: &str = include_str!("../../../product/version.toml");

/// Canonical product-version identity: the four fields of
/// `product/version.toml`, borrowed from the embedded file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProductVersion {
    /// Public release line, e.g. `0.1.0`. Moves on explicit release
    /// decisions only — never per task.
    pub semver: &'static str,
    /// Per-task development identity, e.g. `0.1.0-dev.task0029`.
    /// Advances once per successfully completed task.
    pub development: &'static str,
    /// Canonical task responsible for the current repository state.
    pub task: &'static str,
    /// Current development gate, e.g. `G2`.
    pub gate: &'static str,
}

/// Parsing failure for the canonical version file. Construction-only:
/// the file is checked in beside its tests, so any failure is a
/// repository defect, surfaced loudly rather than defaulted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VersionError(String);

impl std::fmt::Display for VersionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "invalid product/version.toml: {}", self.0)
    }
}

impl std::error::Error for VersionError {}

/// Reads the canonical product version. Panics only when the checked-in
/// file itself is malformed (a repository defect the test suite pins);
/// normal callers receive the single version identity.
#[must_use]
pub fn product_version() -> ProductVersion {
    parse_version(VERSION_TOML).expect("product/version.toml must stay valid")
}

/// Current development identity for `build_version` fields, e.g.
/// `0.1.0-dev.task0029`. Derived, never edited: the canonical source
/// remains `product/version.toml`.
#[must_use]
pub fn development_version() -> &'static str {
    product_version().development
}

/// Parses one version file. Accepts `#` comments and blank lines;
/// requires exactly the four known keys, each once, with double-quoted
/// values. Anything else fails closed.
fn parse_version(input: &'static str) -> Result<ProductVersion, VersionError> {
    let mut semver = None;
    let mut development = None;
    let mut task = None;
    let mut gate = None;
    for (index, raw_line) in input.lines().enumerate() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (key, value) = split_assignment(line)
            .ok_or_else(|| VersionError(format!("line {}: expected key = \"value\"", index + 1)))?;
        let slot = match key {
            "semver" => &mut semver,
            "development" => &mut development,
            "task" => &mut task,
            "gate" => &mut gate,
            other => {
                return Err(VersionError(format!(
                    "line {}: unknown key {other:?}",
                    index + 1
                )));
            }
        };
        if slot.is_some() {
            return Err(VersionError(format!(
                "line {}: duplicate key {key:?}",
                index + 1
            )));
        }
        if value.is_empty() {
            return Err(VersionError(format!(
                "line {}: empty value for {key:?}",
                index + 1
            )));
        }
        *slot = Some(value);
    }
    let version = ProductVersion {
        semver: semver.ok_or_else(|| VersionError("missing semver".to_owned()))?,
        development: development.ok_or_else(|| VersionError("missing development".to_owned()))?,
        task: task.ok_or_else(|| VersionError("missing task".to_owned()))?,
        gate: gate.ok_or_else(|| VersionError("missing gate".to_owned()))?,
    };
    validate_consistency(&version)?;
    Ok(version)
}

/// Splits one `key = "value"` line. Values must be double-quoted with no
/// trailing content; quotes inside values are rejected rather than
/// escaped — version tokens never need them.
fn split_assignment(line: &'static str) -> Option<(&'static str, &'static str)> {
    let (key, rest) = line.split_once('=')?;
    let key = key.trim();
    let value = rest.trim();
    if key.is_empty() || !is_key(key) {
        return None;
    }
    let inner = value.strip_prefix('"')?.strip_suffix('"')?;
    if inner.contains('"') {
        return None;
    }
    // `key`, the quotes, and `inner` all borrow from the input line,
    // which outlives the call for the embedded file.
    Some((key, inner))
}

fn is_key(key: &str) -> bool {
    !key.is_empty()
        && key
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

/// Enforces the versioning policy: `development` is exactly
/// `<semver>-dev.taskNNNN` (four zero-padded digits), `task` is exactly
/// the `TASK-NNNN` suffix, and `gate` is a `G<digits>` planning gate.
fn validate_consistency(version: &ProductVersion) -> Result<(), VersionError> {
    let suffix = version
        .development
        .strip_prefix(version.semver)
        .and_then(|rest| rest.strip_prefix("-dev.task"))
        .ok_or_else(|| {
            VersionError(format!(
                "development {:?} must be <semver>-dev.taskNNNN",
                version.development
            ))
        })?;
    if suffix.len() != 4 || !suffix.bytes().all(|b| b.is_ascii_digit()) {
        return Err(VersionError(format!(
            "development {:?} needs a four-digit zero-padded task number",
            version.development
        )));
    }
    if version.task != format!("TASK-{suffix}") {
        return Err(VersionError(format!(
            "task {:?} must match the development suffix TASK-{suffix}",
            version.task
        )));
    }
    let gate_number = version
        .gate
        .strip_prefix('G')
        .ok_or_else(|| VersionError(format!("gate {:?} must look like G2", version.gate)))?;
    if gate_number.is_empty() || !gate_number.bytes().all(|b| b.is_ascii_digit()) {
        return Err(VersionError(format!(
            "gate {:?} must look like G2",
            version.gate
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_file_is_self_consistent() {
        // Timeless by design: this test validates the *relationship*
        // between the canonical fields (semver / development / task) and
        // the planning gate, never one historical task number. Rolling the
        // version to TASK-0032, TASK-0100, ... must not require editing
        // this test — only product/version.toml moves.
        let version = product_version();
        let suffix = version
            .development
            .strip_prefix(version.semver)
            .and_then(|rest| rest.strip_prefix("-dev.task"))
            .expect("development must be <semver>-dev.taskNNNN");
        assert_eq!(suffix.len(), 4, "task suffix must be four digits");
        assert!(
            suffix.bytes().all(|b| b.is_ascii_digit()),
            "task suffix must be numeric"
        );
        assert_eq!(
            version.task,
            format!("TASK-{suffix}"),
            "task must match the development suffix"
        );
        assert_eq!(
            development_version(),
            version.development,
            "runtime identity must come from the canonical file"
        );
    }

    #[test]
    fn gate_matches_planning_queue() {
        // The version gate tracks the planning queue's current gate: both
        // advance together per the versioning rule, never independently.
        const QUEUE: &str = include_str!("../../../planning/tasks/queue.yaml");
        let current = QUEUE
            .lines()
            .filter_map(|line| line.strip_prefix("current_gate:"))
            .map(str::trim)
            .next()
            .expect("queue must declare current_gate");
        assert_eq!(product_version().gate, current);
    }

    #[test]
    fn future_task_example_validates() {
        let version = parse_version(
            "semver = \"0.1.0\"\ndevelopment = \"0.1.0-dev.task0042\"\ntask = \"TASK-0042\"\ngate = \"G4\"\n",
        )
        .expect("documented future shape must validate");
        assert_eq!(version.task, "TASK-0042");
    }

    #[test]
    fn malformed_inputs_fail_closed() {
        for input in [
            "",
            "semver = \"0.1.0\"\n",
            "semver = \"0.1.0\"\ndevelopment = \"0.1.0-dev.task0029\"\ntask = \"TASK-0029\"\ngate = \"G2\"\nextra = \"1\"\n",
            "semver = \"0.1.0\"\ndevelopment = \"0.1.0-dev.task0029\"\ntask = \"TASK-0029\"\ngate = \"G2\"\ngate = \"G3\"\n",
            "semver = \"0.1.0\"\ndevelopment = \"0.2.0-dev.task0029\"\ntask = \"TASK-0029\"\ngate = \"G2\"\n",
            "semver = \"0.1.0\"\ndevelopment = \"0.1.0-dev.task29\"\ntask = \"TASK-29\"\ngate = \"G2\"\n",
            "semver = \"0.1.0\"\ndevelopment = \"0.1.0-dev.task0030\"\ntask = \"TASK-0029\"\ngate = \"G2\"\n",
            "semver = \"0.1.0\"\ndevelopment = \"0.1.0-dev.task0029\"\ntask = \"TASK-0029\"\ngate = \"G-2\"\n",
            "semver = 0.1.0\ndevelopment = \"0.1.0-dev.task0029\"\ntask = \"TASK-0029\"\ngate = \"G2\"\n",
            "semver = \"\"\ndevelopment = \"0.1.0-dev.task0029\"\ntask = \"TASK-0029\"\ngate = \"G2\"\n",
        ] {
            // Each input is a separate static fixture; leak to gain the
            // required lifetime without a TOML dependency.
            let leaked: &'static str = Box::leak(input.to_owned().into_boxed_str());
            assert!(parse_version(leaked).is_err(), "must reject {input:?}");
        }
    }
}
