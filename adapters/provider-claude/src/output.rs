//! Claude output translation (TASK-0046, G5).
//!
//! Raw CLI output never crosses into core business logic: this module
//! parses the single `--output-format json` result object into an
//! adapter-internal representation, which the executor then maps onto
//! the canonical seam (`WorkEvidence` / `ExecutionProblem`).
//!
//! ```text
//! CLI stdout (untrusted process data)
//!  → parser (bounded, non-panicking, forward-compatible)
//!  → ParsedOutput (adapter-internal)
//!  → canonical WorkEvidence / ExecutionProblem
//! ```
//!
//! Translation covers only what the current runtime needs: session
//! started (as evidence reference), agent output (as summary), usage
//! observation, finished / failed / cancelled. Provider output is
//! evidence/input, never authority: a `"success"` subtype never means
//! Task completion, and malformed critical records fail explicitly.

use crate::config::{bounded_detail, MAX_DETAIL_CHARS};
use crate::failure::ClaudeFailure;
use crate::json::{parse_json, Json};
use crate::usage::UsageSample;

/// Maximum session-reference length accepted from provider output.
/// Real session ids are UUIDs; anything longer is untrusted bloat.
const MAX_SESSION_REF_CHARS: usize = 256;

/// Adapter-internal normalized provider result: the only shape allowed
/// past the CLI boundary. Core never matches on Claude JSON fields.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedOutput {
    /// Opaque provider session reference (evidence only, never a durable
    /// identity, never completion authority).
    pub session_ref: String,
    /// Bounded model output text (evidence summary, never a transcript).
    pub result_text: String,
    /// Provider-reported usage, or unknown when absent.
    pub usage: UsageSample,
    /// Whether the provider flagged its own result as erroneous.
    pub provider_error: bool,
    /// Provider subtype string (diagnostic detail only, never branched on
    /// outside this adapter).
    pub subtype: String,
}

/// Parses one `--output-format json` result document.
///
/// `stdout` must already be capture-bounded by the caller. Empty input
/// (unexpected EOF) and non-JSON input are explicit
/// [`ClaudeFailure::MalformedOutput`]; a provider-flagged error
/// (`is_error` or a non-`success` subtype) is an explicit
/// [`ClaudeFailure::Failed`]. Unknown non-critical fields are ignored per
/// the documented forward-compatibility policy; missing critical fields
/// (`type`, `result`, `session_id`) are malformed.
pub fn parse_provider_output(stdout: &str) -> Result<ParsedOutput, ClaudeFailure> {
    if stdout.trim().is_empty() {
        return Err(ClaudeFailure::malformed(
            "claude produced no output (unexpected EOF); nothing to trust",
        ));
    }
    let json = parse_json(stdout).map_err(|error| {
        ClaudeFailure::malformed(format!("claude output is not valid JSON: {error}"))
    })?;
    parse_result_object(&json)
}

fn parse_result_object(json: &Json) -> Result<ParsedOutput, ClaudeFailure> {
    let kind = json
        .get("type")
        .and_then(|v| v.as_str())
        .ok_or_else(|| ClaudeFailure::malformed("claude result has no string 'type' field"))?;
    if kind != "result" {
        return Err(ClaudeFailure::malformed(format!(
            "claude output type is {kind:?}, not \"result\""
        )));
    }
    let subtype = json
        .get("subtype")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown")
        .to_owned();
    let provider_error = json
        .get("is_error")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
        || subtype != "success";
    let result_text = json
        .get("result")
        .and_then(|v| v.as_str())
        .ok_or_else(|| ClaudeFailure::malformed("claude result has no string 'result' field"))?;
    if result_text.trim().is_empty() {
        return Err(ClaudeFailure::malformed(
            "claude result text is blank; blank output is never evidence",
        ));
    }
    let session_ref = json
        .get("session_id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            ClaudeFailure::malformed("claude result has no string 'session_id' field")
        })?;
    validate_session_ref(session_ref)?;
    if provider_error {
        return Err(ClaudeFailure::failed(format!(
            "claude reported provider error (subtype {subtype:?}): {}",
            truncate_result(result_text)
        )));
    }
    Ok(ParsedOutput {
        session_ref: session_ref.to_owned(),
        result_text: result_text.to_owned(),
        usage: parse_usage(json),
        provider_error: false,
        subtype,
    })
}

/// Session references are opaque bounded text: non-blank, no NUL, no
/// whitespace interior (real ids are UUIDs/hex). Anything else is
/// malformed provider data, never a durable identity.
fn validate_session_ref(session_ref: &str) -> Result<(), ClaudeFailure> {
    let trimmed = session_ref.trim();
    if trimmed.is_empty()
        || trimmed.len() > MAX_SESSION_REF_CHARS
        || trimmed.contains('\0')
        || trimmed.chars().any(char::is_whitespace)
        || trimmed != session_ref
    {
        return Err(ClaudeFailure::malformed(
            "claude session reference is not opaque bounded text",
        ));
    }
    Ok(())
}

/// Extracts the optional `usage` object. Absent or unusable usage is
/// [`UsageProvenance::Unknown`] — never zero-filled, never estimated.
fn parse_usage(json: &Json) -> UsageSample {
    let usage = match json.get("usage") {
        Some(usage) => usage,
        None => return UsageSample::unknown(),
    };
    UsageSample::reported(
        usage.get("input_tokens").and_then(|v| v.as_u64()),
        usage
            .get("cache_creation_input_tokens")
            .and_then(|v| v.as_u64()),
        usage
            .get("cache_read_input_tokens")
            .and_then(|v| v.as_u64()),
        usage.get("output_tokens").and_then(|v| v.as_u64()),
    )
}

/// Bounded excerpt of provider result text for failure details.
fn truncate_result(text: &str) -> String {
    bounded_detail(text.trim().to_owned())
        .chars()
        .take(MAX_DETAIL_CHARS)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::usage::UsageProvenance;

    fn success_doc(result: &str) -> String {
        format!(
            r#"{{"type":"result","subtype":"success","is_error":false,"result":{result:?},"session_id":"550e8400-e29b-41d4-a716-446655440000","usage":{{"input_tokens":12,"output_tokens":34}},"total_cost_usd":0.001,"unknown_future":"ignored"}}"#
        )
    }

    #[test]
    fn success_parses_to_canonical_evidence() {
        let parsed = parse_provider_output(&success_doc("did the work")).unwrap();
        assert_eq!(parsed.session_ref, "550e8400-e29b-41d4-a716-446655440000");
        assert_eq!(parsed.result_text, "did the work");
        assert!(!parsed.provider_error);
        assert_eq!(parsed.subtype, "success");
        assert_eq!(parsed.usage.provenance, UsageProvenance::Reported);
        assert_eq!(
            (parsed.usage.input_tokens, parsed.usage.output_tokens),
            (Some(12), Some(34))
        );
        assert_eq!(parsed.usage.total_input_tokens(), Some(12));
    }

    #[test]
    fn cache_usage_is_preserved_and_counts_toward_total_input() {
        let doc = r#"{"type":"result","subtype":"success","is_error":false,"result":"x","session_id":"s-1","usage":{"input_tokens":5,"cache_creation_input_tokens":40000,"cache_read_input_tokens":600000,"output_tokens":7}}"#;
        let parsed = parse_provider_output(doc).unwrap();
        assert_eq!(parsed.usage.input_tokens, Some(5));
        assert_eq!(parsed.usage.cache_creation_input_tokens, Some(40_000));
        assert_eq!(parsed.usage.cache_read_input_tokens, Some(600_000));
        assert_eq!(parsed.usage.total_input_tokens(), Some(640_005));
    }

    #[test]
    fn usage_absent_is_unknown_never_zero() {
        let doc = r#"{"type":"result","subtype":"success","is_error":false,"result":"x","session_id":"s-1"}"#;
        let parsed = parse_provider_output(doc).unwrap();
        assert_eq!(parsed.usage, UsageSample::unknown());
    }

    #[test]
    fn provider_error_flag_is_failure_not_evidence() {
        for doc in [
            r#"{"type":"result","subtype":"error_during_execution","is_error":true,"result":"boom","session_id":"s-1"}"#,
            r#"{"type":"result","subtype":"error_something","result":"boom","session_id":"s-1"}"#,
            r#"{"type":"result","subtype":"success","is_error":true,"result":"boom","session_id":"s-1"}"#,
        ] {
            let err = parse_provider_output(doc).expect_err("provider error must fail");
            assert!(
                matches!(err, ClaudeFailure::Failed { .. }),
                "provider error must be Failed, got {err:?}"
            );
        }
    }

    #[test]
    fn malformed_critical_records_fail_explicitly() {
        for doc in [
            "",
            "   ",
            "not json",
            r#"{"type":"message","subtype":"success","result":"x","session_id":"s-1"}"#,
            r#"{"subtype":"success","result":"x","session_id":"s-1"}"#,
            r#"{"type":"result","subtype":"success","session_id":"s-1"}"#,
            r#"{"type":"result","subtype":"success","result":"   ","session_id":"s-1"}"#,
            r#"{"type":"result","subtype":"success","result":"x"}"#,
            r#"{"type":"result","subtype":"success","result":"x","session_id":"has space"}"#,
            r#"{"type":"result","subtype":"success","result":"x","session_id":""}"#,
        ] {
            let err = parse_provider_output(doc).expect_err("malformed must fail");
            assert!(
                matches!(err, ClaudeFailure::MalformedOutput { .. }),
                "malformed must be MalformedOutput, got {err:?} for {doc:?}"
            );
        }
    }

    #[test]
    fn unknown_noncritical_records_are_ignored_forward_compatibly() {
        // A future CLI adding fields (or reordering) still parses: only
        // the documented critical fields gate success.
        let doc = r#"{"session_id":"s-9","zzz":{"deep":[1,{"y":null}]},"result":"ok","is_error":false,"type":"result","subtype":"success"}"#;
        let parsed = parse_provider_output(doc).unwrap();
        assert_eq!(parsed.result_text, "ok");
        assert_eq!(parsed.session_ref, "s-9");
    }
}
