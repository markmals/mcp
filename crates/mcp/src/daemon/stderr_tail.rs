//! The last lines a stdio server wrote to its standard error.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex, PoisonError};

use tokio::io::{AsyncBufReadExt, AsyncRead, BufReader};

const MAX_LINES: usize = 200;
const MAX_BYTES: usize = 64 * 1024;

/// At most [`MAX_LINES`] lines and [`MAX_BYTES`] bytes, oldest first; older lines are dropped.
#[derive(Debug, Default)]
pub(crate) struct StderrTail {
    lines: VecDeque<String>,
    bytes: usize,
}

impl StderrTail {
    fn push(&mut self, line: &[u8]) {
        let line = String::from_utf8_lossy(line.strip_suffix(b"\r").unwrap_or(line));
        let line = truncate(&line, MAX_BYTES).to_owned();
        self.bytes += line.len();
        self.lines.push_back(line);
        while self.lines.len() > MAX_LINES || self.bytes > MAX_BYTES {
            let Some(oldest) = self.lines.pop_front() else { break };
            self.bytes -= oldest.len();
        }
    }

    fn lines(&self) -> Vec<String> {
        self.lines.iter().cloned().collect()
    }
}

/// A tail shared between the task that reads a server's stderr and the session that reports it.
#[derive(Debug, Clone, Default)]
pub(crate) struct SharedTail(Arc<Mutex<StderrTail>>);

impl SharedTail {
    pub(crate) fn lines(&self) -> Vec<String> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner).lines()
    }

    fn push(&self, line: &[u8]) {
        self.0.lock().unwrap_or_else(PoisonError::into_inner).push(line);
    }

    /// Reads `stream` to its end. A line longer than the whole tail keeps only its start, so a
    /// server that never writes a newline cannot grow the daemon's memory.
    pub(crate) async fn collect(self, stream: impl AsyncRead + Unpin) {
        let mut reader = BufReader::new(stream);
        let mut line = Vec::new();
        loop {
            let chunk = match reader.fill_buf().await {
                Ok([]) | Err(_) => break,
                Ok(chunk) => chunk,
            };
            let newline = chunk.iter().position(|&byte| byte == b'\n');
            let text = &chunk[..newline.unwrap_or(chunk.len())];
            let room = MAX_BYTES.saturating_sub(line.len());
            line.extend_from_slice(&text[..text.len().min(room)]);
            let consumed = newline.map_or(chunk.len(), |index| index + 1);
            reader.consume(consumed);
            if newline.is_some() {
                self.push(&line);
                line.clear();
            }
        }
        if !line.is_empty() {
            self.push(&line);
        }
    }
}

fn truncate(text: &str, max_bytes: usize) -> &str {
    if text.len() <= max_bytes {
        return text;
    }
    let mut end = max_bytes;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tail_of(lines: impl IntoIterator<Item = String>) -> StderrTail {
        let mut tail = StderrTail::default();
        for line in lines {
            tail.push(line.as_bytes());
        }
        tail
    }

    #[test]
    fn keeps_the_last_two_hundred_lines() {
        let tail = tail_of((1..=250).map(|number| format!("line {number}")));
        let lines = tail.lines();
        assert_eq!(lines.len(), 200);
        assert_eq!(lines.first().map(String::as_str), Some("line 51"));
        assert_eq!(lines.last().map(String::as_str), Some("line 250"));
    }

    #[test]
    fn keeps_at_most_sixty_four_kibibytes() {
        let tail = tail_of((0..100).map(|number| format!("{number:04}{}", "x".repeat(1020))));
        let lines = tail.lines();
        assert_eq!(lines.iter().map(String::len).sum::<usize>(), 64 * 1024);
        assert_eq!(lines.len(), 64);
        assert!(lines[0].starts_with("0036"), "oldest kept line: {}", &lines[0][..4]);
    }

    #[test]
    fn a_line_longer_than_the_tail_keeps_its_start() {
        let tail = tail_of(["first".to_owned(), "é".repeat(40_000)]);
        let lines = tail.lines();
        assert_eq!(lines.len(), 1, "the long line pushes out everything before it");
        assert_eq!(lines[0].len(), 64 * 1024);
        assert!(lines[0].chars().all(|character| character == 'é'));
    }

    #[tokio::test]
    async fn collect_splits_lines_and_keeps_an_unterminated_last_line() {
        let tail = SharedTail::default();
        tail.clone().collect(&b"one\r\ntwo\n\nthree"[..]).await;
        assert_eq!(tail.lines(), ["one", "two", "", "three"]);
    }

    #[tokio::test]
    async fn collect_bounds_a_line_without_a_newline() {
        let tail = SharedTail::default();
        let endless = vec![b'x'; 3 * MAX_BYTES];
        tail.clone().collect(&endless[..]).await;
        assert_eq!(tail.lines().iter().map(String::len).collect::<Vec<_>>(), [MAX_BYTES]);
    }
}
