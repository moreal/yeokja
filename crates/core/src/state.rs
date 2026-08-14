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
        // A configured state_dir is not required to exist up front.
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            std::fs::create_dir_all(parent)?;
        }
        let tmp_path = path.with_extension("json.tmp");
        std::fs::write(&tmp_path, &json)?;
        std::fs::rename(&tmp_path, path)?;
        Ok(())
    }

    /// Where the state for `source_path` lives. Without a `state_dir` it is a
    /// sidecar next to the source; with one, the source's project-relative
    /// path is mirrored under that directory, so a read-only source tree
    /// (e.g. a git submodule) is never written into.
    ///
    /// Project-relative means relative to the working directory, which is the
    /// project root everywhere a config is loaded. An absolute `source_path`
    /// outside the project keeps the sidecar layout.
    pub fn state_file_path(source_path: &Path, state_dir: Option<&Path>) -> std::path::PathBuf {
        let file_name = source_path.file_name().expect("source path must have a file name").to_string_lossy();
        let sidecar = source_path.with_file_name(format!("{file_name}.yeokja.json"));
        let Some(dir) = state_dir else {
            return sidecar;
        };
        let rel = source_path.strip_prefix(".").unwrap_or(source_path);
        let rel = if rel.is_absolute() {
            let under_cwd = std::env::current_dir()
                .ok()
                .and_then(|cwd| rel.strip_prefix(cwd).ok().map(Path::to_path_buf));
            match under_cwd {
                Some(p) => p,
                None => return sidecar,
            }
        } else {
            rel.to_path_buf()
        };
        dir.join(rel.with_file_name(format!("{file_name}.yeokja.json")))
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
        let state_path = StateFile::state_file_path(path, None);
        assert_eq!(state_path, Path::new("/book/chapter1.md.yeokja.json"));
    }

    #[test]
    fn state_dir_mirrors_the_project_relative_source_path() {
        let path = Path::new("./upstream/chapters/ch1.adoc");
        let state_path = StateFile::state_file_path(path, Some(Path::new("state")));
        assert_eq!(
            state_path,
            Path::new("state/upstream/chapters/ch1.adoc.yeokja.json")
        );
    }

    #[test]
    fn absolute_source_under_the_project_still_lands_in_state_dir() {
        let path = std::env::current_dir().unwrap().join("book/ch1.md");
        let state_path = StateFile::state_file_path(&path, Some(Path::new("state")));
        assert_eq!(state_path, Path::new("state/book/ch1.md.yeokja.json"));
    }

    #[test]
    fn absolute_source_outside_the_project_keeps_its_sidecar() {
        let path = Path::new("/elsewhere/ch1.md");
        let state_path = StateFile::state_file_path(path, Some(Path::new("state")));
        assert_eq!(state_path, Path::new("/elsewhere/ch1.md.yeokja.json"));
    }

    #[test]
    fn save_creates_the_state_dir_on_demand() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("state/upstream/ch1.md.yeokja.json");
        StateFile::new(1).save(&path).unwrap();
        assert!(path.exists());
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
