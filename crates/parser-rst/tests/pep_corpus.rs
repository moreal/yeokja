//! Corpus test against the official Python Enhancement Proposals submodule.
//!
//! PEP metadata must parse without becoming translation prose, and an empty
//! reconstruction must preserve every source byte.  PEP 9 is the historical
//! plaintext-template exception whose outer literal marker is intentionally
//! exposed by a dedicated parser.

use std::path::{Path, PathBuf};
use yeokja_core::parser::{DocumentParser, TranslationMap};
use yeokja_parser_rst::{PepParser, PepPlaintextParser, PepTextBlockParser};

fn corpus_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../projects/peps/upstream/peps")
}

#[test]
fn every_pep_parses_and_preserves_its_internet_message_header() {
    let root = corpus_root();
    if !root.is_dir() {
        return;
    }
    let mut files: Vec<PathBuf> = std::fs::read_dir(root)
        .unwrap()
        .filter_map(|entry| {
            let path = entry.ok()?.path();
            let name = path.file_name()?.to_str()?;
            (path.is_file() && name.starts_with("pep-") && name.ends_with(".rst")).then_some(path)
        })
        .collect();
    files.sort();
    assert!(files.len() > 700, "corpus should contain the complete PEP set");

    for path in files {
        let source = std::fs::read_to_string(&path).unwrap();
        let is_plaintext = path.file_name().is_some_and(|name| name == "pep-0009.rst");
        let has_prose_text_block = path.file_name().is_some_and(|name| name == "pep-0020.rst");
        let parser: &dyn DocumentParser = if is_plaintext {
            &PepPlaintextParser
        } else if has_prose_text_block {
            &PepTextBlockParser
        } else {
            &PepParser
        };
        let document = parser
            .parse_checked(&source)
            .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
        assert_eq!(
            parser.reconstruct(&document, &TranslationMap::new()),
            source,
            "{} does not survive an empty reconstruction",
            path.display()
        );

        assert!(
            !document.translatable_segments().is_empty(),
            "{} has no translation prose",
            path.display()
        );
        let preamble_end = source.find("\n\n").unwrap();
        for block in document.sections.iter().flat_map(|section| &section.blocks) {
            if block.span.as_ref().is_some_and(|span| span.start < preamble_end) {
                assert!(
                    source[block.span.clone().unwrap()].trim()
                        == source
                            .lines()
                            .find_map(|line| line.strip_prefix("Title:"))
                            .unwrap()
                            .trim(),
                    "{} exposes a non-Title metadata field as prose",
                    path.display()
                );
            }
        }
    }
}

#[test]
fn pep_416_preserves_the_target_of_its_multiline_recipe_link() {
    let path = corpus_root().join("pep-0416.rst");
    if !path.is_file() {
        return;
    }
    let source = std::fs::read_to_string(path).unwrap();
    let document = PepParser.parse_checked(&source).unwrap();
    let segment = document
        .translatable_segments()
        .into_iter()
        .find(|segment| {
            segment
                .source
                .starts_with("`make dictproxy object via ctypes.pythonapi")
        })
        .unwrap();
    let raw_block = document
        .sections
        .iter()
        .flat_map(|section| &section.blocks)
        .find(|block| block.segments.iter().any(|candidate| candidate.id == segment.id))
        .unwrap()
        .raw_content
        .clone();
    let mut translations = TranslationMap::new();
    translations.insert(segment.id.clone(), segment.source.clone());

    let rebuilt = PepParser.reconstruct(&document, &translations);
    assert!(rebuilt.contains(
        ".. _`make dictproxy object via ctypes.pythonapi and type() (Python recipe 576540)`: http://code.activestate.com/recipes/576540/"
    ), "raw block: {raw_block:?}; generated targets: {:?}", rebuilt.lines().filter(|line| line.starts_with(".. _`")).collect::<Vec<_>>());
}
