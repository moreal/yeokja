//! Corpus tests for Mathematics in Lean's RST-in-Lean source format.
//!
//! The corpus lives in a submodule, so these tests pass silently when it has
//! not been checked out (for example when only the parser crate is packaged).

use std::path::{Path, PathBuf};
use yeokja_core::parser::{DocumentParser, TranslationMap};
use yeokja_parser_rst::MilParser;

fn corpus_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../projects/mil/upstream/MIL")
}
fn section_files(root: &Path) -> Vec<PathBuf> {
    if !root.is_dir() {
        return Vec::new();
    }
    let mut files = Vec::new();
    for chapter in std::fs::read_dir(root).unwrap() {
        let chapter = chapter.unwrap().path();
        if !chapter.is_dir()
            || !chapter
                .file_name()
                .is_some_and(|name| name.to_string_lossy().starts_with('C'))
        {
            continue;
        }
        for entry in std::fs::read_dir(chapter).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().is_some_and(|extension| extension == "lean")
                && path
                    .file_name()
                    .is_some_and(|name| name.to_string_lossy().starts_with('S'))
            {
                files.push(path);
            }
        }
    }
    files.sort();
    files
}

#[test]
fn every_section_parses_and_empty_reconstruction_is_identity() {
    let files = section_files(&corpus_root());
    if files.is_empty() {
        return;
    }
    assert!(files.len() > 40, "corpus should contain the complete textbook");

    for path in files {
        let source = std::fs::read_to_string(&path).unwrap();
        let document = MilParser
            .parse_checked(&source)
            .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
        assert!(!document.translatable_segments().is_empty(), "{}", path.display());
        assert_eq!(
            MilParser.reconstruct(&document, &TranslationMap::new()),
            source,
            "{} does not survive an empty reconstruction",
            path.display()
        );
    }
}
