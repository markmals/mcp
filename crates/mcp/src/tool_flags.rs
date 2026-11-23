//! A tool's input properties as command-line flags of `mcp call`. `docs/tools.md` is the
//! contract.

use schemars::JsonSchema;
use serde::Serialize;
use serde_json::{Map, Value, json};

use crate::NAME;
use crate::failure::Failure;

/// `mcp call`'s own options. A tool flag spelled like one gets a suggestion to move it before
/// the server.
const CALL_OPTIONS: &[&str] = &["output", "input", "dry-run", "timeout"];

/// How a flag's text becomes a JSON value.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum FlagType {
    /// The text unchanged.
    String,
    /// A JSON number.
    Integer,
    /// A JSON number.
    Number,
    /// Alone, `true`; or an explicit `true` or `false`.
    Boolean,
    /// Each occurrence adds one item, converted by `itemType`.
    Array,
    /// JSON text.
    Json,
}

/// One input property, as a flag.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ToolFlag {
    /// How to write it: the property name in kebab-case, or as written when two properties
    /// share a kebab-case spelling.
    pub flag: String,
    /// The property name in the tool's inputSchema; `--<property>` is always accepted too.
    pub property: String,
    #[serde(rename = "type")]
    pub flag_type: FlagType,
    /// How each item of an `array` flag is converted; null otherwise.
    pub item_type: Option<FlagType>,
    pub required: bool,
    pub description: Option<String>,
    /// The values the property's `enum` accepts; null when it has none.
    pub accepted_values: Option<Vec<Value>>,
}

/// What the words after `<tool>` asked for.
#[derive(Debug, PartialEq)]
pub enum FlagInput {
    Arguments(Map<String, Value>),
    Help,
}

/// Every top-level property of `input_schema`, in schema order.
pub fn flags(input_schema: &Value) -> Vec<ToolFlag> {
    let properties = properties(input_schema);
    let required: Vec<&str> = input_schema
        .get("required")
        .and_then(Value::as_array)
        .map(|names| names.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    properties
        .iter()
        .map(|(property, schema)| {
            let kebab = kebab_case(property);
            let shared = properties.keys().filter(|other| kebab_case(other) == kebab).count() > 1;
            let flag_type = classify(schema);
            ToolFlag {
                flag: format!("--{}", if shared { property.clone() } else { kebab }),
                property: property.clone(),
                flag_type,
                item_type: (flag_type == FlagType::Array)
                    .then(|| schema.get("items").map_or(FlagType::Json, classify)),
                required: required.contains(&property.as_str()),
                description: schema.get("description").and_then(Value::as_str).map(str::to_string),
                accepted_values: schema.get("enum").and_then(Value::as_array).cloned(),
            }
        })
        .collect()
}

fn properties(input_schema: &Value) -> Map<String, Value> {
    input_schema.get("properties").and_then(Value::as_object).cloned().unwrap_or_default()
}

/// `filePath`, `file_path`, and `FilePath` are all `file-path`; `URLPath` is `url-path`.
pub fn kebab_case(name: &str) -> String {
    let characters: Vec<char> = name.chars().collect();
    let mut kebab = String::with_capacity(name.len() + 4);
    for (index, &character) in characters.iter().enumerate() {
        if character == '_' || character == ' ' {
            kebab.push('-');
            continue;
        }
        if character.is_uppercase() && index > 0 {
            let previous = characters[index - 1];
            let next_is_lower = characters.get(index + 1).is_some_and(|next| next.is_lowercase());
            let boundary = previous.is_lowercase()
                || previous.is_ascii_digit()
                || (previous.is_uppercase() && next_is_lower);
            if boundary && !kebab.ends_with('-') {
                kebab.push('-');
            }
        }
        kebab.extend(character.to_lowercase());
    }
    kebab
}

fn classify(schema: &Value) -> FlagType {
    let types: Vec<&str> = match schema.get("type") {
        Some(Value::String(kind)) => vec![kind.as_str()],
        Some(Value::Array(kinds)) => kinds.iter().filter_map(Value::as_str).collect(),
        _ => Vec::new(),
    };
    let strings_only_enum = schema
        .get("enum")
        .and_then(Value::as_array)
        .is_some_and(|values| !values.is_empty() && values.iter().all(Value::is_string));
    if types.contains(&"string") || (types.is_empty() && strings_only_enum) {
        return FlagType::String;
    }
    let non_null: Vec<&str> = types.into_iter().filter(|kind| *kind != "null").collect();
    match non_null.as_slice() {
        ["integer"] => FlagType::Integer,
        ["number"] => FlagType::Number,
        ["boolean"] => FlagType::Boolean,
        ["array"] => FlagType::Array,
        _ => FlagType::Json,
    }
}

/// Reads the words after `<tool>` as the tool's flags.
///
/// # Errors
///
/// `INVALID_ARGUMENT` for a word that is not a flag, an unknown or ambiguous flag, a missing or
/// malformed value, or a repeated flag whose property is not an array.
pub fn parse(input_schema: &Value, words: &[String], call: &str) -> Result<FlagInput, Failure> {
    let flags = flags(input_schema);
    let mut arguments = Map::new();
    let mut index = 0;
    while let Some(word) = words.get(index) {
        index += 1;
        if word == "--help" {
            return Ok(FlagInput::Help);
        }
        let Some(spelling) = word.strip_prefix("--").filter(|rest| !rest.is_empty()) else {
            return Err(problem(call, &flags, format!("`{word}` is not a flag of this tool")));
        };
        let (name, inline) = match spelling.split_once('=') {
            Some((name, value)) => (name, Some(value.to_string())),
            None => (spelling, None),
        };
        let flag = find(&flags, name, call)?;
        let element = match flag.flag_type {
            FlagType::Array => flag.item_type.unwrap_or(FlagType::Json),
            other => other,
        };
        let text = match inline {
            Some(text) => text,
            None if element == FlagType::Boolean => match words.get(index).map(String::as_str) {
                Some(value @ ("true" | "false")) => {
                    index += 1;
                    value.to_string()
                }
                _ => "true".to_string(),
            },
            None => {
                let value = words
                    .get(index)
                    .ok_or_else(|| problem(call, &flags, format!("{} needs a value", flag.flag)))?;
                index += 1;
                value.clone()
            }
        };
        let value = convert(element, &text).map_err(|expected| {
            problem(call, &flags, format!("{} takes {expected}, not `{text}`", flag.flag))
        })?;
        insert(&mut arguments, flag, value).map_err(|message| problem(call, &flags, message))?;
    }
    Ok(FlagInput::Arguments(arguments))
}

fn find<'a>(flags: &'a [ToolFlag], name: &str, call: &str) -> Result<&'a ToolFlag, Failure> {
    if let Some(flag) = flags.iter().find(|flag| flag.property == name) {
        return Ok(flag);
    }
    let matching: Vec<&ToolFlag> =
        flags.iter().filter(|flag| kebab_case(&flag.property) == name).collect();
    match matching.as_slice() {
        [flag] => Ok(flag),
        [] if CALL_OPTIONS.contains(&name) => {
            Err(problem(call, flags, format!("this tool has no --{name} flag")).suggest(format!(
                "--{name} is an option of `{NAME} call` itself: put it before the server, as in \
             `{NAME} call --{name} <value> <server> <tool> …`"
            )))
        }
        [] => Err(problem(call, flags, format!("this tool has no --{name} flag"))),
        _ => Err(problem(
            call,
            flags,
            format!(
                "--{name} could mean {}; use the property name as written",
                matching
                    .iter()
                    .map(|flag| format!("--{}", flag.property))
                    .collect::<Vec<_>>()
                    .join(" or ")
            ),
        )),
    }
}

/// The JSON value `text` stands for, or what the flag expected instead.
fn convert(flag_type: FlagType, text: &str) -> Result<Value, &'static str> {
    match flag_type {
        FlagType::String => Ok(Value::String(text.to_string())),
        FlagType::Integer | FlagType::Number => match serde_json::from_str::<Value>(text) {
            Ok(number @ Value::Number(_)) => Ok(number),
            _ => Err("a number"),
        },
        FlagType::Boolean => match text {
            "true" => Ok(Value::Bool(true)),
            "false" => Ok(Value::Bool(false)),
            _ => Err("`true` or `false`"),
        },
        FlagType::Array | FlagType::Json => serde_json::from_str(text).map_err(|_| "JSON"),
    }
}

fn insert(arguments: &mut Map<String, Value>, flag: &ToolFlag, value: Value) -> Result<(), String> {
    if flag.flag_type == FlagType::Array {
        match arguments.entry(flag.property.clone()).or_insert_with(|| Value::Array(Vec::new())) {
            Value::Array(items) => items.push(value),
            _ => return Err(format!("{} was given twice", flag.flag)),
        }
        return Ok(());
    }
    if arguments.insert(flag.property.clone(), value).is_some() {
        return Err(format!("{} was given more than once; it takes one value", flag.flag));
    }
    Ok(())
}

fn problem(call: &str, flags: &[ToolFlag], message: String) -> Failure {
    Failure::invalid_argument(message)
        .with_details(json!({
            "flags": flags.iter().map(|flag| flag.flag.clone()).collect::<Vec<_>>(),
        }))
        .suggest(format!("{NAME} tools info {call}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn schema() -> Value {
        json!({
            "type": "object",
            "properties": {
                "text": {"type": "string", "description": "Text"},
                "count": {"type": "integer"},
                "ratio": {"type": "number"},
                "loud": {"type": "boolean"},
                "tags": {"type": "array", "items": {"type": "string"}},
                "options": {"type": "object"},
                "mode": {"enum": ["plain", "fancy"]},
                "filePath": {"type": "string"},
                "maybe": {"type": ["string", "null"]}
            },
            "required": ["text"]
        })
    }

    fn words(text: &str) -> Vec<String> {
        text.split(' ').filter(|word| !word.is_empty()).map(str::to_string).collect()
    }

    fn arguments(text: &str) -> Value {
        match parse(&schema(), &words(text), "demo echo").unwrap() {
            FlagInput::Arguments(arguments) => Value::Object(arguments),
            FlagInput::Help => panic!("help"),
        }
    }

    fn error(schema: &Value, text: &str) -> Failure {
        parse(schema, &words(text), "demo echo").unwrap_err()
    }

    #[test]
    fn converts_each_flag_by_its_property_type() {
        assert_eq!(
            arguments(
                "--text 42 --count 3 --ratio=0.5 --loud --tags a --tags b --options {\"k\":1} --mode fancy --maybe null"
            ),
            json!({"text": "42", "count": 3, "ratio": 0.5, "loud": true, "tags": ["a", "b"], "options": {"k": 1}, "mode": "fancy", "maybe": "null"})
        );
    }

    #[test]
    fn a_boolean_takes_the_next_word_only_when_it_is_true_or_false() {
        assert_eq!(arguments("--loud false --text x"), json!({"loud": false, "text": "x"}));
        assert_eq!(arguments("--loud=true"), json!({"loud": true}));
        assert_eq!(error(&schema(), "--loud --").code, crate::failure::ErrorCode::InvalidArgument);
        assert!(error(&schema(), "--loud=yes").message.contains("`true` or `false`"));
    }

    #[test]
    fn accepts_kebab_case_and_the_property_name_as_written() {
        assert_eq!(arguments("--file-path a"), json!({"filePath": "a"}));
        assert_eq!(arguments("--filePath a"), json!({"filePath": "a"}));
    }

    #[test]
    fn a_shared_kebab_case_spelling_is_ambiguous() {
        let schema = json!({"properties": {"file_path": {"type": "string"}, "filePath": {"type": "string"}}});
        assert!(error(&schema, "--file-path x").message.contains("--file_path or --filePath"));
        let listed: Vec<String> = flags(&schema).into_iter().map(|flag| flag.flag).collect();
        assert_eq!(listed, ["--file_path", "--filePath"]);
        assert!(parse(&schema, &words("--file_path x --filePath y"), "c").is_ok());
    }

    #[test]
    fn values_that_look_like_flags_belong_to_a_string_flag() {
        assert_eq!(arguments("--text --help"), json!({"text": "--help"}));
        assert_eq!(parse(&schema(), &words("--text x --help"), "c").unwrap(), FlagInput::Help);
    }

    #[test]
    fn rejects_what_does_not_fit() {
        for (text, expected) in [
            ("--unknown 1", "no --unknown flag"),
            ("stray", "`stray` is not a flag"),
            ("--text", "needs a value"),
            ("--count three", "takes a number"),
            ("--options {", "takes JSON"),
            ("--text a --text b", "more than once"),
        ] {
            assert!(error(&schema(), text).message.contains(expected), "{text}");
        }
    }

    #[test]
    fn an_mcp_option_after_the_tool_suggests_moving_it() {
        let failure = error(&schema(), "--output text");
        assert!(
            failure.suggestions.iter().any(|suggestion| suggestion.contains("before the server"))
        );
    }

    #[test]
    fn kebab_case_splits_words() {
        for (name, kebab) in [
            ("filePath", "file-path"),
            ("file_path", "file-path"),
            ("FilePath", "file-path"),
            ("URLPath", "url-path"),
            ("page2Id", "page2-id"),
            ("url", "url"),
        ] {
            assert_eq!(kebab_case(name), kebab, "{name}");
        }
    }

    #[test]
    fn lists_flags_with_their_types_and_requirements() {
        let flags = flags(&schema());
        let text = flags.iter().find(|flag| flag.property == "text").unwrap();
        assert!(text.required && text.flag_type == FlagType::String);
        let tags = flags.iter().find(|flag| flag.property == "tags").unwrap();
        assert_eq!((tags.flag_type, tags.item_type), (FlagType::Array, Some(FlagType::String)));
        let mode = flags.iter().find(|flag| flag.property == "mode").unwrap();
        assert_eq!(mode.accepted_values, Some(vec![json!("plain"), json!("fancy")]));
        assert_eq!(
            flags.iter().find(|flag| flag.property == "maybe").unwrap().flag_type,
            FlagType::String
        );
    }
}
