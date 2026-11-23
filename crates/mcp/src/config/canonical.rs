//! A server entry's fingerprint: what an approval is bound to.

use serde_json::Value;
use sha2::{Digest, Sha256};

/// `sha256:` and the hex digest of `written` in canonical form: object keys sorted, no
/// insignificant whitespace. Reordering keys or reformatting the file keeps the fingerprint;
/// changing any value or `${VAR}` reference changes it.
pub(super) fn fingerprint(written: &Value) -> String {
    let mut canonical = String::new();
    write_canonical(written, &mut canonical);
    let digest = Sha256::digest(canonical.as_bytes());
    let hex: String = digest.iter().map(|byte| format!("{byte:02x}")).collect();
    format!("sha256:{hex}")
}

fn write_canonical(value: &Value, out: &mut String) {
    match value {
        Value::Object(fields) => {
            let mut keys: Vec<&String> = fields.keys().collect();
            keys.sort();
            out.push('{');
            for (index, key) in keys.into_iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                out.push_str(&Value::String(key.clone()).to_string());
                out.push(':');
                write_canonical(&fields[key], out);
            }
            out.push('}');
        }
        Value::Array(items) => {
            out.push('[');
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                write_canonical(item, out);
            }
            out.push(']');
        }
        scalar => out.push_str(&scalar.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn ignores_key_order_and_formatting() {
        let a: Value =
            serde_json::from_str(r#"{"command":"npx","env":{"B":"2","A":"1"}}"#).unwrap();
        let b: Value = serde_json::from_str(
            "{\n  \"env\": { \"A\": \"1\", \"B\": \"2\" },\n  \"command\": \"npx\"\n}",
        )
        .unwrap();
        assert_eq!(fingerprint(&a), fingerprint(&b));
        assert!(fingerprint(&a).starts_with("sha256:"));
        assert_eq!(fingerprint(&a).len(), "sha256:".len() + 64);
    }

    #[test]
    fn changes_with_any_value_or_reference() {
        let base = json!({"command": "npx", "args": ["a"], "env": {"T": "${TOKEN}"}});
        for changed in [
            json!({"command": "npx", "args": ["b"], "env": {"T": "${TOKEN}"}}),
            json!({"command": "npx", "args": ["a"], "env": {"T": "${OTHER}"}}),
            json!({"command": "uvx", "args": ["a"], "env": {"T": "${TOKEN}"}}),
            json!({"command": "npx", "args": ["a", "a"], "env": {"T": "${TOKEN}"}}),
        ] {
            assert_ne!(fingerprint(&base), fingerprint(&changed), "{changed}");
        }
    }
}
