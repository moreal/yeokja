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

/// Find terms from the given map that appear in the text (word boundary match).
pub fn find_terms_in_text(terms: &HashMap<String, String>, text: &str) -> HashMap<String, String> {
    let text_lower = text.to_lowercase();
    let text_chars: Vec<char> = text_lower.chars().collect();
    terms
        .iter()
        .filter(|(term, _)| {
            let term_lower = term.to_lowercase();
            text_lower
                .match_indices(&term_lower)
                .any(|(byte_pos, matched)| {
                    let char_pos = text_lower[..byte_pos].chars().count();
                    let char_end = char_pos + matched.chars().count();
                    let before_ok = char_pos == 0 || !text_chars[char_pos - 1].is_alphanumeric();
                    let after_ok = char_end >= text_chars.len() || !text_chars[char_end].is_alphanumeric();
                    before_ok && after_ok
                })
        })
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect()
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
