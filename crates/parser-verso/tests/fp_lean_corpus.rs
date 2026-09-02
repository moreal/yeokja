//! Contract test for the checked-in official Verso source-range manifest.

use std::path::{Path, PathBuf};
use yeokja_core::parser::{DocumentParser, TranslationMap};
use yeokja_parser_verso::VersoParser;

fn project_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../projects/fp-lean")
}

fn lean_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            lean_files(&path, out);
        } else if path
            .extension()
            .is_some_and(|extension| extension == "lean")
        {
            out.push(path);
        }
    }
}

fn documents(root: &Path) -> Vec<PathBuf> {
    let mut files = vec![root.join("upstream/book/FPLean.lean")];
    lean_files(&root.join("upstream/book/FPLean"), &mut files);
    files.sort();
    files
        .into_iter()
        .filter(|path| {
            std::fs::read_to_string(path).is_ok_and(|source| {
                source
                    .lines()
                    .any(|line| line.trim_start().starts_with("#doc "))
            })
        })
        .collect()
}

#[test]
fn official_manifest_parses_every_fp_lean_manual() {
    let root = project_root();
    if !root.join("upstream/book/FPLean.lean").is_file() {
        return;
    }
    let manifest = root.join("verso-spans.json");
    let files = documents(&root);
    assert_eq!(files.len(), 70, "the FP in Lean manual corpus changed size");

    let mut segment_count = 0;
    for path in files {
        let source = std::fs::read_to_string(&path).unwrap();
        let parser = VersoParser::new(&path, &manifest);
        let document = parser.parse_checked(&source).unwrap_or_else(|error| {
            panic!("{}: {error}", path.display());
        });
        let segments = document.translatable_segments();
        assert!(
            !segments.is_empty(),
            "{} has no official spans",
            path.display()
        );
        segment_count += segments.len();

        for segment in segments {
            assert!(
                !segment.source.contains("```"),
                "code leaked in {}",
                path.display()
            );
            assert!(
                !segment.source.contains("tag :="),
                "metadata leaked in {}",
                path.display()
            );
            assert!(
                !segment.source.starts_with("{include "),
                "include leaked in {}",
                path.display()
            );
            assert!(
                !segment.source.contains("Copyright Microsoft"),
                "attribution became prose"
            );
            assert!(
                !segment.source.contains("/--") && !segment.source.contains("-/"),
                "doc comment delimiters leaked in {}",
                path.display()
            );
        }

        assert_eq!(
            parser.reconstruct(&document, &TranslationMap::new()),
            source,
            "{} changed without translations",
            path.display()
        );
    }

    assert!(
        segment_count > 5_000,
        "suspiciously little prose: {segment_count}"
    );
}

#[test]
fn manifest_declares_the_official_generator_and_pinned_revision() {
    let root = project_root();
    let path = root.join("verso-spans.json");
    if !path.is_file() {
        return;
    }
    let manifest: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    assert_eq!(manifest["schema"], 2);
    assert_eq!(manifest["generator"], "Verso.Parser.document");
    assert_eq!(
        manifest["versoRevision"],
        "aa44714115e9973999dfdde63130f725c3265a82"
    );
    assert_eq!(manifest["lakeManifest"], "upstream/book/lake-manifest.json");
    // 80 book modules plus the 37 example modules whose equational-step
    // justifications the book renders.
    assert_eq!(manifest["documents"].as_array().unwrap().len(), 117);
}

/// `anchorEqSteps` blocks display doc comments from the example modules and
/// require the book's payload to match them line by line, so the manifest
/// exports both copies as `doc_comment` spans.
#[test]
fn equational_step_justifications_are_exported_from_both_copies() {
    let root = project_root();
    if !root.join("upstream/book/FPLean.lean").is_file() {
        return;
    }
    let manifest_path = root.join("verso-spans.json");
    let manifest: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&manifest_path).unwrap()).unwrap();

    let mut book_justifications = 0;
    let mut example_justifications = 0;
    for document in manifest["documents"].as_array().unwrap() {
        let relative = document["path"].as_str().unwrap();
        let doc_comments = document["spans"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|span| span["kind"] == "doc_comment")
            .count();
        let is_example = relative.starts_with("upstream/examples/");
        if is_example {
            example_justifications += doc_comments;
            let path = root.join(relative);
            let source = std::fs::read_to_string(&path).unwrap();
            let parser = VersoParser::new(&path, &manifest_path);
            let parsed = parser
                .parse_checked(&source)
                .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
            assert_eq!(parsed.translatable_segments().len(), doc_comments);
            assert_eq!(
                parser.reconstruct(&parsed, &TranslationMap::new()),
                source,
                "{} changed without translations",
                path.display()
            );
        } else {
            book_justifications += doc_comments;
        }
    }

    assert_eq!(book_justifications, 31, "book eq-step justifications");
    assert_eq!(example_justifications, 32, "example eq-step justifications");

    let contract = root.join("upstream/book/FPLean/FunctorApplicativeMonad/ApplicativeContract.lean");
    let source = std::fs::read_to_string(&contract).unwrap();
    let document = VersoParser::new(&contract, &manifest_path)
        .parse_checked(&source)
        .unwrap();
    let sources: Vec<_> = document
        .translatable_segments()
        .iter()
        .map(|segment| segment.source.clone())
        .collect();
    assert!(sources.iter().any(|text| text == "Definition of `seq`"));
    assert!(sources.iter().any(|text| {
        text == "Clever replacement of one expression by an equivalent one that makes the rule match"
    }));
}
