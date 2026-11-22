//! The input `mcp call` sends: `--input`'s object with the tool's flags over it, checked against
//! the tool's inputSchema.

use schemars::JsonSchema;
use serde::Serialize;
use serde_json::{Map, Value, json};

use crate::NAME;
use crate::failure::Failure;
use crate::tool_flags::{ToolFlag, flags};

/// Whether `mcp` checked the input before sending it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct InputValidation {
    /// `passed`, or `skipped` when mcp could not compile the tool's inputSchema and sent the
    /// input unchecked.
    pub status: ValidationStatus,
    /// Why validation was skipped; null when it passed.
    pub reason: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum ValidationStatus {
    Passed,
    Skipped,
}

/// `--input`'s text as a JSON object; an absent `--input` is `{}`.
///
/// # Errors
///
/// `INVALID_ARGUMENT` when the text is not a JSON object.
pub fn base_object(text: Option<&str>) -> Result<Map<String, Value>, Failure> {
    let Some(text) = text else {
        return Ok(Map::new());
    };
    match serde_json::from_str::<Value>(text) {
        Ok(Value::Object(object)) => Ok(object),
        Ok(_) => Err(Failure::invalid_argument("--input must be a JSON object")
            .with_details(json!({ "argument": "input" }))),
        Err(error) => Err(Failure::invalid_argument(format!("--input is not valid JSON: {error}"))
            .with_details(json!({ "argument": "input" }))),
    }
}

/// Checks `arguments` against `input_schema`.
///
/// # Errors
///
/// `INVALID_ARGUMENT` listing every violation, with the flag each one concerns, and the
/// required properties that are missing, as flags.
pub fn validate(
    input_schema: &Value,
    arguments: &Map<String, Value>,
    call: &str,
) -> Result<InputValidation, Failure> {
    let validator = match jsonschema::validator_for(input_schema) {
        Ok(validator) => validator,
        Err(error) => {
            return Ok(InputValidation {
                status: ValidationStatus::Skipped,
                reason: Some(format!("mcp cannot compile the tool's inputSchema: {error}")),
            });
        }
    };
    let instance = Value::Object(arguments.clone());
    let flags = flags(input_schema);
    let violations: Vec<Value> = validator
        .iter_errors(&instance)
        .map(|error| {
            let path = error.instance_path().to_string();
            // A missing required property is reported at its parent; name the property's flag.
            let property = match error.kind() {
                jsonschema::error::ValidationErrorKind::Required { property }
                    if path.is_empty() =>
                {
                    property.as_str().map(|name| format!("/{name}"))
                }
                _ => None,
            };
            json!({
                "instancePath": path,
                "keyword": error.kind().keyword(),
                "message": error.to_string(),
                "flag": flag_for(&flags, property.as_deref().unwrap_or(&path)),
            })
        })
        .collect();
    if violations.is_empty() {
        return Ok(InputValidation { status: ValidationStatus::Passed, reason: None });
    }
    let missing: Vec<String> = flags
        .iter()
        .filter(|flag| flag.required && !arguments.contains_key(&flag.property))
        .map(|flag| flag.flag.clone())
        .collect();
    let first = violations[0]["message"].as_str().unwrap_or_default().to_string();
    let message = if missing.is_empty() {
        format!("the input does not fit the tool's inputSchema: {first}")
    } else {
        format!("the tool requires {}", missing.join(", "))
    };
    Err(Failure::invalid_argument(message)
        .with_details(json!({ "violations": violations, "missing": missing }))
        .suggest(format!("{NAME} tools info {call}")))
}

/// The flag for the top-level property an instance path starts with.
fn flag_for(flags: &[ToolFlag], instance_path: &str) -> Option<String> {
    let property =
        instance_path.strip_prefix('/')?.split('/').next()?.replace("~1", "/").replace("~0", "~");
    flags.iter().find(|flag| flag.property == property).map(|flag| flag.flag.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn schema() -> Value {
        json!({
            "type": "object",
            "properties": {"text": {"type": "string"}, "filePath": {"type": "string", "maxLength": 3}},
            "required": ["text"]
        })
    }

    fn object(value: Value) -> Map<String, Value> {
        value.as_object().unwrap().clone()
    }

    #[test]
    fn passes_input_that_fits() {
        let validation = validate(&schema(), &object(json!({"text": "hi"})), "demo echo").unwrap();
        assert_eq!(validation.status, ValidationStatus::Passed);
    }

    #[test]
    fn names_missing_required_properties_as_flags() {
        let failure = validate(&schema(), &Map::new(), "demo echo").unwrap_err();
        assert_eq!(failure.details["missing"], json!(["--text"]));
        assert_eq!(failure.details["violations"][0]["keyword"], "required");
        assert_eq!(failure.details["violations"][0]["flag"], "--text");
        assert_eq!(failure.suggestions, [format!("{NAME} tools info demo echo")]);
    }

    #[test]
    fn ties_each_violation_to_its_flag() {
        let failure = validate(&schema(), &object(json!({"text": "a", "filePath": "long"})), "c")
            .unwrap_err();
        let violation = &failure.details["violations"][0];
        assert_eq!(violation["instancePath"], "/filePath");
        assert_eq!(violation["flag"], "--file-path");
        assert_eq!(violation["keyword"], "maxLength");
    }

    #[test]
    fn skips_a_schema_it_cannot_compile() {
        let broken = json!({"type": "object", "properties": {"x": {"$ref": "#/$defs/missing"}}});
        let validation = validate(&broken, &Map::new(), "c").unwrap();
        assert_eq!(validation.status, ValidationStatus::Skipped);
        assert!(validation.reason.is_some());
    }

    #[test]
    fn input_must_be_an_object() {
        assert!(base_object(None).unwrap().is_empty());
        assert_eq!(base_object(Some(r#"{"a":1}"#)).unwrap()["a"], 1);
        assert!(base_object(Some("[1]")).is_err());
        assert!(base_object(Some("{")).is_err());
    }
}
