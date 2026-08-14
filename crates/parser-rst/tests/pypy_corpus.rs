//! Corpus test against the PyPy documentation submodule.
//!
//! Reconstruction must not restructure the document: replacing every segment
//! with its own text and parsing the result again has to yield the same
//! segments. A splice that breaks a literal block's indentation, eats a list
//! marker, or mangles a title adornment shows up here as a diverging parse.
//! The corpus lives in a submodule, so the test passes silently when it is
//! not checked out.

use std::path::{Path, PathBuf};
use yeokja_core::parser::{DocumentParser, TranslationMap};
use yeokja_parser_rst::RstParser;

fn corpus_dir() -> Option<PathBuf> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../projects/pypy/upstream/pypy/doc");
    dir.is_dir().then_some(dir)
}

fn rst_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            rst_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "rst") {
            out.push(path);
        }
    }
}

#[test]
fn self_translation_reparses_to_the_same_segments() {
    let Some(dir) = corpus_dir() else { return };
    let mut files = Vec::new();
    rst_files(&dir, &mut files);
    assert!(files.len() > 100, "corpus should hold the PyPy docs");

    let parser = RstParser;
    for path in files {
        let source = std::fs::read_to_string(&path).unwrap();
        let doc = parser.parse(&source);

        let mut translations = TranslationMap::new();
        for seg in doc.all_segments() {
            translations.insert(seg.id.clone(), seg.source.clone());
        }
        let output = parser.reconstruct(&doc, &translations);

        let reparsed = parser.parse(&output);
        let before: Vec<&str> = doc.all_segments().iter().map(|s| s.source.as_str()).collect();
        let after: Vec<&str> = reparsed
            .all_segments()
            .iter()
            .map(|s| s.source.as_str())
            .collect();
        assert_eq!(
            before,
            after,
            "{} parses differently after reconstruction",
            path.display()
        );
    }
}

#[test]
fn reconstruction_without_translations_is_identity() {
    let Some(dir) = corpus_dir() else { return };
    let mut files = Vec::new();
    rst_files(&dir, &mut files);

    let parser = RstParser;
    for path in files {
        let source = std::fs::read_to_string(&path).unwrap();
        let doc = parser.parse(&source);
        assert_eq!(
            parser.reconstruct(&doc, &TranslationMap::new()),
            source,
            "{} does not survive an empty reconstruction",
            path.display()
        );
    }
}
