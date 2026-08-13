use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::ops::Range;
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GlossaryEntry {
    pub translation: String,
    #[serde(default)]
    pub note: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GlossaryFile {
    #[serde(default)]
    pub terms: HashMap<String, GlossaryEntry>,
}

#[derive(Debug, Clone)]
pub struct Glossary {
    terms: HashMap<String, String>,
}

/// Find terms from the given map that the text uses as prose.
///
/// A term only counts where a translator could act on it. Inside a code span,
/// a URL, or a longer identifier it is a name being quoted, and the translation
/// has to leave it exactly as it stands — so asking for the glossary rendering
/// there fails a translation that is correct, and the block is translated again
/// and fails again. Of the 229 segments theBeamBook's glossary check rejected,
/// 204 were this.
pub fn find_terms_in_text(terms: &HashMap<String, String>, text: &str) -> HashMap<String, String> {
    let text_lower = text.to_lowercase();
    let chars: Vec<char> = text_lower.chars().collect();
    let verbatim = verbatim_spans(&chars);
    terms
        .iter()
        .filter(|(term, _)| {
            let term_chars: Vec<char> = term.to_lowercase().chars().collect();
            occurrences(&chars, &term_chars).any(|start| {
                let end = start + term_chars.len();
                !verbatim.iter().any(|s| s.start < end && start < s.end)
                    && (start == 0 || !is_word_char(chars[start - 1]))
                    && (end >= chars.len() || !is_word_char(chars[end]))
            })
        })
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect()
}

/// Every position in `haystack` where `needle` starts.
fn occurrences<'a>(haystack: &'a [char], needle: &'a [char]) -> impl Iterator<Item = usize> + 'a {
    let last = haystack.len().checked_sub(needle.len());
    last.filter(|_| !needle.is_empty())
        .into_iter()
        .flat_map(move |last| (0..=last).filter(move |&i| haystack[i..i + needle.len()] == *needle))
}

/// What a term has to be free of on both sides to be a word of its own.
///
/// Underscore belongs here even though it is not alphanumeric: `min_heap_size`
/// is one identifier, not a sentence containing "heap". This is also how
/// Asciidoctor reads a word — its `\p{Word}` covers `_`.
fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// Char ranges of `chars` that a reader sees exactly as written: inline code
/// spans and the targets of URLs and macros.
///
/// The link *text* is deliberately left out — `link:https://…[ERTS Reference]`
/// carries prose after the `[` that a translator may well render.
fn verbatim_spans(chars: &[char]) -> Vec<Range<usize>> {
    let mut spans = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        if let Some(end) = code_span_end(chars, i).or_else(|| target_end(chars, i)) {
            spans.push(i..end);
            i = end;
        } else {
            i += 1;
        }
    }
    spans
}

/// Where the code span opening at `at` ends, if one does. Both markups write it
/// between two runs of backticks of the same length, so `` ``x`` `` closes on
/// the pair and not on the first backtick of it.
fn code_span_end(chars: &[char], at: usize) -> Option<usize> {
    let run = |from: usize| chars[from..].iter().take_while(|c| **c == '`').count();
    let open = run(at);
    if open == 0 {
        return None;
    }
    let mut from = at + open;
    loop {
        let offset = chars[from..].iter().position(|c| *c == '`')?;
        let close = from + offset;
        let len = run(close);
        if len == open {
            return Some(close + len);
        }
        from = close + len;
    }
}

/// Where the URL or macro target starting at `at` ends, if one does. A target
/// runs to the whitespace or the `[` that ends it; a Markdown one to its `)`.
fn target_end(chars: &[char], at: usize) -> Option<usize> {
    const SCHEMES: [&str; 9] = [
        "http://", "https://", "ftp://", "mailto:", "link:", "xref:", "image:", "video:", "audio:",
    ];
    // Markdown writes the target after the link text, with no scheme of its own.
    if chars[at] == ']' && chars.get(at + 1) == Some(&'(') {
        let offset = chars[at + 2..].iter().position(|c| *c == ')')?;
        return Some(at + 2 + offset + 1);
    }
    if at > 0 && is_word_char(chars[at - 1]) {
        return None;
    }
    let starts_with = |s: &str| chars[at..].iter().copied().take(s.chars().count()).eq(s.chars());
    SCHEMES.iter().any(|s| starts_with(s)).then(|| {
        at + chars[at..]
            .iter()
            .position(|c| c.is_whitespace() || *c == '[')
            .unwrap_or(chars.len() - at)
    })
}

impl Glossary {
    pub fn load(path: &Path) -> Result<Self, GlossaryError> {
        let content = std::fs::read_to_string(path)
            .map_err(GlossaryError::Io)?;
        Self::from_toml(&content)
    }

    pub fn from_toml(content: &str) -> Result<Self, GlossaryError> {
        let file: GlossaryFile = toml::from_str(content)
            .map_err(|e| GlossaryError::Parse(e.to_string()))?;
        let terms = file.terms.into_iter()
            .map(|(k, v)| (k, v.translation))
            .collect();
        Ok(Self { terms })
    }

    pub fn empty() -> Self {
        Self { terms: HashMap::new() }
    }

    pub fn terms(&self) -> &HashMap<String, String> {
        &self.terms
    }

    /// Find glossary terms the given text uses as prose.
    /// Uses char-based boundary checking to handle non-ASCII (Korean, CJK) text correctly.
    pub fn find_matching_terms(&self, text: &str) -> HashMap<String, String> {
        find_terms_in_text(&self.terms, text)
    }

    /// Check if a glossary snapshot is stale compared to current glossary.
    pub fn is_snapshot_stale(&self, snapshot: &HashMap<String, String>, source_text: &str) -> bool {
        let current_matches = self.find_matching_terms(source_text);
        for (term, translation) in &current_matches {
            match snapshot.get(term) {
                Some(snap_translation) if snap_translation == translation => {}
                _ => return true,
            }
        }
        false
    }
}

/// Add or update a term in a glossary TOML file, preserving existing entries
/// (including their `note` fields). Creates the file if it does not exist.
pub fn upsert_term_in_file(
    path: &Path,
    term: &str,
    translation: &str,
) -> Result<(), GlossaryError> {
    let content = if path.exists() {
        std::fs::read_to_string(path)?
    } else {
        String::new()
    };

    let mut doc: toml::Table = if content.is_empty() {
        toml::Table::new()
    } else {
        content
            .parse()
            .map_err(|e: toml::de::Error| GlossaryError::Parse(e.to_string()))?
    };

    let terms = doc
        .entry("terms")
        .or_insert_with(|| toml::Value::Table(toml::Table::new()));
    if let toml::Value::Table(terms_table) = terms {
        let entry = terms_table
            .entry(term.to_string())
            .or_insert_with(|| toml::Value::Table(toml::Table::new()));
        if let toml::Value::Table(entry_table) = entry {
            entry_table.insert(
                "translation".to_string(),
                toml::Value::String(translation.to_string()),
            );
        } else {
            let mut entry_table = toml::Table::new();
            entry_table.insert(
                "translation".to_string(),
                toml::Value::String(translation.to_string()),
            );
            *entry = toml::Value::Table(entry_table);
        }
    }

    let serialized = toml::to_string_pretty(&doc)
        .map_err(|e| GlossaryError::Serialize(e.to_string()))?;
    std::fs::write(path, serialized)?;
    Ok(())
}

/// Remove a term from a glossary TOML file. Returns whether the term existed.
pub fn remove_term_in_file(path: &Path, term: &str) -> Result<bool, GlossaryError> {
    if !path.exists() {
        return Ok(false);
    }
    let content = std::fs::read_to_string(path)?;
    let mut doc: toml::Table = content
        .parse()
        .map_err(|e: toml::de::Error| GlossaryError::Parse(e.to_string()))?;

    let removed = match doc.get_mut("terms") {
        Some(toml::Value::Table(terms_table)) => terms_table.remove(term).is_some(),
        _ => false,
    };

    if removed {
        let serialized = toml::to_string_pretty(&doc)
            .map_err(|e| GlossaryError::Serialize(e.to_string()))?;
        std::fs::write(path, serialized)?;
    }
    Ok(removed)
}

#[derive(Debug, thiserror::Error)]
pub enum GlossaryError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Parse error: {0}")]
    Parse(String),
    #[error("Serialize error: {0}")]
    Serialize(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_glossary() -> Glossary {
        let toml = r#"
[terms.repository]
translation = "저장소"
note = "Git 저장소"

[terms.commit]
translation = "커밋"
"#;
        Glossary::from_toml(toml).unwrap()
    }

    #[test]
    fn load_glossary_from_toml() {
        let g = test_glossary();
        assert_eq!(g.terms().get("repository").unwrap(), "저장소");
        assert_eq!(g.terms().get("commit").unwrap(), "커밋");
    }

    #[test]
    fn find_matching_terms_word_boundary() {
        let g = test_glossary();
        let matches = g.find_matching_terms("The repository stores commits.");
        assert_eq!(matches.get("repository").unwrap(), "저장소");
        assert_eq!(matches.get("commit"), None);
    }

    #[test]
    fn find_matching_terms_no_partial() {
        let g = test_glossary();
        let matches = g.find_matching_terms("repositoryName is set");
        assert!(matches.is_empty());
    }

    fn beam_glossary() -> Glossary {
        Glossary::from_toml(
            r#"
[terms.Erlang]
translation = "Erlang"

[terms.heap]
translation = "힙"

[terms.compiler]
translation = "컴파일러"
"#,
        )
        .unwrap()
    }

    /// The case this was written for. `erlang:processes/0` has to survive the
    /// translation as it stands, so a glossary rendering can never appear for
    /// it — and every retry produced the same text and the same complaint.
    #[test]
    fn a_term_inside_a_code_span_is_a_name() {
        let g = beam_glossary();
        assert!(g.find_matching_terms("Call `erlang:processes/0` to list them.").is_empty());
        assert!(g.find_matching_terms("Set ``min_heap_size`` on spawn.").is_empty());
        // …but the same term outside one is still prose.
        let both = g.find_matching_terms("In Erlang, call `erlang:processes/0`.");
        assert_eq!(both.get("Erlang").unwrap(), "Erlang");
    }

    #[test]
    fn a_term_inside_a_url_is_a_name() {
        let g = beam_glossary();
        for text in [
            "See https://www.erlang.org/doc/apps/compiler/index.html for details.",
            "See link:https://erlang.org/x[the docs] for details.",
            "See [the docs](https://erlang.org/compiler) for details.",
        ] {
            assert!(g.find_matching_terms(text).is_empty(), "{text:?}");
        }
    }

    /// A macro's link text is prose and gets translated, so it is not masked
    /// along with the target.
    #[test]
    fn link_text_is_still_prose() {
        let g = beam_glossary();
        let matches = g.find_matching_terms("link:https://x.example/a[The Erlang compiler]");
        assert_eq!(matches.get("Erlang").unwrap(), "Erlang");
        assert_eq!(matches.get("compiler").unwrap(), "컴파일러");
    }

    /// Underscore joins a word rather than breaking it, which is also how
    /// Asciidoctor reads one.
    #[test]
    fn an_identifier_is_one_word() {
        let g = beam_glossary();
        assert!(g.find_matching_terms("The allocate_heap_zero instruction.").is_empty());
        assert_eq!(
            g.find_matching_terms("The heap grows.").get("heap").unwrap(),
            "힙"
        );
    }

    /// A backtick with nothing to pair with opens no span, and the prose after
    /// it still counts.
    #[test]
    fn a_lone_backtick_masks_nothing() {
        let g = beam_glossary();
        assert_eq!(
            g.find_matching_terms("A ` and the heap after it")
                .get("heap")
                .unwrap(),
            "힙"
        );
    }

    #[test]
    fn snapshot_not_stale_when_matching() {
        let g = test_glossary();
        let mut snapshot = HashMap::new();
        snapshot.insert("repository".to_string(), "저장소".to_string());
        assert!(!g.is_snapshot_stale(&snapshot, "The repository is ready."));
    }

    #[test]
    fn snapshot_stale_when_translation_changed() {
        let g = test_glossary();
        let mut snapshot = HashMap::new();
        snapshot.insert("repository".to_string(), "레포지토리".to_string());
        assert!(g.is_snapshot_stale(&snapshot, "The repository is ready."));
    }

    #[test]
    fn snapshot_stale_when_new_term_added() {
        let g = test_glossary();
        let snapshot = HashMap::new();
        assert!(g.is_snapshot_stale(&snapshot, "The repository is ready."));
    }

    #[test]
    fn snapshot_not_stale_when_term_not_in_text() {
        let g = test_glossary();
        let snapshot = HashMap::new();
        assert!(!g.is_snapshot_stale(&snapshot, "Hello world."));
    }

    #[test]
    fn upsert_preserves_notes_and_updates() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("glossary.toml");
        std::fs::write(
            &path,
            "[terms.repository]\ntranslation = \"저장소\"\nnote = \"Git 저장소\"\n",
        )
        .unwrap();

        upsert_term_in_file(&path, "branch", "브랜치").unwrap();
        upsert_term_in_file(&path, "repository", "리포지터리").unwrap();

        let g = Glossary::load(&path).unwrap();
        assert_eq!(g.terms().get("branch").unwrap(), "브랜치");
        assert_eq!(g.terms().get("repository").unwrap(), "리포지터리");

        // The note on the updated entry must survive.
        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.contains("Git 저장소"));
    }

    #[test]
    fn upsert_creates_file_when_missing() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("glossary.toml");
        upsert_term_in_file(&path, "commit", "커밋").unwrap();
        let g = Glossary::load(&path).unwrap();
        assert_eq!(g.terms().get("commit").unwrap(), "커밋");
    }

    #[test]
    fn remove_term_removes_and_reports() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("glossary.toml");
        upsert_term_in_file(&path, "commit", "커밋").unwrap();

        assert!(remove_term_in_file(&path, "commit").unwrap());
        assert!(!remove_term_in_file(&path, "commit").unwrap());
        let g = Glossary::load(&path).unwrap();
        assert!(g.terms().is_empty());
    }
}
