//! Finding state files whose source is gone.
//!
//! An upstream restructure (a bumped submodule that deleted or renamed
//! chapters) leaves `.yeokja.json` files behind. Those orphans hold the only
//! expensive asset in the system — finished translations — so nothing here
//! deletes anything: this module only locates and names them, for `status`
//! to report, `translate` to try adopting across renames, and an explicit
//! clean command to remove.

use crate::config::ProjectConfig;
use std::path::{Path, PathBuf};

const STATE_SUFFIX: &str = ".yeokja.json";

#[derive(Debug, Clone, PartialEq)]
pub struct OrphanState {
    pub state_path: PathBuf,
    /// The source file this state was for, which no longer exists.
    pub expected_source: PathBuf,
}

/// Every state file in the project whose source file no longer exists.
/// Paths in `config` are taken relative to the working directory, like
/// everywhere else a config is used.
pub fn find_orphan_states(config: &ProjectConfig) -> Vec<OrphanState> {
    find_orphan_states_under(Path::new(""), config)
}

/// [`find_orphan_states`] with config-relative paths resolved against `root`.
///
/// With a `state_dir` the directory itself is walked; the sidecar layout
/// walks each configured source root instead.
pub fn find_orphan_states_under(root: &Path, config: &ProjectConfig) -> Vec<OrphanState> {
    let mut state_files: Vec<(PathBuf, PathBuf)> = Vec::new();

    match config.state_dir() {
        Some(state_dir) => {
            let state_dir = root.join(state_dir);
            for state_path in walk_state_files(&state_dir) {
                let Ok(rel) = state_path.strip_prefix(&state_dir) else {
                    continue;
                };
                let Some(source_rel) = strip_state_suffix(rel) else {
                    continue;
                };
                state_files.push((state_path.clone(), root.join(source_rel)));
            }
        }
        None => {
            for source_config in &config.sources {
                for state_path in walk_state_files(&root.join(&source_config.path)) {
                    let Some(source) = strip_state_suffix(&state_path) else {
                        continue;
                    };
                    state_files.push((state_path.clone(), source));
                }
            }
        }
    }

    state_files.sort();
    state_files.dedup();
    state_files
        .into_iter()
        .filter(|(_, source)| !source.exists())
        .map(|(state_path, expected_source)| OrphanState {
            state_path,
            expected_source,
        })
        .collect()
}

fn strip_state_suffix(path: &Path) -> Option<PathBuf> {
    let name = path.file_name()?.to_str()?;
    let stem = name.strip_suffix(STATE_SUFFIX)?;
    Some(path.with_file_name(stem))
}

fn walk_state_files(root: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path
                .file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.ends_with(STATE_SUFFIX))
            {
                found.push(path);
            }
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ProjectConfig;

    fn config(state_dir: bool) -> ProjectConfig {
        let state_line = if state_dir { "state_dir = \"state\"" } else { "" };
        ProjectConfig::from_toml(&format!(
            r#"
[project]
source_lang = "en"
target_lang = "ko"
{state_line}

[[sources]]
path = "book/"
pattern = "**/*.md"
parser = "markdown"
output = "ko/{{path}}"

[provider]
type = "claude_code"
model = "test"
"#
        ))
        .unwrap()
    }

    #[test]
    fn sidecar_orphans_are_found_and_live_states_are_not() {
        let dir = tempfile::tempdir().unwrap();
        let book = dir.path().join("book");
        std::fs::create_dir_all(&book).unwrap();
        std::fs::write(book.join("kept.md"), "hi").unwrap();
        std::fs::write(book.join("kept.md.yeokja.json"), "{}").unwrap();
        std::fs::write(book.join("gone.md.yeokja.json"), "{}").unwrap();

        let orphans = find_orphan_states_under(dir.path(), &config(false));
        assert_eq!(orphans.len(), 1);
        assert_eq!(orphans[0].expected_source, book.join("gone.md"));
        assert_eq!(orphans[0].state_path, book.join("gone.md.yeokja.json"));
    }

    #[test]
    fn state_dir_orphans_map_back_to_project_relative_sources() {
        let dir = tempfile::tempdir().unwrap();
        let book = dir.path().join("book");
        let mirrored = dir.path().join("state/book");
        std::fs::create_dir_all(&book).unwrap();
        std::fs::create_dir_all(&mirrored).unwrap();
        std::fs::write(book.join("kept.md"), "hi").unwrap();
        std::fs::write(mirrored.join("kept.md.yeokja.json"), "{}").unwrap();
        std::fs::write(mirrored.join("gone.md.yeokja.json"), "{}").unwrap();

        let orphans = find_orphan_states_under(dir.path(), &config(true));
        assert_eq!(orphans.len(), 1);
        assert_eq!(orphans[0].expected_source, book.join("gone.md"));
        assert_eq!(
            orphans[0].state_path,
            mirrored.join("gone.md.yeokja.json")
        );
    }
}
