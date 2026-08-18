//! Official-AST-backed parser for Verso manuals embedded in Lean source.
//!
//! Yeokja does not duplicate Verso's grammar. A Lean extractor built against
//! the document project's pinned Verso revision runs `Verso.Parser.document`
//! and records the source ranges of natural-language nodes. This crate strictly
//! validates that manifest against the current source and Lake pin, converts
//! the ranges to Yeokja blocks, and performs lossless splice reconstruction.

use serde::Deserialize;
use std::path::{Path, PathBuf};
use yeokja_core::model::*;
use yeokja_core::parser::{DocumentParseError, DocumentParser, Markup, TranslationMap};
use yeokja_parser_utils::{make_segments, normalize_inline_text, splice_reconstruct};

const MANIFEST_SCHEMA: u32 = 1;
const OFFICIAL_GENERATOR: &str = "Verso.Parser.document";

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SpanManifest {
    schema: u32,
    generator: String,
    verso_revision: String,
    documents: Vec<ManifestDocument>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ManifestDocument {
    path: String,
    source_hash: String,
    spans: Vec<ManifestSpan>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ManifestSpan {
    start: usize,
    stop: usize,
    kind: SpanKind,
    level: Option<u8>,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
enum SpanKind {
    Heading,
    Paragraph,
    ListItem,
    BlockQuote,
    Table,
}

impl SpanKind {
    fn block_type(self) -> BlockType {
        match self {
            Self::Heading => BlockType::Heading,
            Self::Paragraph => BlockType::Paragraph,
            Self::ListItem => BlockType::ListItem,
            Self::BlockQuote => BlockType::BlockQuote,
            Self::Table => BlockType::Table,
        }
    }
}

/// A Verso source plus the official-parser manifest that describes it.
pub struct VersoParser {
    source_path: PathBuf,
    manifest_path: PathBuf,
}

impl VersoParser {
    pub fn new(source_path: impl Into<PathBuf>, manifest_path: impl Into<PathBuf>) -> Self {
        Self {
            source_path: source_path.into(),
            manifest_path: manifest_path.into(),
        }
    }

    fn parse_official(&self, source: &str) -> Result<Document, DocumentParseError> {
        let manifest_text = std::fs::read_to_string(&self.manifest_path).map_err(|error| {
            self.error(format!(
                "cannot read official Verso span manifest {}: {error}; run its update script",
                self.manifest_path.display()
            ))
        })?;
        let manifest: SpanManifest = serde_json::from_str(&manifest_text).map_err(|error| {
            self.error(format!(
                "invalid official Verso span manifest {}: {error}",
                self.manifest_path.display()
            ))
        })?;
        self.validate_manifest_header(&manifest)?;

        let key = source_manifest_key(&self.source_path, &self.manifest_path);
        let entry = manifest
            .documents
            .iter()
            .find(|document| normalized_manifest_key(&document.path) == key)
            .ok_or_else(|| {
                self.error(format!(
                    "{} has no entry in {}; regenerate the official Verso spans",
                    self.source_path.display(),
                    self.manifest_path.display()
                ))
            })?;

        let actual_hash = fnv1a64(source).to_string();
        if entry.source_hash != actual_hash {
            return Err(self.error(format!(
                "official Verso spans are stale for {}: manifest hash {}, source hash {}; regenerate {}",
                self.source_path.display(),
                entry.source_hash,
                actual_hash,
                self.manifest_path.display()
            )));
        }

        self.document_from_spans(source, &entry.spans)
    }

    fn validate_manifest_header(&self, manifest: &SpanManifest) -> Result<(), DocumentParseError> {
        if manifest.schema != MANIFEST_SCHEMA {
            return Err(self.error(format!(
                "unsupported Verso span manifest schema {} (expected {MANIFEST_SCHEMA})",
                manifest.schema
            )));
        }
        if manifest.generator != OFFICIAL_GENERATOR {
            return Err(self.error(format!(
                "Verso spans were produced by {:?}, not the official {OFFICIAL_GENERATOR}",
                manifest.generator
            )));
        }
        let pinned =
            pinned_verso_revision(&self.source_path).map_err(|message| self.error(message))?;
        if pinned != manifest.verso_revision {
            return Err(self.error(format!(
                "Verso revision mismatch: {} pins {pinned}, but {} was generated with {}; regenerate it",
                self.source_path.display(),
                self.manifest_path.display(),
                manifest.verso_revision
            )));
        }
        Ok(())
    }

    fn document_from_spans(
        &self,
        source: &str,
        spans: &[ManifestSpan],
    ) -> Result<Document, DocumentParseError> {
        validate_spans(source, spans).map_err(|message| self.error(message.to_string()))?;

        let mut sections = vec![Section { blocks: Vec::new() }];
        let mut section_idx = 0usize;
        let mut block_idx = 0usize;

        for span in spans {
            let block_type = span.kind.block_type();
            if block_type == BlockType::Heading
                && span.level.is_some_and(|level| level <= 2)
                && !sections.last().unwrap().blocks.is_empty()
            {
                sections.push(Section { blocks: Vec::new() });
                section_idx += 1;
                block_idx = 0;
            }

            let range = span.start..span.stop;
            let raw = &source[range.clone()];
            let normalized = normalize_inline_text(raw);
            let segments = make_segments(&normalized, block_type, section_idx, block_idx);
            sections.last_mut().unwrap().blocks.push(Block {
                block_type,
                segments,
                raw_content: raw.to_string(),
                heading_level: span.level,
                span: Some(range),
                role: BlockRole::None,
                translatable: block_type.is_translatable(),
            });
            block_idx += 1;
        }

        sections.retain(|section| !section.blocks.is_empty());
        Ok(Document {
            sections,
            source: source.to_string(),
        })
    }

    fn error(&self, message: String) -> DocumentParseError {
        DocumentParseError(format!("Verso parse error: {message}"))
    }
}

impl DocumentParser for VersoParser {
    fn markup(&self) -> Markup {
        Markup::Verso
    }

    fn parse(&self, source: &str) -> Document {
        self.parse_official(source)
            .unwrap_or_else(|error| panic!("{error}"))
    }

    fn parse_checked(&self, source: &str) -> Result<Document, DocumentParseError> {
        self.parse_official(source)
    }

    fn reconstruct(&self, document: &Document, translations: &TranslationMap) -> String {
        let mut reconstructed = splice_reconstruct(document, translations);
        if document_title_is_translated(document, translations) {
            ensure_document_tag(&mut reconstructed, &self.source_path);
            replace_disabled_part_tags(&mut reconstructed, &self.source_path);
        }
        reconstructed
    }
}

fn document_title_is_translated(document: &Document, translations: &TranslationMap) -> bool {
    document
        .sections
        .iter()
        .flat_map(|section| &section.blocks)
        .filter(|block| block.block_type == BlockType::Heading && block.heading_level == Some(1))
        .min_by_key(|block| {
            block
                .span
                .as_ref()
                .map(|span| span.start)
                .unwrap_or(usize::MAX)
        })
        .is_some_and(|block| {
            block
                .segments
                .iter()
                .any(|segment| translations.contains_key(&segment.id))
        })
}

/// Give translated `#doc` parts a stable ASCII tag when upstream did not.
///
/// Verso auto-tags mangle every non-ASCII character to `___`, so unrelated
/// Korean titles of the same length collide when independently elaborated
/// parts are assembled. Explicit upstream tags remain authoritative. For the
/// few documents without one, a source-path hash is stable across translations
/// and unique within a project.
fn ensure_document_tag(reconstructed: &mut String, source_path: &Path) {
    let Some(doc_start) = reconstructed.find("#doc ") else {
        return;
    };
    let Some(relative_line_end) = reconstructed[doc_start..].find('\n') else {
        return;
    };
    let body_start = doc_start + relative_line_end + 1;
    let first_content = reconstructed[body_start..]
        .find(|ch: char| !ch.is_whitespace())
        .map(|offset| body_start + offset)
        .unwrap_or(reconstructed.len());
    let tag = format!(
        "yeokja-doc-{:016x}",
        fnv1a64(&source_path.to_string_lossy())
    );

    if reconstructed[first_content..].starts_with("%%%") {
        let metadata_body = first_content + 3;
        let Some(relative_close) = reconstructed[metadata_body..].find("\n%%%") else {
            return;
        };
        let close = metadata_body + relative_close;
        if reconstructed[metadata_body..close].contains("tag :=") {
            return;
        }
        reconstructed.insert_str(close, &format!("\ntag := \"{tag}\""));
    } else {
        reconstructed.insert_str(body_start, &format!("%%%\ntag := \"{tag}\"\n%%%\n"));
    }
}

/// Replace `tag := none` on translated parts with stable, unique ASCII tags.
///
/// Upstream uses `none` when an English heading's auto-generated tag is good
/// enough. Korean characters all mangle to the same `___`, so two unrelated
/// headings with the same character count can receive the same internal tag
/// when separately elaborated documents are combined.
fn replace_disabled_part_tags(reconstructed: &mut String, source_path: &Path) {
    let path_hash = fnv1a64(&source_path.to_string_lossy());
    let mut search_from = 0;
    let mut index = 0usize;
    while let Some(relative) = reconstructed[search_from..].find("tag := none") {
        let start = search_from + relative;
        let end = start + "tag := none".len();
        let replacement = format!("tag := \"yeokja-part-{path_hash:016x}-{index}\"");
        reconstructed.replace_range(start..end, &replacement);
        search_from = start + replacement.len();
        index += 1;
    }
}

fn validate_spans(source: &str, spans: &[ManifestSpan]) -> Result<(), DocumentParseError> {
    let mut prior_stop = 0usize;
    for (index, span) in spans.iter().enumerate() {
        if span.start >= span.stop {
            return Err(DocumentParseError(format!(
                "span {index} is empty or reversed: {}..{}",
                span.start, span.stop
            )));
        }
        if span.stop > source.len()
            || !source.is_char_boundary(span.start)
            || !source.is_char_boundary(span.stop)
        {
            return Err(DocumentParseError(format!(
                "span {index} is outside UTF-8 source boundaries: {}..{} for {} bytes",
                span.start,
                span.stop,
                source.len()
            )));
        }
        if span.start < prior_stop {
            return Err(DocumentParseError(format!(
                "span {index} overlaps or is out of order: {}..{} follows byte {prior_stop}",
                span.start, span.stop
            )));
        }
        match span.kind {
            SpanKind::Heading if span.level.is_none() => {
                return Err(DocumentParseError(format!(
                    "heading span {index} has no level"
                )));
            }
            SpanKind::Heading => {}
            _ if span.level.is_some() => {
                return Err(DocumentParseError(format!(
                    "non-heading span {index} unexpectedly has a level"
                )));
            }
            _ => {}
        }
        prior_stop = span.stop;
    }
    Ok(())
}

fn fnv1a64(source: &str) -> u64 {
    source
        .as_bytes()
        .iter()
        .fold(0xcbf29ce484222325, |hash, byte| {
            (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3)
        })
}

fn normalized_manifest_key(path: &str) -> String {
    path.trim_start_matches("./").replace('\\', "/")
}

fn source_manifest_key(source_path: &Path, manifest_path: &Path) -> String {
    let absolute_manifest = if manifest_path.is_absolute() {
        manifest_path.to_path_buf()
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(manifest_path)
    };
    if source_path.is_absolute()
        && let Some(project_root) = absolute_manifest.parent()
        && let Ok(relative) = source_path.strip_prefix(project_root)
    {
        return normalized_manifest_key(&relative.to_string_lossy());
    }
    normalized_manifest_key(&source_path.to_string_lossy())
}

fn pinned_verso_revision(source_path: &Path) -> Result<String, String> {
    let start = if source_path.is_absolute() {
        source_path.parent()
    } else {
        source_path.parent().or(Some(Path::new(".")))
    };
    for directory in start.into_iter().flat_map(Path::ancestors) {
        let candidate = directory.join("lake-manifest.json");
        if !candidate.is_file() {
            continue;
        }
        let text = std::fs::read_to_string(&candidate)
            .map_err(|error| format!("cannot read {}: {error}", candidate.display()))?;
        let manifest: serde_json::Value = serde_json::from_str(&text)
            .map_err(|error| format!("invalid {}: {error}", candidate.display()))?;
        let packages = manifest
            .get("packages")
            .and_then(serde_json::Value::as_array)
            .or_else(|| manifest.as_array())
            .ok_or_else(|| format!("{} has no packages array", candidate.display()))?;
        let revision = packages.iter().find_map(|package| {
            (package.get("name").and_then(|value| value.as_str()) == Some("verso"))
                .then(|| package.get("rev")?.as_str().map(str::to_owned))
                .flatten()
        });
        return revision
            .ok_or_else(|| format!("{} does not pin a verso package", candidate.display()));
    }
    Err(format!(
        "no lake-manifest.json found above {}; the official Verso revision cannot be verified",
        source_path.display()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    struct Fixture {
        _temp: tempfile::TempDir,
        source_path: PathBuf,
        manifest_path: PathBuf,
        source: String,
    }

    impl Fixture {
        fn new(source: &str, ranges: &[(&str, usize, usize, Option<u8>)]) -> Self {
            let temp = tempfile::tempdir().unwrap();
            let book = temp.path().join("upstream/book");
            std::fs::create_dir_all(book.join("FPLean")).unwrap();
            let source_path = book.join("FPLean/Chapter.lean");
            std::fs::write(&source_path, source).unwrap();
            std::fs::write(
                book.join("lake-manifest.json"),
                r#"[{"name":"verso","rev":"official-revision"}]"#,
            )
            .unwrap();
            let manifest_path = temp.path().join("verso-spans.json");
            let spans: Vec<_> = ranges
                .iter()
                .map(|(kind, start, stop, level)| {
                    json!({"start": start, "stop": stop, "kind": kind, "level": level})
                })
                .collect();
            std::fs::write(
                &manifest_path,
                serde_json::to_string(&json!({
                    "schema": 1,
                    "generator": OFFICIAL_GENERATOR,
                    "versoRevision": "official-revision",
                    "documents": [{
                        "path": "upstream/book/FPLean/Chapter.lean",
                        "sourceHash": fnv1a64(source).to_string(),
                        "spans": spans,
                    }]
                }))
                .unwrap(),
            )
            .unwrap();
            Self {
                _temp: temp,
                source_path,
                manifest_path,
                source: source.to_string(),
            }
        }

        fn parser(&self) -> VersoParser {
            VersoParser::new(&self.source_path, &self.manifest_path)
        }
    }

    #[test]
    fn builds_blocks_from_official_ranges_and_splices_translations() {
        let source = "#doc (Manual) \"Title\" =>\n\n# Section\n\nBody text.\n";
        let title = source.find("Title").unwrap();
        let section = source.find("Section").unwrap();
        let body = source.find("Body text.").unwrap();
        let fixture = Fixture::new(
            source,
            &[
                ("heading", title, title + 5, Some(1)),
                ("heading", section, section + 7, Some(1)),
                ("paragraph", body, body + 10, None),
            ],
        );
        let parser = fixture.parser();
        let document = parser.parse_checked(&fixture.source).unwrap();
        assert_eq!(document.sections.len(), 2);
        assert_eq!(document.translatable_segments().len(), 3);

        let mut translations = TranslationMap::new();
        let segments = document.translatable_segments();
        translations.insert(segments[0].id.clone(), "제목".to_string());
        translations.insert(segments[2].id.clone(), "본문입니다.".to_string());
        let output = parser.reconstruct(&document, &translations);
        assert!(output.contains("#doc (Manual) \"제목\" =>"));
        assert!(output.contains("tag := \"yeokja-doc-"));
        assert!(output.contains("본문입니다."));
        assert!(output.contains("# Section"));
    }

    #[test]
    fn translated_document_title_merges_a_tag_into_existing_metadata() {
        let source = "#doc (Manual) \"Title\" =>\n%%%\nauthors := [\"Author\"]\n%%%\n\nBody.\n";
        let title = source.find("Title").unwrap();
        let body = source.find("Body.").unwrap();
        let fixture = Fixture::new(
            source,
            &[
                ("heading", title, title + 5, Some(1)),
                ("paragraph", body, body + 5, None),
            ],
        );
        let parser = fixture.parser();
        let document = parser.parse_checked(&fixture.source).unwrap();
        let title_id = document.translatable_segments()[0].id.clone();
        let output = parser.reconstruct(
            &document,
            &TranslationMap::from([(title_id, "제목".to_string())]),
        );

        assert!(output.contains("authors := [\"Author\"]\ntag := \"yeokja-doc-"));
        assert_eq!(output.matches("%%%").count(), 2);
    }

    #[test]
    fn an_explicit_document_tag_remains_authoritative() {
        let source = "#doc (Manual) \"Title\" =>\n%%%\ntag := \"upstream-tag\"\n%%%\n\nBody.\n";
        let title = source.find("Title").unwrap();
        let body = source.find("Body.").unwrap();
        let fixture = Fixture::new(
            source,
            &[
                ("heading", title, title + 5, Some(1)),
                ("paragraph", body, body + 5, None),
            ],
        );
        let parser = fixture.parser();
        let document = parser.parse_checked(&fixture.source).unwrap();
        let title_id = document.translatable_segments()[0].id.clone();
        let output = parser.reconstruct(
            &document,
            &TranslationMap::from([(title_id, "제목".to_string())]),
        );

        assert!(output.contains("tag := \"upstream-tag\""));
        assert!(!output.contains("yeokja-doc-"));
        assert_eq!(output.matches("tag :=").count(), 1);
    }

    #[test]
    fn translated_parts_replace_disabled_auto_tags_with_stable_tags() {
        let source = "#doc (Manual) \"Title\" =>\n%%%\ntag := \"doc-tag\"\n%%%\n\n# One\n%%%\ntag := none\n%%%\n\n# Two\n%%%\ntag := none\n%%%\n";
        let title = source.find("Title").unwrap();
        let one = source.find("One").unwrap();
        let two = source.find("Two").unwrap();
        let fixture = Fixture::new(
            source,
            &[
                ("heading", title, title + 5, Some(1)),
                ("heading", one, one + 3, Some(1)),
                ("heading", two, two + 3, Some(1)),
            ],
        );
        let parser = fixture.parser();
        let document = parser.parse_checked(&fixture.source).unwrap();
        let translations = document
            .translatable_segments()
            .iter()
            .map(|segment| (segment.id.clone(), format!("번역-{}", segment.id)))
            .collect();
        let output = parser.reconstruct(&document, &translations);

        assert!(!output.contains("tag := none"));
        assert_eq!(output.matches("tag := \"yeokja-part-").count(), 2);
        assert!(output.contains("-0\""));
        assert!(output.contains("-1\""));
    }

    #[test]
    fn stale_source_hash_is_a_hard_error() {
        let fixture = Fixture::new("Body text.", &[("paragraph", 0, 10, None)]);
        let error = fixture.parser().parse_checked("Changed text.").unwrap_err();
        assert!(error.to_string().contains("spans are stale"));
    }

    #[test]
    fn verso_revision_mismatch_is_a_hard_error() {
        let fixture = Fixture::new("Body text.", &[("paragraph", 0, 10, None)]);
        let text = std::fs::read_to_string(&fixture.manifest_path).unwrap();
        std::fs::write(
            &fixture.manifest_path,
            text.replace("official-revision", "old-revision"),
        )
        .unwrap();
        let error = fixture.parser().parse_checked(&fixture.source).unwrap_err();
        assert!(error.to_string().contains("revision mismatch"));
    }

    #[test]
    fn overlapping_ranges_are_rejected() {
        let fixture = Fixture::new(
            "First. Second.",
            &[("paragraph", 0, 6, None), ("paragraph", 5, 14, None)],
        );
        let error = fixture.parser().parse_checked(&fixture.source).unwrap_err();
        assert!(error.to_string().contains("overlaps"));
    }

    #[test]
    fn missing_document_entry_is_a_hard_error() {
        let fixture = Fixture::new("Body text.", &[("paragraph", 0, 10, None)]);
        let text = std::fs::read_to_string(&fixture.manifest_path).unwrap();
        std::fs::write(
            &fixture.manifest_path,
            text.replace("Chapter.lean", "Elsewhere.lean"),
        )
        .unwrap();
        let error = fixture.parser().parse_checked(&fixture.source).unwrap_err();
        assert!(error.to_string().contains("has no entry"));
    }
}
