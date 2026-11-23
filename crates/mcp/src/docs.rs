//! The Markdown documentation compiled into the binary, which `docs search` and `docs show`
//! serve. An installed binary therefore answers documentation questions without the source tree.

/// One shipped document.
pub struct Document {
    /// Where the file lives in the crate, which is also how commands name it: `docs/tools.md`.
    pub path: &'static str,
    pub source: &'static str,
}

/// Every shipped document. A test fails when a file in `docs/` is missing here.
pub const DOCUMENTS: &[Document] = &[
    Document { path: "docs/configuration.md", source: include_str!("../docs/configuration.md") },
    Document { path: "docs/tools.md", source: include_str!("../docs/tools.md") },
    Document { path: "docs/sessions.md", source: include_str!("../docs/sessions.md") },
    Document { path: "docs/approval.md", source: include_str!("../docs/approval.md") },
    Document { path: "docs/cli.md", source: include_str!("../docs/cli.md") },
];

/// A heading and the text up to the next heading.
pub struct Section<'a> {
    /// The heading text without its `#` marks; empty for text before the first heading.
    pub heading: &'a str,
    pub body: String,
}

impl Document {
    /// The first `# ` heading, else the path.
    pub fn title(&self) -> &'static str {
        body(self.source)
            .lines()
            .find_map(|line| line.strip_prefix("# ").map(str::trim))
            .unwrap_or(self.path)
    }

    /// The document split at every heading, frontmatter dropped.
    pub fn sections(&self) -> Vec<Section<'static>> {
        let mut sections = vec![Section { heading: "", body: String::new() }];
        let mut fenced = false;
        for line in body(self.source).lines() {
            if line.starts_with("```") {
                fenced = !fenced;
            }
            let heading = (!fenced)
                .then(|| line.trim_start_matches('#'))
                .filter(|rest| line.starts_with('#') && rest.starts_with(' '));
            match heading {
                Some(rest) => sections.push(Section { heading: rest.trim(), body: String::new() }),
                None => {
                    if let Some(section) = sections.last_mut() {
                        section.body.push_str(line);
                        section.body.push('\n');
                    }
                }
            }
        }
        sections.retain(|section| !section.heading.is_empty() || !section.body.trim().is_empty());
        sections
    }
}

pub fn find(path: &str) -> Option<&'static Document> {
    DOCUMENTS.iter().find(|document| document.path == path)
}

/// `source` without YAML frontmatter.
fn body(source: &str) -> &str {
    source
        .strip_prefix("---\n")
        .and_then(|rest| rest.find("\n---\n").map(|end| &rest[end + 5..]))
        .unwrap_or(source)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_file_in_docs_is_shipped() {
        let directory = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("docs");
        let mut on_disk: Vec<String> = std::fs::read_dir(directory)
            .unwrap()
            .map(|entry| format!("docs/{}", entry.unwrap().file_name().to_string_lossy()))
            .filter(|path| path.ends_with(".md"))
            .collect();
        on_disk.sort();
        let mut shipped: Vec<String> =
            DOCUMENTS.iter().map(|document| document.path.to_string()).collect();
        shipped.sort();
        assert_eq!(on_disk, shipped, "list every docs/*.md file in DOCUMENTS");
    }

    #[test]
    fn sections_split_at_headings_outside_code() {
        let document = Document {
            path: "docs/x.md",
            source: "---\ntitle: X\n---\n# Title\n\nIntro.\n\n## First\n\nOne.\n\n```sh\n# not a heading\n```\n\n### Second\nTwo.\n",
        };
        assert_eq!(document.title(), "Title");
        let headings: Vec<&str> =
            document.sections().iter().map(|section| section.heading).collect();
        assert_eq!(headings, ["Title", "First", "Second"]);
        assert!(document.sections()[1].body.contains("# not a heading"));
    }
}
