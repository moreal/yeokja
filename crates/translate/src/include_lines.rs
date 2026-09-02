//! Keeps `.. include::` line offsets pointing at the right place when the
//! included file is itself translated by the project.
//!
//! `:start-line:`/`:end-line:` count lines of the included file. The Korean
//! rendering of that file is shorter (paragraphs are joined), so the offsets
//! written for the English file cut the Korean one somewhere else. When the
//! including file's output is written, each such directive is followed to its
//! target; if the target is a source of this project, its translated output is
//! rebuilt from state and the offsets are carried across by block.

use crate::orchestrator::{ParserFactory, scan_file};
use std::path::{Component, Path, PathBuf};
use yeokja_core::config::ProjectConfig;
use yeokja_core::glossary::Glossary;
use yeokja_core::parser::TranslationMap;
use yeokja_parser_rst::include::{LineMap, rewrite_line_options};

/// Rewrite the line options of every `include` directive in `output`, the
/// translated text of `file_path`, whose target is another source file of the
/// project. Directives whose target is not translated here, or whose
/// translation cannot be lined up with its source, are left untouched.
pub(crate) fn follow_rst_includes(
    output: &str,
    file_path: &Path,
    config: &ProjectConfig,
    glossary: &Glossary,
    parser_factory: &ParserFactory,
) -> String {
    rewrite_line_options(output, |target| {
        let target_path = include_target(file_path, target)?;
        if !is_project_source(&target_path, config) {
            tracing::debug!(
                file = %file_path.display(),
                target = %target_path.display(),
                "Include target is not a project source; line options kept"
            );
            return None;
        }
        let (doc, reconciled) = match scan_file(&target_path, config, glossary, parser_factory) {
            Ok(scanned) => scanned,
            Err(error) => {
                tracing::debug!(
                    file = %file_path.display(),
                    target = %target_path.display(),
                    error = %error,
                    "Include target could not be read; line options kept"
                );
                return None;
            }
        };
        let translations: TranslationMap = reconciled
            .into_iter()
            .filter_map(|rs| rs.state.translation.map(|t| (rs.state.id, t)))
            .collect();
        let parser = parser_factory(&target_path, config);
        let translated = parser.reconstruct(&doc, &translations);
        let map = LineMap::between(&doc, &parser.parse(&translated));
        if map.is_none() {
            tracing::debug!(
                file = %file_path.display(),
                target = %target_path.display(),
                "Include target's translation does not line up with its source; line options kept"
            );
        }
        map
    })
}

/// The included file's path, resolved against the including file's directory
/// and normalized lexically so it can be compared with configured sources.
fn include_target(file_path: &Path, target: &str) -> Option<PathBuf> {
    let joined = file_path.parent()?.join(target);
    let mut normalized = PathBuf::new();
    for component in joined.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                if !matches!(
                    normalized.components().next_back(),
                    None | Some(Component::ParentDir | Component::RootDir | Component::Prefix(_))
                ) {
                    normalized.pop();
                } else {
                    normalized.push(component);
                }
            }
            other => normalized.push(other),
        }
    }
    Some(normalized)
}

/// Whether `path` is a file some `[[sources]]` entry translates.
fn is_project_source(path: &Path, config: &ProjectConfig) -> bool {
    let path = path.strip_prefix(".").unwrap_or(path);
    path.is_file()
        && config.sources.iter().any(|source| {
            let root = Path::new(&source.path);
            let root = root.strip_prefix(".").unwrap_or(root);
            let Ok(relative) = path.strip_prefix(root) else {
                return false;
            };
            let matches = |pattern: &str| {
                glob::Pattern::new(pattern).is_ok_and(|pattern| pattern.matches_path(relative))
            };
            matches(&source.pattern) && !source.exclude.iter().any(|excluded| matches(excluded))
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use yeokja_core::parser::DocumentParser;
    use yeokja_core::state::{SegmentState, StateFile};
    use yeokja_parser_rst::RstParser;

    const GC_INFO: &str = "Garbage collection\n==================\n\nThe nursery is where\nyoung objects are born\nand die.\n\n.. _env:\n\nEnvironment variables\n---------------------\n\nSet ``PYPY_GC_NURSERY``\nto tune it.\n";

    const MAN_PAGE: &str = "ENVIRONMENT\n===========\n\n.. include:: ../gc_info.rst\n   :start-line: 10\n   :end-line: 14\n\n.. include:: ../notes.txt\n   :start-line: 10\n";

    fn factory() -> ParserFactory {
        Arc::new(|_, _| Box::new(RstParser))
    }

    /// A project whose `doc/gc_info.rst` is fully translated, with sidecar state.
    fn project(dir: &Path) -> ProjectConfig {
        let doc = dir.join("doc");
        std::fs::create_dir_all(doc.join("man")).unwrap();
        std::fs::write(doc.join("gc_info.rst"), GC_INFO).unwrap();
        std::fs::write(doc.join("notes.txt"), GC_INFO).unwrap();
        std::fs::write(doc.join("man/pypy.1.rst"), MAN_PAGE).unwrap();

        let parsed = RstParser.parse(GC_INFO);
        let mut state = StateFile::new(0);
        for seg in parsed.translatable_segments() {
            state.segments.push(SegmentState {
                id: seg.id.clone(),
                source: seg.source.clone(),
                source_hash: seg.source_hash,
                context_hash: 0,
                translation: Some(format!("번역: {}", seg.source)),
                glossary_snapshot: Default::default(),
                translated_at: None,
                issues: Vec::new(),
            });
        }
        state
            .save(&StateFile::state_file_path(&doc.join("gc_info.rst"), None))
            .unwrap();

        ProjectConfig::from_toml(&format!(
            r#"
[project]
source_lang = "en"
target_lang = "ko"

[[sources]]
path = "{doc}"
pattern = "**/*.rst"
parser = "rst"
output = "{dir}/ko/{{path}}"

[provider]
type = "openai_compatible"
model = "test"
"#,
            doc = doc.display(),
            dir = dir.display()
        ))
        .unwrap()
    }

    #[test]
    fn line_options_follow_the_translated_target() {
        let dir = tempfile::tempdir().unwrap();
        let config = project(dir.path());
        let man = dir.path().join("doc/man/pypy.1.rst");

        let out = follow_rst_includes(MAN_PAGE, &man, &config, &Glossary::empty(), &factory());

        // Source line 10 is the underline of "Environment variables"; the
        // three-line paragraph above it became one line, so it moved up by two.
        assert_eq!(GC_INFO.lines().nth(10), Some("---------------------"));
        assert_eq!(
            out,
            "ENVIRONMENT\n===========\n\n.. include:: ../gc_info.rst\n   :start-line: 8\n   :end-line: 11\n\n.. include:: ../notes.txt\n   :start-line: 10\n"
        );
    }

    #[test]
    fn a_target_outside_the_project_is_left_alone() {
        let dir = tempfile::tempdir().unwrap();
        let config = project(dir.path());
        let elsewhere = dir.path().join("other/readme.rst");
        std::fs::create_dir_all(elsewhere.parent().unwrap()).unwrap();
        std::fs::write(&elsewhere, "Text.\n").unwrap();

        let text = ".. include:: ../doc/gc_info.rst\n   :start-line: 10\n";
        // The including file is outside the project but the target is inside:
        // the directive still follows the target.
        let out = follow_rst_includes(text, &elsewhere, &config, &Glossary::empty(), &factory());
        assert_eq!(out, ".. include:: ../doc/gc_info.rst\n   :start-line: 8\n");

        let text = ".. include:: readme.rst\n   :start-line: 10\n";
        let out = follow_rst_includes(text, &elsewhere, &config, &Glossary::empty(), &factory());
        assert_eq!(out, text);
    }

    #[test]
    fn include_targets_resolve_against_the_including_file() {
        assert_eq!(
            include_target(
                Path::new("upstream/pypy/doc/man/pypy.1.rst"),
                "../gc_info.rst"
            ),
            Some(PathBuf::from("upstream/pypy/doc/gc_info.rst"))
        );
        assert_eq!(
            include_target(Path::new("doc/a.rst"), "./sub/../b.rst"),
            Some(PathBuf::from("doc/b.rst"))
        );
        assert_eq!(
            include_target(Path::new("a.rst"), "../../b.rst"),
            Some(PathBuf::from("../../b.rst"))
        );
    }
}
