//! Keeping configured secret values out of everything `mcp` reports, except the tool results
//! servers return.

use serde_json::Value;

use crate::failure::Failure;

pub const MASK: &str = "<redacted>";

/// `text` with every one of `secrets` replaced by [`MASK`]. `secrets` must be ordered longest
/// first, so a secret containing another is replaced whole.
pub fn text(text: &str, secrets: &[String]) -> String {
    secrets.iter().fold(text.to_string(), |text, secret| text.replace(secret.as_str(), MASK))
}

/// `value` with every string, and every object key, redacted.
pub fn value(value: &Value, secrets: &[String]) -> Value {
    match value {
        Value::String(string) => Value::String(text(string, secrets)),
        Value::Array(items) => {
            Value::Array(items.iter().map(|item| self::value(item, secrets)).collect())
        }
        Value::Object(fields) => Value::Object(
            fields
                .iter()
                .map(|(key, item)| (text(key, secrets), self::value(item, secrets)))
                .collect(),
        ),
        other => other.clone(),
    }
}

pub fn failure(failure: Failure, secrets: &[String]) -> Failure {
    Failure {
        code: failure.code,
        message: text(&failure.message, secrets),
        details: Box::new(value(&failure.details, secrets)),
        suggestions: failure
            .suggestions
            .iter()
            .map(|suggestion| text(suggestion, secrets))
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn replaces_every_occurrence_in_messages_details_and_suggestions() {
        let secrets = vec!["token-123456".to_string()];
        let redacted = failure(
            Failure::unavailable("bad token-123456")
                .with_details(json!({"stderr": ["x token-123456 y"], "token-123456": 1}))
                .suggest("retry token-123456"),
            &secrets,
        );
        let rendered = redacted.to_json().to_string();
        assert!(!rendered.contains("token-123456"), "{rendered}");
        assert_eq!(redacted.message, "bad <redacted>");
    }

    #[test]
    fn a_longer_secret_is_replaced_whole() {
        let secrets = vec!["abcdefgh-ijkl".to_string(), "abcdefgh".to_string()];
        assert_eq!(text("abcdefgh-ijkl and abcdefgh", &secrets), "<redacted> and <redacted>");
    }
}
