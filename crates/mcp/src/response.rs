//! What a command hands back: its result, a text rendering for humans, and stderr warnings.

use serde::Serialize;
use serde_json::Value;

/// A rendering of a result for `--output text`.
pub trait RenderText {
    fn render_text(&self) -> String;
}

/// A successful command's outcome. The result goes to stdout; warnings go to stderr.
#[derive(Debug)]
pub struct Response {
    pub result: Value,
    pub text: String,
    /// Diagnostics for stderr, such as a store that does not exist yet.
    pub warnings: Vec<String>,
}

impl Response {
    /// # Panics
    ///
    /// When `result` fails to serialize, which the derived `Serialize` of this crate's result
    /// types never does.
    pub fn new<T: Serialize + RenderText>(result: &T) -> Response {
        Response {
            result: serde_json::to_value(result).expect("command results serialize to JSON"),
            text: result.render_text(),
            warnings: Vec::new(),
        }
    }

    #[must_use]
    pub fn warn(mut self, warning: impl Into<String>) -> Response {
        self.warnings.push(warning.into());
        self
    }
}
