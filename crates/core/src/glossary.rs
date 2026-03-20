use serde::{Deserialize, Serialize};
use std::collections::HashMap;
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

    /// Find glossary terms that appear in the given text (word boundary match).
    /// Uses char-based boundary checking to handle non-ASCII (Korean, CJK) text correctly.
    pub fn find_matching_terms(&self, text: &str) -> HashMap<String, String> {
        let text_lower = text.to_lowercase();
        let text_chars: Vec<char> = text_lower.chars().collect();
        self.terms
            .iter()
            .filter(|(term, _)| {
                let term_lower = term.to_lowercase();
                text_lower
                    .match_indices(&term_lower)
                    .any(|(byte_pos, matched)| {
                        let char_pos = text_lower[..byte_pos].chars().count();
                        let char_end = char_pos + matched.chars().count();
                        let before_ok = char_pos == 0
                            || !text_chars[char_pos - 1].is_alphanumeric();
                        let after_ok = char_end >= text_chars.len()
                            || !text_chars[char_end].is_alphanumeric();
                        before_ok && after_ok
                    })
            })
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect()
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

#[derive(Debug, thiserror::Error)]
pub enum GlossaryError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Parse error: {0}")]
    Parse(String),
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
}
