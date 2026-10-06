//! Verification-level selection (TASK-0134, audit capabilities
//! V1–V3): changed-path classification drives V0..V3, observed effects
//! provide the write-observation fallback with no-op detection, and the
//! ecosystem derives verification commands as data. Pure selection only:
//! this module never executes commands and never changes the TASK-0125
//! completion contract — the [`VerificationPlan`] is an input to the
//! orchestrator's existing verify flow.
//!
//! Vocabulary (adapted from the agent-router audit: lockfiles/config/
//! schema imply full verification, docs-only implies none, interface
//! files imply a check-only pass, everything else is standard):
//! [`VerificationLevel::V0`] (no-op), [`VerificationLevel::V1`]
//! (interface check), [`VerificationLevel::V2`] (standard),
//! [`VerificationLevel::V3`] (full). The level selects a
//! [`VerificationPlan`] (scope + derived ecosystem commands as data)
//! and maps to [`task_domain::RequirementKind`] obligations carried by
//! the existing TASK-0123 model. This module never executes commands.

use evaluation::ObservedEffect;

/// Verification depth, cheapest first. Ordering is significant: the
/// highest class across inputs wins.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum VerificationLevel {
    /// Nothing to verify (docs-only or empty scope).
    V0,
    /// Interface check (contracts, declarations, headers).
    V1,
    /// Standard verification (code changes).
    V2,
    /// Full verification (lockfiles, manifests, schema).
    V3,
}

impl VerificationLevel {
    /// Stable machine name.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::V0 => "V0",
            Self::V1 => "V1",
            Self::V2 => "V2",
            Self::V3 => "V3",
        }
    }
}

/// Advisory risk input to level selection. Elevated risk raises the
/// floor to [`VerificationLevel::V1`] — a sanity check is never fully
/// skipped — but never lowers a selected level.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RiskTier {
    /// Default selection.
    Standard,
    /// Suspicious change: minimum V1.
    Elevated,
}

/// Package ecosystem derived from manifest/lockfile signals.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Ecosystem {
    /// `Cargo.toml` / `Cargo.lock`.
    Cargo,
    /// `package.json` / `package-lock.json`.
    Npm,
    /// `pnpm-lock.yaml`.
    Pnpm,
    /// `yarn.lock`.
    Yarn,
    /// `bun.lockb`.
    Bun,
    /// `go.mod` / `go.sum`.
    Go,
    /// `pyproject.toml` / `setup.py` / `setup.cfg`.
    Python,
    /// No ecosystem signal; commands are never fabricated.
    Unknown,
}

impl Ecosystem {
    /// Stable machine name.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Cargo => "cargo",
            Self::Npm => "npm",
            Self::Pnpm => "pnpm",
            Self::Yarn => "yarn",
            Self::Bun => "bun",
            Self::Go => "go",
            Self::Python => "python",
            Self::Unknown => "unknown",
        }
    }
}

/// One derived verification command (data, never executed here).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerificationCommand {
    /// Program to run.
    pub program: String,
    /// Arguments in order.
    pub args: Vec<String>,
}

/// Selected verification plan: level, ecosystem, scope, and the
/// derived commands for that pair.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerificationPlan {
    /// Selected depth.
    pub level: VerificationLevel,
    /// Detected ecosystem.
    pub ecosystem: Ecosystem,
    /// Scope paths in input order (changed paths, or effect subjects
    /// when no path list is available).
    pub scope: Vec<String>,
    /// Commands derived for (ecosystem, level); empty for V0 and for
    /// unknown ecosystems.
    pub commands: Vec<VerificationCommand>,
}

/// Exact lockfile names implying [`VerificationLevel::V3`].
const LOCKFILES: &[&str] = &[
    "cargo.lock",
    "package-lock.json",
    "yarn.lock",
    "pnpm-lock.yaml",
    "bun.lockb",
    "go.sum",
    "poetry.lock",
    "uv.lock",
    "gemfile.lock",
    "pipfile.lock",
];

/// Build manifest names implying [`VerificationLevel::V3`].
const MANIFESTS: &[&str] = &[
    "cargo.toml",
    "package.json",
    "go.mod",
    "pyproject.toml",
    "setup.py",
    "setup.cfg",
    "gemfile",
    "pipfile",
];

/// Documentation extensions (V0 when nothing else is present).
const DOC_EXTENSIONS: &[&str] = &["md", "mdx", "rst", "adoc"];

/// Documentation file names without extensions.
const DOC_NAMES: &[&str] = &["license", "licence", "notice", "changelog"];

/// Interface/contract extensions implying [`VerificationLevel::V1`].
const INTERFACE_EXTENSIONS: &[&str] = &["d.ts", "proto", "h", "hpp"];

/// Basename of a repo-relative path, lowercased by the caller contract.
fn basename(lower: &str) -> &str {
    lower.rsplit('/').next().unwrap_or(lower)
}

/// Classifies one path; the caller takes the maximum across inputs.
fn classify_path(path: &str) -> VerificationLevel {
    let lower = path.to_ascii_lowercase();
    let file = basename(&lower);
    if LOCKFILES.contains(&file) || MANIFESTS.contains(&file) {
        return VerificationLevel::V3;
    }
    if lower.contains("schema")
        || file.ends_with(".sql")
        || lower.split('/').any(|segment| segment == "migrations")
    {
        return VerificationLevel::V3;
    }
    if INTERFACE_EXTENSIONS
        .iter()
        .any(|extension| file.ends_with(extension))
    {
        return VerificationLevel::V1;
    }
    if lower.starts_with("docs/")
        || DOC_NAMES.contains(&file)
        || DOC_EXTENSIONS
            .iter()
            .any(|extension| file.ends_with(&format!(".{extension}")))
    {
        return VerificationLevel::V0;
    }
    VerificationLevel::V2
}

/// Selects the verification level from changed paths with an observed-
/// effect fallback: effect subjects classify through the same rules
/// when no path list is available. Empty inputs on both sides are a
/// no-op ([`VerificationLevel::V0`]); [`RiskTier::Elevated`] raises the
/// floor to [`VerificationLevel::V1`].
pub fn select_verification_level<P: AsRef<str>>(
    changed_paths: &[P],
    observed_effects: &[ObservedEffect],
    risk: RiskTier,
) -> VerificationLevel {
    let mut level = VerificationLevel::V0;
    let mut any = false;
    for path in changed_paths {
        any = true;
        level = level.max(classify_path(path.as_ref()));
    }
    for effect in observed_effects {
        any = true;
        level = level.max(classify_path(effect.subject_ref().as_str()));
    }
    if !any {
        level = VerificationLevel::V0;
    }
    match risk {
        RiskTier::Standard => level,
        RiskTier::Elevated => level.max(VerificationLevel::V1),
    }
}

/// Detects the ecosystem from the first manifest/lockfile signal in
/// path order; [`Ecosystem::Unknown`] when nothing matches.
pub fn detect_ecosystem<P: AsRef<str>>(paths: &[P]) -> Ecosystem {
    for path in paths {
        let lower = path.as_ref().to_ascii_lowercase();
        let file = basename(&lower);
        if file == "cargo.toml" || file == "cargo.lock" {
            return Ecosystem::Cargo;
        }
        if file == "yarn.lock" {
            return Ecosystem::Yarn;
        }
        if file == "pnpm-lock.yaml" {
            return Ecosystem::Pnpm;
        }
        if file == "bun.lockb" {
            return Ecosystem::Bun;
        }
        if file == "package-lock.json" || file == "package.json" {
            return Ecosystem::Npm;
        }
        if file == "go.mod" || file == "go.sum" {
            return Ecosystem::Go;
        }
        if file == "pyproject.toml" || file == "setup.py" || file == "setup.cfg" {
            return Ecosystem::Python;
        }
    }
    Ecosystem::Unknown
}

fn command(program: &str, args: &[&str]) -> VerificationCommand {
    VerificationCommand {
        program: program.to_owned(),
        args: args.iter().map(|arg| (*arg).to_owned()).collect(),
    }
}

/// Derives the conventional verification commands for one
/// (ecosystem, level) pair as data. V0 and unknown ecosystems derive
/// nothing — commands are never fabricated.
#[must_use]
pub fn derive_commands(ecosystem: Ecosystem, level: VerificationLevel) -> Vec<VerificationCommand> {
    if level == VerificationLevel::V0 || ecosystem == Ecosystem::Unknown {
        return Vec::new();
    }
    match ecosystem {
        Ecosystem::Cargo => match level {
            VerificationLevel::V0 => Vec::new(),
            VerificationLevel::V1 => vec![command("cargo", &["check", "--locked"])],
            VerificationLevel::V2 => vec![command("cargo", &["test", "--locked"])],
            VerificationLevel::V3 => vec![
                command("cargo", &["fmt", "--all", "--check"]),
                command(
                    "cargo",
                    &[
                        "clippy",
                        "--workspace",
                        "--all-targets",
                        "--locked",
                        "--",
                        "-D",
                        "warnings",
                    ],
                ),
                command("cargo", &["test", "--workspace", "--locked"]),
            ],
        },
        Ecosystem::Npm | Ecosystem::Pnpm | Ecosystem::Yarn | Ecosystem::Bun => {
            let program = ecosystem.as_str();
            match level {
                VerificationLevel::V0 => Vec::new(),
                VerificationLevel::V1 => vec![command(program, &["run", "typecheck"])],
                VerificationLevel::V2 => vec![command(program, &["test"])],
                VerificationLevel::V3 => vec![
                    command(program, &["run", "lint"]),
                    command(program, &["run", "typecheck"]),
                    command(program, &["test"]),
                ],
            }
        }
        Ecosystem::Go => match level {
            VerificationLevel::V0 => Vec::new(),
            VerificationLevel::V1 => vec![command("go", &["vet", "./..."])],
            VerificationLevel::V2 => vec![command("go", &["test", "./..."])],
            VerificationLevel::V3 => vec![
                command("gofmt", &["-l", "."]),
                command("go", &["vet", "./..."]),
                command("go", &["test", "./..."]),
            ],
        },
        Ecosystem::Python => match level {
            VerificationLevel::V0 => Vec::new(),
            VerificationLevel::V1 => vec![command("python", &["-m", "compileall", "."])],
            VerificationLevel::V2 => vec![command("pytest", &[])],
            VerificationLevel::V3 => vec![
                command("python", &["-m", "compileall", "."]),
                command("pytest", &[]),
            ],
        },
        Ecosystem::Unknown => Vec::new(),
    }
}

/// Maps a level to the [`task_domain::RequirementKind`] obligations it
/// carries through the existing TASK-0123 model. Human-gated kinds are
/// never derived.
#[must_use]
pub fn required_kinds_for_level(level: VerificationLevel) -> Vec<task_domain::RequirementKind> {
    use task_domain::RequirementKind;
    match level {
        VerificationLevel::V0 => Vec::new(),
        VerificationLevel::V1 => vec![RequirementKind::ProcessExit],
        VerificationLevel::V2 => vec![RequirementKind::TestSuite],
        VerificationLevel::V3 => vec![RequirementKind::TestSuite, RequirementKind::SecurityCheck],
    }
}

/// Plans verification for one change: selects the level, scopes to the
/// changed paths (or effect subjects when no path list is available),
/// detects the ecosystem, and derives its commands.
pub fn plan_verification<P: AsRef<str>>(
    changed_paths: &[P],
    observed_effects: &[ObservedEffect],
    risk: RiskTier,
) -> VerificationPlan {
    let level = select_verification_level(changed_paths, observed_effects, risk);
    let scope: Vec<String> = if changed_paths.is_empty() {
        observed_effects
            .iter()
            .map(|effect| effect.subject_ref().as_str().to_owned())
            .collect()
    } else {
        changed_paths
            .iter()
            .map(|path| path.as_ref().to_owned())
            .collect()
    };
    let ecosystem = detect_ecosystem(&scope);
    let commands = derive_commands(ecosystem, level);
    VerificationPlan {
        level,
        ecosystem,
        scope,
        commands,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use evaluation::{
        AttemptId, CaptureAuthority, EffectId, EffectKind, EffectPayload, EffectProvenance,
        EffectSubjectRef, ExecutionId, ObservedEffect, Sha256Digest, SubjectFingerprint,
    };

    fn effect(subject: &str) -> ObservedEffect {
        let digest = format!("sha256:{}", "c".repeat(64));
        ObservedEffect::restore(
            EffectId::new("effect-1").unwrap(),
            EffectSubjectRef::new(subject).unwrap(),
            SubjectFingerprint::new(digest.clone()).unwrap(),
            EffectKind::ArtifactHash,
            CaptureAuthority::LocalHarness,
            "2024-01-01T00:00:00Z",
            EffectPayload::ArtifactHash {
                bytes: 3,
                sha256: Sha256Digest::new(digest).unwrap(),
            },
            EffectProvenance::new(
                ExecutionId::new("E-1").unwrap(),
                AttemptId::new("A-1").unwrap(),
                "test",
            )
            .unwrap(),
        )
        .unwrap()
    }

    #[test]
    fn docs_only_paths_select_v0() {
        let level = select_verification_level(
            &["README.md", "docs/guide.mdx", "LICENSE"],
            &[],
            RiskTier::Standard,
        );
        assert_eq!(level, VerificationLevel::V0);
    }

    #[test]
    fn lockfiles_select_v3() {
        for lockfile in [
            "Cargo.lock",
            "package-lock.json",
            "yarn.lock",
            "pnpm-lock.yaml",
            "bun.lockb",
            "go.sum",
            "poetry.lock",
        ] {
            let level = select_verification_level(&[lockfile], &[], RiskTier::Standard);
            assert_eq!(level, VerificationLevel::V3, "lockfile {lockfile}");
        }
    }

    #[test]
    fn manifests_and_schema_select_v3() {
        for path in [
            "Cargo.toml",
            "package.json",
            "go.mod",
            "db/schema.sql",
            "migrations/0001_init.sql",
        ] {
            let level = select_verification_level(&[path], &[], RiskTier::Standard);
            assert_eq!(level, VerificationLevel::V3, "config/schema {path}");
        }
    }

    #[test]
    fn interface_only_changes_select_v1() {
        for path in ["types/index.d.ts", "proto/api.proto", "include/lib.h"] {
            let level = select_verification_level(&[path], &[], RiskTier::Standard);
            assert_eq!(level, VerificationLevel::V1, "interface {path}");
        }
    }

    #[test]
    fn code_changes_select_v2() {
        let level = select_verification_level(&["src/main.rs"], &[], RiskTier::Standard);
        assert_eq!(level, VerificationLevel::V2);
    }

    #[test]
    fn highest_class_wins_across_paths() {
        let level = select_verification_level(
            &["README.md", "src/main.rs", "Cargo.lock"],
            &[],
            RiskTier::Standard,
        );
        assert_eq!(level, VerificationLevel::V3);
    }

    #[test]
    fn empty_inputs_are_a_noop_v0() {
        let effects: Vec<ObservedEffect> = Vec::new();
        let level = select_verification_level::<String>(&[], &effects, RiskTier::Standard);
        assert_eq!(level, VerificationLevel::V0);
    }

    #[test]
    fn effects_fall_back_to_subject_classification() {
        let effects = vec![effect("src/lib.rs")];
        let level = select_verification_level::<String>(&[], &effects, RiskTier::Standard);
        assert_eq!(level, VerificationLevel::V2);
        let effects = vec![effect("Cargo.lock")];
        let level = select_verification_level::<String>(&[], &effects, RiskTier::Standard);
        assert_eq!(level, VerificationLevel::V3);
    }

    #[test]
    fn elevated_risk_raises_the_floor_to_v1() {
        let level = select_verification_level(&["README.md"], &[], RiskTier::Elevated);
        assert_eq!(level, VerificationLevel::V1);
        let level = select_verification_level(&["src/main.rs"], &[], RiskTier::Elevated);
        assert_eq!(level, VerificationLevel::V2);
    }

    #[test]
    fn ecosystem_detection_prefers_first_signal() {
        assert_eq!(
            detect_ecosystem(&["Cargo.toml", "package.json"]),
            Ecosystem::Cargo
        );
        assert_eq!(detect_ecosystem(&["yarn.lock"]), Ecosystem::Yarn);
        assert_eq!(detect_ecosystem(&["go.mod"]), Ecosystem::Go);
        assert_eq!(detect_ecosystem(&["pyproject.toml"]), Ecosystem::Python);
        assert_eq!(detect_ecosystem(&["src/main.rs"]), Ecosystem::Unknown);
    }

    #[test]
    fn v0_derives_no_commands_and_unknown_derives_none() {
        assert!(derive_commands(Ecosystem::Cargo, VerificationLevel::V0).is_empty());
        assert!(
            derive_commands(Ecosystem::Unknown, VerificationLevel::V3).is_empty(),
            "unknown ecosystems must not fabricate commands"
        );
        let full = derive_commands(Ecosystem::Cargo, VerificationLevel::V3);
        assert!(full.iter().all(|command| command.program == "cargo"));
        assert!(full.len() >= 3);
    }

    #[test]
    fn levels_map_to_requirement_kinds() {
        use task_domain::RequirementKind;
        assert!(required_kinds_for_level(VerificationLevel::V0).is_empty());
        assert_eq!(
            required_kinds_for_level(VerificationLevel::V1),
            vec![RequirementKind::ProcessExit]
        );
        assert_eq!(
            required_kinds_for_level(VerificationLevel::V2),
            vec![RequirementKind::TestSuite]
        );
        assert_eq!(
            required_kinds_for_level(VerificationLevel::V3),
            vec![RequirementKind::TestSuite, RequirementKind::SecurityCheck,]
        );
    }

    #[test]
    fn plan_is_deterministic_and_scoped() {
        let paths = ["src/main.rs", "Cargo.toml"];
        let effects: Vec<ObservedEffect> = Vec::new();
        let first = plan_verification(&paths, &effects, RiskTier::Standard);
        let second = plan_verification(&paths, &effects, RiskTier::Standard);
        assert_eq!(first, second);
        assert_eq!(first.level, VerificationLevel::V3);
        assert_eq!(first.ecosystem, Ecosystem::Cargo);
        assert_eq!(first.scope, vec!["src/main.rs", "Cargo.toml"]);
        assert!(!first.commands.is_empty());
    }
}
