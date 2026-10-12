//! History ranking with disclosed signals (TASK-0171, ADR-0043 §7).

use task_domain::TaskId;

use crate::model::{EpochId, HistoryRef};

/// Recency share of the rank score (weight tuning is TASK-0171's).
pub const RECENCY_WEIGHT: f64 = 0.4;
/// Full-text share of the rank score.
pub const TEXT_WEIGHT: f64 = 0.6;
/// Hard cap on history search results (CR-4: bounded reads, no bulk).
pub const HISTORY_SEARCH_LIMIT: usize = 100;

/// Which field produced the text match (disclosed per hit).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchField {
    Title,
    Body,
    Metadata,
}

/// Disclosed rank signals: recency plus full-text rank.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RankSignals {
    pub recency: f64,
    pub text: f64,
    pub matched: MatchField,
}

/// How the hit snippet was built (disclosed per hit).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SnippetMethod {
    TitleMatch,
    BodyExcerpt,
    MetadataOnly,
}

/// What a history ref points at (disclosed per hit for provenance).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HistoryKind {
    Checkpoint,
    Note,
    Receipt,
}

impl HistoryKind {
    /// Canonical wire name (also the SQL CHECK value).
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Checkpoint => "CHECKPOINT",
            Self::Note => "NOTE",
            Self::Receipt => "RECEIPT",
        }
    }

    /// Parses a wire name; unknown values fail closed.
    pub fn parse(raw: &str) -> Result<Self, crate::model::ContinuityError> {
        match raw {
            "CHECKPOINT" => Ok(Self::Checkpoint),
            "NOTE" => Ok(Self::Note),
            "RECEIPT" => Ok(Self::Receipt),
            _ => Err(crate::model::ContinuityError::InvalidInput(format!(
                "unknown history kind: {raw}"
            ))),
        }
    }
}

/// One ranked history hit (ADR-0043 §7).
#[derive(Debug, Clone, PartialEq)]
pub struct HistoryHit {
    pub history_ref: HistoryRef,
    pub epoch_id: EpochId,
    pub kind: HistoryKind,
    pub rank: RankSignals,
    pub snippet: SnippetMethod,
}

/// Store-supplied rank input: a LIKE match over committed-ref metadata.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoryCandidate {
    pub history_ref: HistoryRef,
    pub epoch_id: EpochId,
    pub kind: HistoryKind,
    pub created_at: u64,
    pub title: String,
    pub body: String,
}

/// Accepted search filters (ADR-0043 §7): metadata, task, epoch, event
/// type. Embeddings stay optional and out of this filter set.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SearchFilters {
    pub task: Option<TaskId>,
    pub epoch: Option<EpochId>,
    pub event_type: Option<String>,
    pub metadata: Vec<(String, String)>,
}

/// Bounded content for one exact history ref (CR-4): title plus an
/// excerpt no larger than its validated source (notes and
/// continuations are ≤4096 chars by construction).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoryContent {
    pub history_ref: HistoryRef,
    pub epoch_id: EpochId,
    pub kind: HistoryKind,
    pub title: String,
    pub excerpt: String,
}

fn occurrences(haystack: &str, needle: &str) -> usize {
    if needle.is_empty() {
        return 0;
    }
    let haystack = haystack.to_lowercase();
    let needle = needle.to_lowercase();
    haystack.match_indices(&needle).count()
}

/// Ranks candidates by disclosed signals: recency decays per hour
/// (`1/(1+age_hours)`), text counts case-insensitive substring hits
/// (title hits weigh double, saturating at 1.0). Score-descending,
/// ties broken by history ref for determinism.
pub fn rank_hits(
    candidates: Vec<HistoryCandidate>,
    query: &str,
    now_millis: u64,
) -> Vec<HistoryHit> {
    let mut scored: Vec<(f64, HistoryHit)> = candidates
        .into_iter()
        .map(|candidate| {
            let age_hours = now_millis.saturating_sub(candidate.created_at) as f64 / 3_600_000.0;
            let recency = 1.0 / (1.0 + age_hours);
            let title_hits = occurrences(&candidate.title, query);
            let body_hits = occurrences(&candidate.body, query);
            let text = ((title_hits * 2 + body_hits) as f64 / 4.0).min(1.0);
            let (matched, snippet) = if title_hits > 0 {
                (MatchField::Title, SnippetMethod::TitleMatch)
            } else if body_hits > 0 {
                (MatchField::Body, SnippetMethod::BodyExcerpt)
            } else {
                (MatchField::Metadata, SnippetMethod::MetadataOnly)
            };
            let score = RECENCY_WEIGHT * recency + TEXT_WEIGHT * text;
            (
                score,
                HistoryHit {
                    history_ref: candidate.history_ref,
                    epoch_id: candidate.epoch_id,
                    kind: candidate.kind,
                    rank: RankSignals {
                        recency,
                        text,
                        matched,
                    },
                    snippet,
                },
            )
        })
        .collect();
    scored.sort_by(|left, right| {
        right
            .0
            .partial_cmp(&left.0)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| left.1.history_ref.cmp(&right.1.history_ref))
    });
    scored.into_iter().map(|(_, hit)| hit).collect()
}

#[cfg(test)]
mod tests {
    use task_domain::TaskId;

    use crate::history::{
        rank_hits, HistoryCandidate, HistoryKind, MatchField, RECENCY_WEIGHT, TEXT_WEIGHT,
    };
    use crate::mint::{mint_epoch_id, mint_history_ref};

    fn candidate(seq: u128, created_at: u64, title: &str, body: &str) -> HistoryCandidate {
        HistoryCandidate {
            history_ref: mint_history_ref(1000, seq),
            epoch_id: mint_epoch_id(1000, 1),
            kind: HistoryKind::Note,
            created_at,
            title: title.to_owned(),
            body: body.to_owned(),
        }
    }

    #[test]
    fn rank_prefers_recent_on_tie() {
        let old = candidate(1, 1000, "rollover done", "checkpoint committed");
        let new = candidate(2, 9000, "rollover done", "checkpoint committed");
        let ranked = rank_hits(vec![old, new], "rollover", 10_000);
        assert_eq!(ranked.len(), 2);
        assert!(ranked[0].rank.recency > ranked[1].rank.recency);
        assert_eq!(RECENCY_WEIGHT + TEXT_WEIGHT, 1.0);
    }

    #[test]
    fn rank_discloses_matched_field() {
        let hit = &rank_hits(
            vec![candidate(1, 1000, "migration notes", "checkpoint sealed")],
            "migration",
            10_000,
        )[0];
        assert_eq!(hit.rank.matched, MatchField::Title);
        let hit = &rank_hits(
            vec![candidate(1, 1000, "daily notes", "migration sealed")],
            "migration",
            10_000,
        )[0];
        assert_eq!(hit.rank.matched, MatchField::Body);
    }

    #[test]
    fn rank_is_stable_for_equal_scores() {
        let a = candidate(1, 1000, "same words here", "same words here");
        let b = candidate(2, 1000, "same words here", "same words here");
        let ranked = rank_hits(vec![b.clone(), a.clone()], "same", 10_000);
        assert_eq!(ranked[0].history_ref, a.history_ref);
        assert_eq!(ranked[1].history_ref, b.history_ref);
        let _ = TaskId::new("T-1").unwrap();
    }
}
