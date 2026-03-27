use crate::model::SegmentId;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

const CURRENT_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateFile {
    pub version: u32,
    pub source_hash: u64,
    pub segments: Vec<SegmentState>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SegmentState {
    pub id: SegmentId,
    pub source: String,
    pub source_hash: u64,
    pub context_hash: u64,
    pub translation: Option<String>,
    #[serde(default)]
    pub glossary_snapshot: HashMap<String, String>,
    pub translated_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub issues: Vec<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum StateError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
}

impl StateFile {
    pub fn new(source_hash: u64) -> Self {
        Self {
            version: CURRENT_VERSION,
            source_hash,
            segments: Vec::new(),
        }
    }

    pub fn load(path: &Path) -> Result<Self, StateError> {
        let content = std::fs::read_to_string(path)?;
        let state: Self = serde_json::from_str(&content)?;
        Ok(state)
    }

    pub fn save(&self, path: &Path) -> Result<(), StateError> {
        let json = serde_json::to_string_pretty(self)?;
        let tmp_path = path.with_extension("json.tmp");
        std::fs::write(&tmp_path, &json)?;
        std::fs::rename(&tmp_path, path)?;
        Ok(())
    }

    pub fn state_file_path(source_path: &Path) -> std::path::PathBuf {
        let file_name = source_path.file_name().expect("source path must have a file name").to_string_lossy();
        source_path.with_file_name(format!("{file_name}.yeokja.json"))
    }

    pub fn find_by_hash(&self, source_hash: u64) -> Vec<&SegmentState> {
        self.segments.iter().filter(|s| s.source_hash == source_hash).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn state_file_path_generation() {
        let path = Path::new("/book/chapter1.md");
        let state_path = StateFile::state_file_path(path);
        assert_eq!(state_path, Path::new("/book/chapter1.md.yeokja.json"));
    }

    #[test]
    fn save_and_load_roundtrip() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test.md.yeokja.json");
        let mut state = StateFile::new(12345);
        state.segments.push(SegmentState {
            id: SegmentId::new(0, 0, 0),
            source: "Hello.".to_string(),
            source_hash: 111,
            context_hash: 222,
            translation: Some("안녕하세요.".to_string()),
            glossary_snapshot: HashMap::new(),
            translated_at: Some(Utc::now()),
            issues: Vec::new(),
        });
        state.save(&path).unwrap();
        let loaded = StateFile::load(&path).unwrap();
        assert_eq!(loaded.version, 1);
        assert_eq!(loaded.source_hash, 12345);
        assert_eq!(loaded.segments.len(), 1);
        assert_eq!(loaded.segments[0].source, "Hello.");
        assert_eq!(loaded.segments[0].translation.as_deref(), Some("안녕하세요."));
    }

    #[test]
    fn atomic_write_leaves_no_tmp_on_success() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test.md.yeokja.json");
        let tmp_path = path.with_extension("json.tmp");
        let state = StateFile::new(1);
        state.save(&path).unwrap();
        assert!(path.exists());
        assert!(!tmp_path.exists());
    }
}
