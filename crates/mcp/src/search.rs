//! What `cli search` and `docs search` share: query words and bounded, resumable pages.

use schemars::JsonSchema;
use serde::Serialize;
use serde_json::json;

use crate::failure::Failure;

const STOP_WORDS: &[&str] = &[
    "a", "an", "and", "are", "as", "at", "be", "by", "can", "do", "does", "for", "from", "how",
    "i", "in", "into", "is", "it", "me", "my", "of", "on", "or", "the", "this", "that", "to",
    "what", "when", "where", "which", "with",
];

/// The lowercase words of `text` that carry meaning for a search.
pub fn query_words(text: &str) -> Vec<String> {
    let mut words: Vec<String> =
        words(text).into_iter().filter(|word| !STOP_WORDS.contains(&word.as_str())).collect();
    words.dedup();
    words
}

/// Every lowercase word of `text`, stop words included.
pub fn words(text: &str) -> Vec<String> {
    text.to_lowercase()
        .split(|character: char| !character.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .map(str::to_string)
        .collect()
}

/// How well `query` matches `word`: 1 for the same word, 0.6 for the same stem (`note` and
/// `notes`, `migrate` and `migration`), else 0. Two words share a stem when they share a prefix
/// of at least four letters that leaves at most two letters of the shorter word and at most
/// four of the longer one.
pub fn word_match(query: &str, word: &str) -> f64 {
    if query == word {
        return 1.0;
    }
    let common = query.chars().zip(word.chars()).take_while(|(left, right)| left == right).count();
    let (shorter, longer) = if query.len() <= word.len() { (query, word) } else { (word, query) };
    let shorter = shorter.chars().count();
    let longer = longer.chars().count();
    if common >= 4 && shorter - common <= 2 && longer - common <= 4 { 0.6 } else { 0.0 }
}

/// The best match of `query` against any of `words`.
pub fn best_match(query: &str, words: &[String]) -> f64 {
    words.iter().map(|word| word_match(query, word)).fold(0.0, f64::max)
}

/// Rounds a relevance score to three decimals for output.
pub fn rounded(score: f64) -> f64 {
    (score * 1000.0).round() / 1000.0
}

/// One page of a collection.
#[derive(Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Page<T> {
    pub items: Vec<T>,
    /// Whether more items follow this page.
    pub has_more: bool,
    /// Pass as `--cursor` to get the next page; null when there is none.
    pub next_cursor: Option<String>,
}

/// Slices `ranked` from `cursor` (an offset from a previous `nextCursor`) to at most `limit`
/// items.
///
/// # Errors
///
/// `INVALID_ARGUMENT` when `cursor` is not an offset this function issued: every one it issues
/// falls inside `ranked`.
pub fn page<T>(ranked: Vec<T>, cursor: Option<&str>, limit: u64) -> Result<Page<T>, Failure> {
    let total = ranked.len();
    let offset = match cursor {
        None => 0,
        Some(cursor) => {
            cursor.parse::<usize>().ok().filter(|offset| *offset < total).ok_or_else(|| {
                Failure::invalid_argument(format!(
                    "--cursor `{cursor}` is not a cursor this command returned"
                ))
                .with_details(json!({ "argument": "cursor", "value": cursor }))
                .suggest("Pass the nextCursor value from the previous page, or omit --cursor")
            })?
        }
    };
    let limit = usize::try_from(limit).unwrap_or(usize::MAX);
    let end = offset.saturating_add(limit).min(total);
    let items: Vec<T> = ranked.into_iter().skip(offset).take(limit).collect();
    let has_more = end < total;
    Ok(Page { items, has_more, next_cursor: has_more.then(|| end.to_string()) })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drops_stop_words_and_punctuation() {
        assert_eq!(query_words("Delete the old note!"), ["delete", "old", "note"]);
    }

    #[test]
    fn matches_words_that_share_a_stem() {
        assert_eq!(word_match("note", "notes"), 0.6);
        assert_eq!(word_match("migrate", "migration"), 0.6);
        assert_eq!(word_match("add", "address"), 0.0);
        assert_eq!(word_match("store", "store"), 1.0);
    }

    #[test]
    fn pages_resume_from_the_returned_cursor() {
        let first = page((0..7).collect(), None, 3).unwrap();
        assert_eq!(first.items, [0, 1, 2]);
        assert!(first.has_more);
        let second = page((0..7).collect(), first.next_cursor.as_deref(), 3).unwrap();
        assert_eq!(second.items, [3, 4, 5]);
        let last = page((0..7).collect(), second.next_cursor.as_deref(), 3).unwrap();
        assert_eq!(last.items, [6]);
        assert!(!last.has_more);
        assert_eq!(last.next_cursor, None);
    }

    #[test]
    fn rejects_a_cursor_it_did_not_issue() {
        let failure = page(vec![1], Some("abc"), 3).err().unwrap();
        assert_eq!(failure.code, crate::failure::ErrorCode::InvalidArgument);
    }

    #[test]
    fn rejects_a_cursor_past_the_last_item() {
        for cursor in ["7", "1000"] {
            let failure = page((0..7).collect::<Vec<_>>(), Some(cursor), 3).err().unwrap();
            assert_eq!(failure.code, crate::failure::ErrorCode::InvalidArgument, "{cursor}");
        }
    }
}
