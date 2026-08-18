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
    assert_eq!(manifest["schema"], 1);
    assert_eq!(manifest["generator"], "Verso.Parser.document");
    assert_eq!(
        manifest["versoRevision"],
        "aa44714115e9973999dfdde63130f725c3265a82"
    );
    assert_eq!(manifest["documents"].as_array().unwrap().len(), 80);
}
