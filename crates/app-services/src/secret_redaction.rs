//! INV-008 display redaction for finder output (TASK-0174).
//!
//! Pure total functions: never fail, never panic, char-boundary
//! safe. Assignment shapes follow the policy crate's
//! `looks_like_secret_material` (PEM private keys + the seven
//! canonical keys); vendor prefixes and the base64-body rule are this
//! screen's operationalization of §SECRET's "raw provider
//! credentials" (`docs/security/DATA_CLASSIFICATION.md` names no
//! token catalog). Display is deliberately stricter than admission:
//! the policy screen has no prefix/body rules. Unlike admission
//! (which rejects whole literals), display redacts *values*: names
//! and separators stay so lines remain identifiable.
//!
//! Documented residuals (fail-open shapes, accepted): bare
//! high-entropy tokens with no marker (indistinguishable from
//! hashes, which must pass); dotted credentials such as JWTs (dots
//! delimit runs); lone short base64 fragments with no redacted
//! neighbor; all-lowercase short fragments next to redacted lines.

/// Display marker replacing redacted secret bytes.
pub const REDACTED: &str = "[redacted]";

/// Canonical assignment keys, mirroring the policy crate's secret
/// screen (`looks_like_secret_material`).
const ASSIGNMENT_KEYS: [&str; 7] = [
    "password", "passwd", "secret", "apikey", "api_key", "api-key", "token",
];

/// Vendor credential prefixes with minimum suffix lengths. The floor
/// keeps bare doc mentions (`sk-ant-`, `AKIA`) from firing: only a
/// prefix carrying a plausible credential body redacts.
const PREFIX_FLOORS: [(&str, usize); 12] = [
    ("AKIA", 16),
    ("ghp_", 8),
    ("gho_", 8),
    ("github_pat_", 8),
    ("xoxb-", 8),
    ("xoxp-", 8),
    ("xoxa-", 8),
    ("xoxr-", 8),
    ("xoxs-", 8),
    ("sk-ant-", 8),
    ("glpat-", 8),
    ("AIza", 8),
];

/// Redacts secret-shaped values on one display line.
pub fn redact_line(line: &str) -> String {
    if is_pem_private_begin(line) || is_pem_private_end(line) {
        return REDACTED.to_string();
    }
    let redacted = redact_assignments(line);
    let redacted = redact_prefixed_tokens(&redacted);
    redact_base64_runs(&redacted, MIN_BODY_RUN, false)
}

/// Redacts a display window. Stateful across lines: a PEM private-key
/// block redacts from its BEGIN line through its END line; mid-block
/// slices (no BEGIN in view) redact via the body-token rule, and
/// short fragments adjacent to redacted lines are caught too.
pub fn redact_lines(lines: &[String]) -> Vec<String> {
    let mut in_key_block = false;
    let mut redacted: Vec<String> = lines
        .iter()
        .map(|line| {
            if in_key_block {
                if is_pem_private_end(line) {
                    in_key_block = false;
                }
                return REDACTED.to_string();
            }
            if is_pem_private_begin(line) {
                in_key_block = !is_pem_private_end(line);
                return REDACTED.to_string();
            }
            redact_line(line)
        })
        .collect();
    // Adjacency catch: a short base64 fragment (key tail chunk) next
    // to a redacted line is presumptively key material too.
    let fired: Vec<bool> = redacted.iter().map(|line| line == REDACTED).collect();
    for (index, line) in redacted.iter_mut().enumerate() {
        let adjacent =
            (index > 0 && fired[index - 1]) || (index + 1 < fired.len() && fired[index + 1]);
        if adjacent && line != REDACTED {
            *line = redact_base64_runs(line, MIN_FRAGMENT_RUN, true);
        }
    }
    redacted
}

fn is_pem_private_begin(line: &str) -> bool {
    let normalized = line.to_ascii_lowercase();
    normalized.contains("-----begin") && normalized.contains("private key-----")
}

fn is_pem_private_end(line: &str) -> bool {
    let normalized = line.to_ascii_lowercase();
    normalized.contains("-----end") && normalized.contains("private key-----")
}

/// Value redaction for the canonical assignment keys. Keeps the name
/// and separator so the line stays identifiable; replaces the value
/// (quoted through its close quote, else the non-space run) with the
/// marker. Repeats until no key fires (several pairs per line).
fn redact_assignments(line: &str) -> String {
    let mut current = line.to_string();
    while let Some((value_start, value_end)) = first_assignment_value(&current) {
        current.replace_range(value_start..value_end, REDACTED);
    }
    current
}

/// Byte span of the first redactable assignment value, or `None`.
/// All indices are `char` boundaries by construction (splits only on
/// ASCII bytes, which are never inside a multi-byte char).
fn first_assignment_value(line: &str) -> Option<(usize, usize)> {
    let normalized = line.to_ascii_lowercase();
    for key in ASSIGNMENT_KEYS {
        for (index, _) in normalized.match_indices(key) {
            let mut rest = &line[index + key.len()..];
            rest = rest.trim_start();
            rest = rest
                .strip_prefix(['\'', '"'])
                .map(str::trim_start)
                .unwrap_or(rest);
            let Some(sep) = rest.strip_prefix(['=', ':']) else {
                continue;
            };
            let value = sep.trim_start();
            if value.is_empty() {
                continue;
            }
            let value_start = line.len() - value.len();
            let value_end = if value.starts_with(['\'', '"']) {
                let quote = value.as_bytes()[0];
                match value[1..].find(quote as char) {
                    Some(end) => value_start + 1 + end + 1,
                    None => line.len(),
                }
            } else {
                match value.find(char::is_whitespace) {
                    Some(end) => value_start + end,
                    None => line.len(),
                }
            };
            // Skip values already consumed by an earlier pass (the
            // marker itself, possibly abutted by structural punctuation
            // like a JSON `}`): replacement must terminate, and a
            // pre-existing literal marker carries no secret bytes.
            if line[value_start..].starts_with(REDACTED) {
                continue;
            }
            return Some((value_start, value_end));
        }
    }
    None
}

/// Vendor-prefixed credentials and `Bearer` tokens. Redacts the
/// prefix plus its body; `Bearer` keeps the scheme word (the name,
/// like assignment names) and redacts the token run.
fn redact_prefixed_tokens(line: &str) -> String {
    let mut current = line.to_string();
    loop {
        let before = current.clone();
        for (prefix, floor) in PREFIX_FLOORS {
            current = redact_one_prefix(&current, prefix, floor, false);
        }
        current = redact_bearer(&current);
        if current == before {
            break;
        }
    }
    current
}

fn is_secret_char(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'~' | b'+' | b'/' | b'=' | b'.')
}

fn redact_one_prefix(line: &str, prefix: &str, floor: usize, keep_prefix: bool) -> String {
    let mut out = String::with_capacity(line.len());
    let mut rest = line;
    while let Some(index) = rest.find(prefix) {
        let after = &rest[index + prefix.len()..];
        let run = after
            .bytes()
            .take_while(|byte| is_secret_char(*byte))
            .count();
        if run >= floor {
            let cut = index + prefix.len() + run;
            out.push_str(&rest[..index]);
            if keep_prefix {
                out.push_str(prefix);
            }
            out.push_str(REDACTED);
            rest = &rest[cut..];
        } else {
            let cut = index + prefix.len();
            out.push_str(&rest[..cut]);
            rest = &rest[cut..];
        }
    }
    out.push_str(rest);
    out
}

fn redact_bearer(line: &str) -> String {
    // Case-insensitive scheme word; the original casing stays, the
    // token run redacts.
    let mut out = String::with_capacity(line.len());
    let mut rest = line;
    while let Some(index) = rest.to_ascii_lowercase().find("bearer") {
        // Word boundary on the left (never `keybearer`).
        if index > 0 {
            let before = rest.as_bytes()[index - 1];
            if before.is_ascii_alphanumeric() || before == b'_' {
                let cut = index + 6;
                out.push_str(&rest[..cut]);
                rest = &rest[cut..];
                continue;
            }
        }
        let after_scheme = &rest[index + 6..];
        let spaces = after_scheme
            .bytes()
            .take_while(|byte| byte.is_ascii_whitespace())
            .count();
        if spaces == 0 {
            let cut = index + 6;
            out.push_str(&rest[..cut]);
            rest = &rest[cut..];
            continue;
        }
        let run_start = index + 6 + spaces;
        let run = rest[run_start..]
            .bytes()
            .take_while(|byte| is_secret_char(*byte))
            .count();
        if run >= 8 {
            out.push_str(&rest[..run_start]);
            out.push_str(REDACTED);
            rest = &rest[run_start + run..];
        } else {
            let cut = run_start + run;
            out.push_str(&rest[..cut]);
            rest = &rest[cut..];
        }
    }
    out.push_str(rest);
    out
}

/// Minimum mixed-base64 run that fires statelessly (PEM bodies are
/// 64 chars; 32+ mixed tokens are vanishingly rare in prose).
const MIN_BODY_RUN: usize = 32;
/// Minimum fragment caught by window adjacency (key tail chunks).
const MIN_FRAGMENT_RUN: usize = 4;

fn is_base64_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'+' || byte == b'/' || byte == b'='
}

/// Outside-hex signal: hex hashes and UUIDs never carry these, PEM
/// bodies virtually always do.
fn is_mixed_signal(byte: u8) -> bool {
    matches!(byte, b'g'..=b'z' | b'G'..=b'Z' | b'+' | b'/' | b'=')
}

/// Strict fragment signal for window adjacency: uppercase, digits,
/// or base64 punctuation. Plain lowercase English words next to a
/// redacted line must NOT fire.
fn is_fragment_signal(byte: u8) -> bool {
    matches!(byte, b'A'..=b'Z' | b'0'..=b'9' | b'+' | b'/' | b'=')
}

/// Redacts maximal base64-alphabet runs of at least `floor` bytes
/// carrying a signal. The stateless pass uses the mixed signal
/// (long runs); the adjacency pass uses the strict fragment signal
/// (short runs next to redacted lines). Catches key bodies without
/// markers (excerpts, mid-block windows); hex hashes, UUIDs, dotted
/// names, lowercase prose, and lone short fragments pass.
fn redact_base64_runs(line: &str, floor: usize, strict: bool) -> String {
    let bytes = line.as_bytes();
    let mut out = String::with_capacity(line.len());
    let mut index = 0;
    // All slice boundaries fall on ASCII bytes, which are always
    // char boundaries — safe for multi-byte text on either side.
    while index < bytes.len() {
        if !is_base64_byte(bytes[index]) {
            let start = index;
            while index < bytes.len() && !is_base64_byte(bytes[index]) {
                index += 1;
            }
            out.push_str(&line[start..index]);
            continue;
        }
        let start = index;
        while index < bytes.len() && is_base64_byte(bytes[index]) {
            index += 1;
        }
        let run = &bytes[start..index];
        let signal = if strict {
            run.iter().any(|byte| is_fragment_signal(*byte))
        } else {
            run.iter().any(|byte| is_mixed_signal(*byte))
        };
        if run.len() >= floor && signal {
            out.push_str(REDACTED);
        } else {
            out.push_str(&line[start..index]);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_assignment_values_and_keeps_names() {
        assert_eq!(redact_line("password=hunter2"), "password=[redacted]");
        assert_eq!(redact_line("Password: hunter2"), "Password: [redacted]");
        assert_eq!(redact_line("passwd = hunter2"), "passwd = [redacted]");
        assert_eq!(redact_line("api_key=hunter2"), "api_key=[redacted]");
        assert_eq!(redact_line("APIKEY: hunter2"), "APIKEY: [redacted]");
        assert_eq!(redact_line("api-key = hunter2"), "api-key = [redacted]");
        assert_eq!(redact_line("secret=hunter2"), "secret=[redacted]");
        assert_eq!(redact_line("token: hunter2"), "token: [redacted]");
    }

    #[test]
    fn redacts_quoted_and_json_values() {
        assert_eq!(
            redact_line("\"password\": \"hunter2\""),
            "\"password\": [redacted]"
        );
        assert_eq!(
            redact_line("{\"api_key\": \"sk-test\"}"),
            "{\"api_key\": [redacted]}"
        );
        assert_eq!(redact_line("secret='abc def'"), "secret=[redacted]");
    }

    #[test]
    fn redacts_pem_private_key_lines() {
        assert_eq!(redact_line("-----BEGIN PRIVATE KEY-----"), "[redacted]");
        assert_eq!(redact_line("-----BEGIN RSA PRIVATE KEY-----"), "[redacted]");
        let window = redact_lines(&[
            "-----BEGIN PRIVATE KEY-----".to_string(),
            "MIIEvwIBADANBgkqhkiG9w0BAQEFAASC".to_string(),
            "-----END PRIVATE KEY-----".to_string(),
            "trailing".to_string(),
        ]);
        assert_eq!(
            window,
            vec![
                "[redacted]".to_string(),
                "[redacted]".to_string(),
                "[redacted]".to_string(),
                "trailing".to_string(),
            ]
        );
    }

    #[test]
    fn redacts_provider_prefixes_with_suffix_floor() {
        assert_eq!(
            redact_line("key = AKIAIOSFODNN7EXAMPLE"),
            "key = [redacted]"
        );
        assert_eq!(
            redact_line("token ghp_abcdefgh1234567890"),
            "token [redacted]"
        );
        assert_eq!(
            redact_line("Authorization: Bearer abcdefgh1234"),
            "Authorization: Bearer [redacted]"
        );
        // Bare prefixes without a suffix never fire (doc mentions).
        assert_eq!(
            redact_line("uses `sk-ant-` prefixes"),
            "uses `sk-ant-` prefixes"
        );
        assert_eq!(
            redact_line("AKIA is an AWS prefix"),
            "AKIA is an AWS prefix"
        );
    }

    #[test]
    fn leaves_benign_content_untouched() {
        assert_eq!(
            redact_line("sha256:ca978112ca1bbdcafac231b39a23dc4da786eff8147c4e72b9807785afee48bb"),
            "sha256:ca978112ca1bbdcafac231b39a23dc4da786eff8147c4e72b9807785afee48bb"
        );
        assert_eq!(
            redact_line("id: 550e8400-e29b-41d4-a716-446655440000"),
            "id: 550e8400-e29b-41d4-a716-446655440000"
        );
        assert_eq!(
            redact_line("fix token ring handling"),
            "fix token ring handling"
        );
        assert_eq!(
            redact_line("the secret of good code"),
            "the secret of good code"
        );
        assert_eq!(
            redact_line("let token = tokens[i]; // see below"),
            "let token = [redacted] // see below"
        );
    }

    #[test]
    fn redacts_lone_end_lines() {
        assert_eq!(redact_line("-----END PRIVATE KEY-----"), "[redacted]");
    }

    #[test]
    fn redacts_base64_body_tokens() {
        let body = "MIIEvwIBADANBgkqhkiG9w0BAQEFAASCAnkwggJjAgEAAoIBAQC7VJTUt9Us8A==";
        assert_eq!(redact_line(body), "[redacted]");
        assert_eq!(
            redact_line(&format!("\"private_key\": \"{body}\"")),
            "\"private_key\": \"[redacted]\""
        );
        // Hex hashes, UUIDs, dotted names: not base64-shaped, pass through.
        assert_eq!(
            redact_line("ca978112ca1bbdcafac231b39a23dc4da786eff8147c4e72b9807785afee48bb"),
            "ca978112ca1bbdcafac231b39a23dc4da786eff8147c4e72b9807785afee48bb"
        );
        assert_eq!(
            redact_line("host a.very.long.hostname.example.com ok"),
            "host a.very.long.hostname.example.com ok"
        );
        // Short fragments alone are not shaped enough to fire.
        assert_eq!(redact_line("see abcd12=="), "see abcd12==");
    }

    #[test]
    fn redacts_mid_block_windows_and_adjacent_fragments() {
        let body = "MIIEvwIBADANBgkqhkiG9w0BAQEFAASCAnkwggJjAgEAAoIBAQC7VJTUt9Us8A==";
        let window = redact_lines(&[body.to_string(), body.to_string(), "tail qXk9".to_string()]);
        assert_eq!(
            window,
            vec![
                "[redacted]".to_string(),
                "[redacted]".to_string(),
                "tail [redacted]".to_string(),
            ]
        );
    }

    #[test]
    fn bearer_scheme_is_case_insensitive() {
        assert_eq!(redact_line("bearer abcdefgh1234"), "bearer [redacted]");
        assert_eq!(redact_line("BEARER abcdefgh1234"), "BEARER [redacted]");
    }

    #[test]
    fn is_char_boundary_safe_on_unicode() {
        assert_eq!(redact_line("пароль=hunter2"), "пароль=hunter2");
        assert_eq!(redact_line("password=секрет🔑xyz"), "password=[redacted]");
        assert_eq!(redact_line(""), "");
    }
}
