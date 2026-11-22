//! `${VAR}` and `${VAR:-default}` in configuration values.

/// A value with its variables expanded, and the values substituted into it.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct Expanded {
    pub(super) text: String,
    pub(super) substituted: Vec<String>,
}

/// Expands `text` from `env`. `${VAR:-default}` takes the default when `VAR` is unset or empty,
/// as the shell does. A `$` not followed by `{`, and a `${` with no closing `}`, are literal.
///
/// # Errors
///
/// The name of the first variable that is unset and has no default.
pub(super) fn expand(
    text: &str,
    env: &impl Fn(&str) -> Option<String>,
) -> Result<Expanded, String> {
    let mut expanded =
        Expanded { text: String::with_capacity(text.len()), substituted: Vec::new() };
    let mut rest = text;
    while let Some(start) = rest.find("${") {
        let Some(length) = rest[start + 2..].find('}') else {
            break;
        };
        expanded.text.push_str(&rest[..start]);
        let reference = &rest[start + 2..start + 2 + length];
        let (name, default) = match reference.split_once(":-") {
            Some((name, default)) => (name, Some(default)),
            None => (reference, None),
        };
        let value =
            match (env(name).filter(|value| !value.is_empty() || default.is_none()), default) {
                (Some(value), _) => value,
                (None, Some(default)) => default.to_string(),
                (None, None) => return Err(name.to_string()),
            };
        expanded.text.push_str(&value);
        expanded.substituted.push(value);
        rest = &rest[start + 3 + length..];
    }
    expanded.text.push_str(rest);
    Ok(expanded)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(key: &str) -> Option<String> {
        match key {
            "TOKEN" => Some("sesame".into()),
            "EMPTY" => Some(String::new()),
            _ => None,
        }
    }

    fn text(value: &str) -> Result<String, String> {
        expand(value, &env).map(|expanded| expanded.text)
    }

    #[test]
    fn expands_set_variables_and_records_what_it_substituted() {
        let expanded = expand("Bearer ${TOKEN}!", &env).unwrap();
        assert_eq!(expanded.text, "Bearer sesame!");
        assert_eq!(expanded.substituted, ["sesame"]);
    }

    #[test]
    fn a_default_covers_unset_and_empty_variables_like_the_shell() {
        assert_eq!(text("${MISSING:-fallback}").unwrap(), "fallback");
        assert_eq!(text("${EMPTY:-fallback}").unwrap(), "fallback");
        assert_eq!(text("${EMPTY}").unwrap(), "");
        assert_eq!(text("${TOKEN:-fallback}").unwrap(), "sesame");
    }

    #[test]
    fn an_unset_variable_without_a_default_is_named() {
        assert_eq!(text("a ${TOKEN} ${MISSING} ${ALSO}"), Err("MISSING".to_string()));
    }

    #[test]
    fn dollars_without_a_closed_brace_are_literal() {
        assert_eq!(text("$TOKEN costs $5").unwrap(), "$TOKEN costs $5");
        assert_eq!(text("${TOKEN").unwrap(), "${TOKEN");
        assert_eq!(text("!echo hi").unwrap(), "!echo hi");
    }

    #[test]
    fn expands_every_reference_in_order() {
        assert_eq!(text("${TOKEN}-${MISSING:-x}-${TOKEN}").unwrap(), "sesame-x-sesame");
    }
}
