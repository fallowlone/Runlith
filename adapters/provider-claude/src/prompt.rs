//! Bounded task-prompt derivation (TASK-0046, G5).
//!
//! Derives the provider input text from durable Task facts (objective +
//! acceptance criteria) only. The prompt is evidence scope, never a
//! security boundary: workspace confinement comes from the adapter's
//! `current_dir` binding, and completion authority stays with the
//! evaluation pipeline no matter what the prompt or the model says.

use crate::config::MAX_PROMPT_CHARS;

/// Builds the bounded prompt for one execution from the Task's durable
/// intent. Truncates explicitly (never silently) when the objective plus
/// acceptance criteria exceed [`MAX_PROMPT_CHARS`].
#[must_use]
pub fn build_task_prompt(objective: &str, acceptance: &[String]) -> String {
    let mut prompt = String::from(
        "Complete the following task. Work inside the current directory only.\n\nTask:\n",
    );
    prompt.push_str(objective.trim());
    if !acceptance.is_empty() {
        prompt.push_str("\n\nAcceptance criteria:\n");
        for (index, criterion) in acceptance.iter().enumerate() {
            prompt.push_str(&format!("\n{}. {}", index + 1, criterion.trim()));
        }
    }
    prompt.push_str("\n\nWhen finished, summarize what you did concisely.");
    if prompt.chars().count() <= MAX_PROMPT_CHARS {
        return prompt;
    }
    let truncated: String = prompt.chars().take(MAX_PROMPT_CHARS).collect();
    format!("{truncated}…[truncated]")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prompt_carries_objective_and_acceptance() {
        let prompt = build_task_prompt("fix the bug", &["tests pass".to_owned()]);
        assert!(prompt.contains("fix the bug"));
        assert!(prompt.contains("tests pass"));
        assert!(prompt.contains("current directory"));
    }

    #[test]
    fn prompt_is_bounded_with_explicit_marker() {
        let big = "x".repeat(MAX_PROMPT_CHARS + 100);
        let prompt = build_task_prompt(&big, &[]);
        assert!(prompt.chars().count() <= MAX_PROMPT_CHARS + "…[truncated]".chars().count());
        assert!(prompt.ends_with("…[truncated]"));
    }

    #[test]
    fn prompt_never_grants_completion_authority() {
        // Static guard on the only instruction text this adapter authors:
        // no completion/verification vocabulary may appear.
        let prompt = build_task_prompt("objective", &["criterion".to_owned()]);
        let lowered = prompt.to_ascii_lowercase();
        for forbidden in ["completed", "verified", "approved", "merge"] {
            assert!(
                !lowered.contains(forbidden),
                "prompt must not contain {forbidden:?}"
            );
        }
    }
}
