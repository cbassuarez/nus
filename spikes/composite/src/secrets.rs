//! Best-effort local DLP for complete text payloads, never the live terminal or
//! editor buffer. No secrets or matching excerpts are logged or retained.
use regex::Regex;
use std::sync::OnceLock;

pub const MASK: &str = "[!SECRET!]";
const LIMIT: usize = 8 * 1024 * 1024;

pub struct Scrubbed {
    pub text: String,
    pub findings: usize,
}

fn patterns() -> &'static Vec<Regex> {
    static PATTERNS: OnceLock<Vec<Regex>> = OnceLock::new();
    PATTERNS.get_or_init(|| [
        r"(?s)-----BEGIN (?:[A-Z0-9]+ )*PRIVATE KEY-----.*?(?:-----END (?:[A-Z0-9]+ )*PRIVATE KEY-----|\z)",
        r"\b(?:sk-(?:proj-|ant-api\d+-)?[A-Za-z0-9_-]{16,}|gh[pousr]_[A-Za-z0-9]{20,}|github_pat_[A-Za-z0-9_]{20,}|xox[baprs]-[A-Za-z0-9-]{16,}|AKIA[A-Z0-9]{16}|ASIA[A-Z0-9]{16}|AIza[A-Za-z0-9_-]{30,})\b",
        r"\beyJ[A-Za-z0-9_-]{5,}\.[A-Za-z0-9_-]{5,}\.[A-Za-z0-9_-]{8,}\b",
        r#"(?im)(?:[\w.-]*(?:api[_-]?key|access[_-]?key|secret|token|password|passwd|credential|connection[_-]?string|private[_-]?key)[\w.-]*|pwd)["']?\s*[:=]\s*"(?P<secret>(?:\\.|[^"\\])*)""#,
        r#"(?im)(?:[\w.-]*(?:api[_-]?key|access[_-]?key|secret|token|password|passwd|credential|connection[_-]?string|private[_-]?key)[\w.-]*|pwd)["']?\s*[:=]\s*'(?P<secret>(?:\\.|[^'\\])*)'"#,
        r#"(?im)(?:[\w.-]*(?:api[_-]?key|access[_-]?key|secret|token|password|passwd|credential|connection[_-]?string|private[_-]?key)[\w.-]*|pwd)["']?\s*[:=]\s*["']?(?P<secret>[^\s"'`,;}{<>]+)"#,
        r#"(?i)\b(?:authorization|proxy-authorization)["']?\s*[:=]\s*["']?(?:bearer|basic)\s+(?P<secret>[^\s"'<>]+)"#,
        r#"(?i)\b[a-z][a-z0-9+.-]*://[^\s/@:]+:(?P<secret>[^\s/@]+)@"#,
    ].iter().map(|p| Regex::new(p).expect("constant secret pattern")).collect())
}

fn high_entropy(word: &str) -> bool {
    if word.len() < 24 || word == MASK {
        return false;
    }
    let bytes = word.as_bytes();
    let mut counts = [0usize; 128];
    for &b in bytes {
        if !b.is_ascii_alphanumeric() && !b"_+/=-".contains(&b) {
            return false;
        }
        counts[b as usize] += 1;
    }
    // Ordinary identifiers and SHA checksums are too ambiguous. Detect mixed
    // case/base64 credentials; labelled hex secrets are handled above.
    if !bytes.iter().any(u8::is_ascii_uppercase)
        || !bytes.iter().any(u8::is_ascii_lowercase)
        || !bytes.iter().any(u8::is_ascii_digit)
    {
        return false;
    }
    let len = bytes.len() as f64;
    counts
        .into_iter()
        .filter(|n| *n > 0)
        .map(|n| {
            let p = n as f64 / len;
            -p * p.log2()
        })
        .sum::<f64>()
        >= 4.3
}

pub fn scrub(input: &str) -> Scrubbed {
    if input.len() > LIMIT {
        return Scrubbed {
            text: "[!SECRET! payload exceeds local scanning limit]".into(),
            findings: 1,
        };
    }
    static ANSI: OnceLock<Regex> = OnceLock::new();
    let text = ANSI
        .get_or_init(|| {
            Regex::new(r"\x1b(?:\[[0-?]*[ -/]*[@-~]|\][^\x07\x1b]*(?:\x07|\x1b\\))").unwrap()
        })
        .replace_all(input, "");
    let mut spans = Vec::new();
    for pattern in patterns() {
        for matched in pattern.captures_iter(&text) {
            let m = matched
                .name("secret")
                .unwrap_or_else(|| matched.get(0).unwrap());
            if m.as_str() != MASK {
                spans.push((m.start(), m.end()));
            }
        }
    }
    let mut start = None;
    for (i, ch) in text
        .char_indices()
        .chain(std::iter::once((text.len(), ' ')))
    {
        if ch.is_ascii_alphanumeric() || "_+/=-".contains(ch) {
            start.get_or_insert(i);
        } else if let Some(begin) = start.take() {
            if high_entropy(&text[begin..i]) {
                spans.push((begin, i));
            }
        }
    }
    spans.sort_unstable();
    let mut merged: Vec<(usize, usize)> = Vec::new();
    for (start, end) in spans {
        match merged.last_mut() {
            Some(last) if start <= last.1 => last.1 = last.1.max(end),
            _ => merged.push((start, end)),
        }
    }
    let mut out = String::with_capacity(text.len());
    let mut at = 0;
    for &(start, end) in &merged {
        out.push_str(&text[at..start]);
        out.push_str(MASK);
        at = end;
    }
    out.push_str(&text[at..]);
    Scrubbed {
        text: out,
        findings: merged.len(),
    }
}

/// Redact structured response values too: a JSON field supplies context that
/// scanning its value alone would miss. Keep protocol shapes and numbers.
pub fn scrub_json(value: &mut serde_json::Value) -> usize {
    match value {
        serde_json::Value::String(s) => {
            let clean = scrub(s);
            *s = clean.text;
            clean.findings
        }
        serde_json::Value::Array(a) => a.iter_mut().map(scrub_json).sum(),
        serde_json::Value::Object(o) => o
            .iter_mut()
            .map(|(key, value)| {
                let probe = scrub(&format!("{key}=example-value"));
                let authorization = key.eq_ignore_ascii_case("authorization")
                    || key.eq_ignore_ascii_case("proxy-authorization");
                if (probe.findings > 0 || authorization) && !value.is_null() {
                    *value = serde_json::Value::String(MASK.into());
                    1
                } else {
                    scrub_json(value)
                }
            })
            .sum(),
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn authorization_headers_are_redacted_in_json_and_quoted_logs() {
        for input in [r#"{"Authorization": "Bearer short-session"}"#, "'Proxy-Authorization': 'Basic dXNlcjpwYXNz'"] {
            let clean = scrub(input);
            assert_eq!(clean.findings, 1);
            assert!(!clean.text.contains("short-session"));
            assert!(!clean.text.contains("dXNlcjpwYXNz"));
        }
        let mut value = serde_json::json!({"headers":{"Authorization":"Bearer short-session","Proxy-Authorization":"Basic dXNlcjpwYXNz"}, "body":"public text"});
        assert_eq!(scrub_json(&mut value), 2);
        assert_eq!(value["headers"]["Authorization"], MASK);
        assert_eq!(value["headers"]["Proxy-Authorization"], MASK);
        assert_eq!(value["body"], "public text");
    }
    #[test]
    fn protects_context_credentials_and_unicode_without_mutating_sources() {
        let input="café 🔑\nAPI_KEY='simple-secret'\nAuthorization: Bearer session-value\npostgres://alice:pass123@localhost/db\nordinary source code";
        let clean = scrub(input);
        assert_eq!(clean.findings, 3);
        for secret in ["simple-secret", "session-value", "pass123"] {
            assert!(!clean.text.contains(secret));
        }
        assert!(clean.text.contains("café 🔑") && clean.text.contains("ordinary source code"));
        assert!(input.contains("simple-secret"));
        assert_eq!(scrub(&clean.text).text, clean.text);
    }
    #[test]
    fn catches_multiline_keys_provider_tokens_ansi_and_entropy() {
        for input in [
            "-----BEGIN PRIVATE KEY-----\nabc\ndef\n-----END PRIVATE KEY-----",
            "sk-proj-abcdefghijklmnopqrstuvwxyz1234567890",
            "ghp_abcdefghijklmnopqrstuvwxyz1234567890",
            "eyJhbGciOiJIUzI1NiJ9.eyJ1c2VyIjoiYWxpY2UifQ.signature12345678",
            "secret=\x1b[31mredacted-value\x1b[0m",
            "aG7pQ2uK9wX4eT6yI8oP1sD3fH5jL0zV",
        ] {
            assert!(scrub(input).text.contains(MASK), "fixture not detected");
        }
        assert_eq!(scrub("let counter = 123; a_very_long_descriptive_identifier; 0123456789abcdef0123456789abcdef").findings,0);
    }
    #[test]
    fn quoted_passwords_are_redacted_in_full() {
        for input in [
            r#"password="several secret words""#,
            "token='two words'",
            r#"{"password":"with \"quotes\" and spaces"}"#,
        ] {
            let clean = scrub(input);
            assert!(
                !clean.text.contains("words") && !clean.text.contains("spaces"),
                "quoted secret leaked"
            );
        }
    }
    #[test]
    fn json_labels_and_large_payloads_fail_closed() {
        let mut v = serde_json::json!({"password":"short", "output":"token=abcd", "count":4});
        assert_eq!(scrub_json(&mut v), 2);
        assert_eq!(v["password"], MASK);
        assert_eq!(v["count"], 4);
        assert_eq!(scrub(&"x".repeat(LIMIT + 1)).findings, 1);
    }
}
