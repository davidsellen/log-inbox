use regex::Regex;
use serde_json::{Map, Value};
use std::sync::OnceLock;

const REDACTED: &str = "[REDACTED]";

fn redaction_patterns() -> &'static [(Regex, &'static str)] {
    static PATTERNS: OnceLock<Vec<(Regex, &'static str)>> = OnceLock::new();
    PATTERNS.get_or_init(|| {
        vec![
            (
                Regex::new("(?i)bearer\\s+[a-z0-9._~+/=-]+").expect("valid bearer regex"),
                "Bearer [REDACTED]",
            ),
            (
                Regex::new("(?i)(api[_-]?key\\s*[=:]\\s*)[^\\s,;]+").expect("valid api key regex"),
                "${1}[REDACTED]",
            ),
            (
                Regex::new("(?i)(password\\s*=\\s*)[^;\\s]+").expect("valid password regex"),
                "${1}[REDACTED]",
            ),
            (
                Regex::new("(?i)(cookie\\s*[:=]\\s*)[^\\r\\n]+").expect("valid cookie regex"),
                "${1}[REDACTED]",
            ),
            (
                Regex::new(
                    "-----BEGIN [A-Z ]*PRIVATE KEY-----[\\s\\S]*?-----END [A-Z ]*PRIVATE KEY-----",
                )
                .expect("valid private key regex"),
                REDACTED,
            ),
        ]
    })
}

pub fn redact_text(input: &str) -> String {
    redaction_patterns()
        .iter()
        .fold(input.to_owned(), |text, (regex, replacement)| {
            regex.replace_all(&text, *replacement).into_owned()
        })
}

pub fn redact_metadata(metadata: Map<String, Value>) -> Map<String, Value> {
    metadata
        .into_iter()
        .map(|(key, value)| (key, redact_value(value)))
        .collect()
}

fn redact_value(value: Value) -> Value {
    match value {
        Value::String(text) => Value::String(redact_text(&text)),
        Value::Array(items) => Value::Array(items.into_iter().map(redact_value).collect()),
        Value::Object(map) => Value::Object(redact_metadata(map)),
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_common_secret_shapes() {
        let text = "Authorization: Bearer abc.def api_key=secret Password=hunter2;";
        let redacted = redact_text(text);

        assert!(!redacted.contains("abc.def"));
        assert!(!redacted.contains("secret"));
        assert!(!redacted.contains("hunter2"));
    }
}
