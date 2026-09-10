//! Minimal bounded JSON parser for Claude CLI result objects
//! (TASK-0046, G5).
//!
//! The repository carries no `serde` dependency (and the task forbids new
//! dependencies without evidence), so the adapter parses the small,
//! known-shape `--output-format json` result with a purpose-built
//! recursive-descent parser instead of a general data-binding framework.
//!
//! Contract: total, non-panicking, forward-compatible. Unknown fields are
//! preserved in the value tree and ignored by field extractors; malformed
//! input returns a typed error (never a panic, never partial trust).
//! Depth is capped (32), input size is capped by the caller (capture
//! bound), and invalid `\u` escapes decode lossily to U+FFFD rather than
//! failing the whole document.

/// Maximum object/array nesting depth.
const MAX_DEPTH: usize = 32;

/// A JSON value. Objects keep insertion order as pairs (no hashing
/// needed for a handful of known fields).
#[derive(Debug, Clone, PartialEq)]
pub enum Json {
    /// JSON null.
    Null,
    /// JSON boolean.
    Bool(bool),
    /// JSON number as f64 (integers round-trip exactly up to 2^53).
    Num(f64),
    /// JSON string (unescaped).
    Str(String),
    /// JSON array.
    Arr(Vec<Json>),
    /// JSON object.
    Obj(Vec<(String, Json)>),
}

impl Json {
    /// Looks up `key` in an object. `None` for non-objects and absent keys.
    #[must_use]
    pub fn get(&self, key: &str) -> Option<&Json> {
        match self {
            Self::Obj(pairs) => pairs.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    /// Borrows the string value, if this is one.
    #[must_use]
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::Str(s) => Some(s),
            _ => None,
        }
    }

    /// Reads the boolean value, if this is one.
    #[must_use]
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Self::Bool(b) => Some(*b),
            _ => None,
        }
    }

    /// Reads an unsigned integer value, if this is a finite non-negative
    /// whole number that fits. Token counts arrive as plain integers.
    #[must_use]
    pub fn as_u64(&self) -> Option<u64> {
        match self {
            Self::Num(n) => {
                if n.is_finite() && *n >= 0.0 && n.fract() == 0.0 && *n <= u64::MAX as f64 {
                    Some(*n as u64)
                } else {
                    None
                }
            }
            _ => None,
        }
    }
}

/// Parser failure: input is not usable JSON.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JsonError(String);

impl JsonError {
    fn at(message: impl Into<String>, position: usize) -> Self {
        Self(format!("{} at byte {position}", message.into()))
    }
}

impl std::fmt::Display for JsonError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "invalid JSON: {}", self.0)
    }
}

impl std::error::Error for JsonError {}

/// Parses one complete JSON document. Trailing garbage after the first
/// value is an error (a result object must be exactly one value).
pub fn parse_json(input: &str) -> Result<Json, JsonError> {
    let mut parser = Parser {
        bytes: input.as_bytes(),
        pos: 0,
    };
    parser.skip_ws();
    let value = parser.value(0)?;
    parser.skip_ws();
    if parser.pos != parser.bytes.len() {
        return Err(JsonError::at("trailing data after JSON value", parser.pos));
    }
    Ok(value)
}

struct Parser<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl Parser<'_> {
    fn skip_ws(&mut self) {
        while self.pos < self.bytes.len()
            && matches!(self.bytes[self.pos], b' ' | b'\t' | b'\n' | b'\r')
        {
            self.pos += 1;
        }
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.pos).copied()
    }

    fn value(&mut self, depth: usize) -> Result<Json, JsonError> {
        if depth > MAX_DEPTH {
            return Err(JsonError::at("nesting exceeds bound", self.pos));
        }
        match self.peek() {
            Some(b'{') => self.object(depth),
            Some(b'[') => self.array(depth),
            Some(b'"') => Ok(Json::Str(self.string()?)),
            Some(b't') => self.literal("true", Json::Bool(true)),
            Some(b'f') => self.literal("false", Json::Bool(false)),
            Some(b'n') => self.literal("null", Json::Null),
            Some(c) if c == b'-' || c.is_ascii_digit() => self.number(),
            _ => Err(JsonError::at("unexpected character", self.pos)),
        }
    }

    fn literal(&mut self, word: &str, value: Json) -> Result<Json, JsonError> {
        if self.bytes[self.pos..].starts_with(word.as_bytes()) {
            self.pos += word.len();
            Ok(value)
        } else {
            Err(JsonError::at("invalid literal", self.pos))
        }
    }

    fn object(&mut self, depth: usize) -> Result<Json, JsonError> {
        self.pos += 1; // '{'
        let mut pairs = Vec::new();
        self.skip_ws();
        if self.peek() == Some(b'}') {
            self.pos += 1;
            return Ok(Json::Obj(pairs));
        }
        loop {
            self.skip_ws();
            if self.peek() != Some(b'"') {
                return Err(JsonError::at("object key must be a string", self.pos));
            }
            let key = self.string()?;
            self.skip_ws();
            if self.peek() != Some(b':') {
                return Err(JsonError::at("object key without ':'", self.pos));
            }
            self.pos += 1;
            self.skip_ws();
            let value = self.value(depth + 1)?;
            pairs.push((key, value));
            self.skip_ws();
            match self.peek() {
                Some(b',') => {
                    self.pos += 1;
                }
                Some(b'}') => {
                    self.pos += 1;
                    return Ok(Json::Obj(pairs));
                }
                _ => return Err(JsonError::at("expected ',' or '}'", self.pos)),
            }
        }
    }

    fn array(&mut self, depth: usize) -> Result<Json, JsonError> {
        self.pos += 1; // '['
        let mut items = Vec::new();
        self.skip_ws();
        if self.peek() == Some(b']') {
            self.pos += 1;
            return Ok(Json::Arr(items));
        }
        loop {
            self.skip_ws();
            items.push(self.value(depth + 1)?);
            self.skip_ws();
            match self.peek() {
                Some(b',') => {
                    self.pos += 1;
                }
                Some(b']') => {
                    self.pos += 1;
                    return Ok(Json::Arr(items));
                }
                _ => return Err(JsonError::at("expected ',' or ']'", self.pos)),
            }
        }
    }

    fn number(&mut self) -> Result<Json, JsonError> {
        let start = self.pos;
        if self.peek() == Some(b'-') {
            self.pos += 1;
        }
        let int_start = self.pos;
        while self.peek().is_some_and(|c| c.is_ascii_digit()) {
            self.pos += 1;
        }
        if self.pos == int_start {
            return Err(JsonError::at("invalid number", start));
        }
        if self.bytes[int_start] == b'0' && self.pos - int_start > 1 {
            return Err(JsonError::at("leading zeros are not valid JSON", start));
        }
        if self.peek() == Some(b'.') {
            self.pos += 1;
            let frac_start = self.pos;
            while self.peek().is_some_and(|c| c.is_ascii_digit()) {
                self.pos += 1;
            }
            if self.pos == frac_start {
                return Err(JsonError::at("invalid number fraction", start));
            }
        }
        if matches!(self.peek(), Some(b'e') | Some(b'E')) {
            self.pos += 1;
            if matches!(self.peek(), Some(b'+') | Some(b'-')) {
                self.pos += 1;
            }
            let exp_start = self.pos;
            while self.peek().is_some_and(|c| c.is_ascii_digit()) {
                self.pos += 1;
            }
            if self.pos == exp_start {
                return Err(JsonError::at("invalid number exponent", start));
            }
        }
        let text = std::str::from_utf8(&self.bytes[start..self.pos])
            .map_err(|_| JsonError::at("invalid number encoding", start))?;
        text.parse::<f64>()
            .map(Json::Num)
            .map_err(|_| JsonError::at("number out of range", start))
    }

    fn string(&mut self) -> Result<String, JsonError> {
        debug_assert_eq!(self.peek(), Some(b'"'));
        self.pos += 1;
        let mut out = String::new();
        loop {
            let byte = match self.bytes.get(self.pos) {
                Some(b) => *b,
                None => return Err(JsonError::at("unterminated string", self.pos)),
            };
            match byte {
                b'"' => {
                    self.pos += 1;
                    return Ok(out);
                }
                b'\\' => {
                    self.pos += 1;
                    self.escape_into(&mut out)?;
                }
                0x00..=0x1F => {
                    return Err(JsonError::at("unescaped control character", self.pos));
                }
                _ => {
                    // Copy one full UTF-8 sequence through verbatim.
                    let rest = &self.bytes[self.pos..];
                    let text = std::str::from_utf8(rest)
                        .map_err(|_| JsonError::at("invalid UTF-8 in string", self.pos))?;
                    let ch = text
                        .chars()
                        .next()
                        .ok_or_else(|| JsonError::at("invalid UTF-8 in string", self.pos))?;
                    out.push(ch);
                    self.pos += ch.len_utf8();
                }
            }
        }
    }

    fn escape_into(&mut self, out: &mut String) -> Result<(), JsonError> {
        let byte = self
            .bytes
            .get(self.pos)
            .copied()
            .ok_or_else(|| JsonError::at("unterminated escape", self.pos))?;
        self.pos += 1;
        match byte {
            b'"' => out.push('"'),
            b'\\' => out.push('\\'),
            b'/' => out.push('/'),
            b'b' => out.push('\u{0008}'),
            b'f' => out.push('\u{000C}'),
            b'n' => out.push('\n'),
            b'r' => out.push('\r'),
            b't' => out.push('\t'),
            b'u' => {
                let high = self.hex4()?;
                if (0xD800..0xDC00).contains(&high) {
                    // Surrogate pair: must be followed by a low surrogate.
                    if self.bytes.get(self.pos) == Some(&b'\\')
                        && self.bytes.get(self.pos + 1) == Some(&b'u')
                    {
                        self.pos += 2;
                        let low = self.hex4()?;
                        if (0xDC00..0xE000).contains(&low) {
                            let scalar = 0x10000 + ((high - 0xD800) << 10) + (low - 0xDC00);
                            out.push(char::from_u32(scalar).unwrap_or('\u{FFFD}'));
                        } else {
                            out.push('\u{FFFD}');
                        }
                    } else {
                        out.push('\u{FFFD}');
                    }
                } else if (0xDC00..0xE000).contains(&high) {
                    out.push('\u{FFFD}');
                } else {
                    out.push(char::from_u32(high).unwrap_or('\u{FFFD}'));
                }
            }
            _ => return Err(JsonError::at("invalid escape", self.pos)),
        }
        Ok(())
    }

    fn hex4(&mut self) -> Result<u32, JsonError> {
        if self.pos + 4 > self.bytes.len() {
            return Err(JsonError::at("truncated \\u escape", self.pos));
        }
        let text = std::str::from_utf8(&self.bytes[self.pos..self.pos + 4])
            .map_err(|_| JsonError::at("invalid \\u escape", self.pos))?;
        let value = u32::from_str_radix(text, 16)
            .map_err(|_| JsonError::at("invalid \\u escape", self.pos))?;
        self.pos += 4;
        Ok(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_result_shaped_objects_with_unknown_fields_ignored() {
        let doc = r#"{"type":"result","subtype":"success","is_error":false,
            "result":"did it","session_id":"abc","usage":{"input_tokens":12,"output_tokens":34},
            "future_field":{"nested":[1,2,{"x":null}]},"total_cost_usd":0.001}"#;
        let json = parse_json(doc).unwrap();
        assert_eq!(json.get("type").and_then(|v| v.as_str()), Some("result"));
        assert_eq!(json.get("is_error").and_then(|v| v.as_bool()), Some(false));
        assert_eq!(json.get("result").and_then(|v| v.as_str()), Some("did it"));
        let usage = json.get("usage").unwrap();
        assert_eq!(usage.get("input_tokens").and_then(|v| v.as_u64()), Some(12));
        assert_eq!(
            usage.get("output_tokens").and_then(|v| v.as_u64()),
            Some(34)
        );
        // Unknown future fields parse but are simply never queried.
        assert!(json.get("future_field").is_some());
    }

    #[test]
    fn rejects_malformed_documents_explicitly() {
        for bad in [
            "",
            "   ",
            "{",
            "{\"a\":}",
            "{\"a\":1,}",
            "[1,]",
            "{\"a\" 1}",
            "garbage",
            "{\"a\":\"unterminated}",
            "{\"a\":\"bad \\x escape\"}",
            "{\"a\":01}",
            "{\"a\":1} trailing",
            "{\"a\":\"ctrl\x01char\"}",
        ] {
            assert!(parse_json(bad).is_err(), "{bad:?} must not parse");
        }
    }

    #[test]
    fn string_escapes_decode_including_surrogate_pairs() {
        let json = parse_json(r#"{"a":"x\ny\t\u0041\ud83d\ude00\ud800"}"#).unwrap();
        assert_eq!(json.get("a").and_then(|v| v.as_str()), Some("x\ny\tA😀�"));
    }

    #[test]
    fn depth_is_bounded() {
        let deep = "[".repeat(40) + &"]".repeat(40);
        assert!(parse_json(&deep).is_err());
        let shallow = "[".repeat(8) + &"]".repeat(8);
        assert!(parse_json(&shallow).is_ok());
    }

    #[test]
    fn numbers_classify_for_token_counts() {
        let json = parse_json(r#"{"int":12,"neg":-1,"frac":1.5,"exp":1e3,"big":1e30}"#).unwrap();
        assert_eq!(json.get("int").and_then(|v| v.as_u64()), Some(12));
        assert_eq!(json.get("neg").and_then(|v| v.as_u64()), None);
        assert_eq!(json.get("frac").and_then(|v| v.as_u64()), None);
        assert_eq!(json.get("exp").and_then(|v| v.as_u64()), Some(1000));
        assert_eq!(json.get("big").and_then(|v| v.as_u64()), None);
    }

    #[test]
    fn unicode_passthrough_and_empty_shapes() {
        let json = parse_json(r#"{"emoji":"héllo 😀","empty_obj":{},"empty_arr":[]}"#).unwrap();
        assert_eq!(json.get("emoji").and_then(|v| v.as_str()), Some("héllo 😀"));
    }
}
