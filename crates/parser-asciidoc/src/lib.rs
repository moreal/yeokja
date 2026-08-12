//! Span-based AsciiDoc parser.
//!
//! AsciiDoc block structure is line-oriented, so this parser scans lines while
//! tracking byte offsets into the original source. Each translatable block
//! records the byte range (`Block::span`) of its text content — excluding
//! heading markers, list bullets, admonition labels, and delimiters — and
//! segments keep the raw inline markup (`*bold*`, `https://...[link]`, etc.).
//! Reconstruction splices translations into the original source, preserving
//! everything outside the translated spans byte-for-byte: attribute entries,
//! anchors, block attribute lines, delimited blocks, comments, and tables.

use std::ops::Range;
use yeokja_core::model::*;
use yeokja_core::parser::{DocumentParser, TranslationMap};
use yeokja_parser_utils::{make_segments, normalize_inline_text, splice_reconstruct};

pub struct AsciidocParser;

const ADMONITION_LABELS: [&str; 5] = ["NOTE:", "TIP:", "IMPORTANT:", "WARNING:", "CAUTION:"];

struct ParseState<'a> {
    source: &'a str,
    sections: Vec<Section>,
    section_idx: usize,
    block_idx: usize,
    /// Currently accumulating translatable run (paragraph or list item).
    current: Option<(BlockType, Range<usize>)>,
    /// Open opaque delimited block (listing/literal/passthrough/comment/table):
    /// the trimmed delimiter line that must appear again to close it, plus the
    /// block's starting offset.
    opaque: Option<(String, usize)>,
    /// Open container delimiters (`____`, `====`, `****`); content inside is
    /// parsed normally, `____` labels its content as BlockQuote.
    containers: Vec<String>,
    /// Inside the document header (title author/revision lines).
    in_doc_header: bool,
    seen_content: bool,
}

impl ParseState<'_> {
    fn flush_current(&mut self) {
        let Some((block_type, span)) = self.current.take() else {
            return;
        };
        let raw = &self.source[span.clone()];
        if raw.trim().is_empty() {
            return;
        }
        let normalized = normalize_inline_text(raw);
        let segments = make_segments(&normalized, block_type, self.section_idx, self.block_idx);
        self.push_block(Block {
            block_type,
            segments,
            raw_content: raw.to_string(),
            heading_level: None,
            span: Some(span),
        });
    }

    fn push_block(&mut self, block: Block) {
        self.sections.last_mut().unwrap().blocks.push(block);
        self.block_idx += 1;
        self.seen_content = true;
    }

    /// Extend the current run (or start a new one of `block_type`).
    fn extend_current(&mut self, block_type: BlockType, range: Range<usize>) {
        match &mut self.current {
            Some((_, span)) => span.end = range.end,
            None => self.current = Some((block_type, range)),
        }
    }

    /// Type for plain text at this position: BlockQuote inside `____`.
    fn text_type(&self) -> BlockType {
        if self.containers.iter().any(|d| d.starts_with('_')) {
            BlockType::BlockQuote
        } else {
            BlockType::Paragraph
        }
    }

    fn start_section_if_needed(&mut self, level: u8) {
        if level <= 2 && !self.sections.last().unwrap().blocks.is_empty() {
            self.sections.push(Section { blocks: Vec::new() });
            self.section_idx += 1;
            self.block_idx = 0;
        }
    }
}

/// `----`, `....`, `++++`, `////` (comment), `|===` (table) open opaque blocks.
fn opaque_delimiter(trimmed: &str) -> bool {
    let all_of = |c: char| trimmed.len() >= 4 && trimmed.chars().all(|x| x == c);
    all_of('-') || all_of('.') || all_of('+') || all_of('/')
        || (trimmed.starts_with('|') && trimmed[1..].len() >= 3 && trimmed[1..].chars().all(|c| c == '='))
}

/// `____`, `====`, `****` open/close container blocks parsed normally inside.
fn container_delimiter(trimmed: &str) -> bool {
    let all_of = |c: char| trimmed.len() >= 4 && trimmed.chars().all(|x| x == c);
    all_of('_') || all_of('=') || all_of('*')
}

/// `= Heading` → (level, byte offset of the text within the line).
fn parse_heading(content: &str) -> Option<(u8, usize)> {
    let level = content.chars().take_while(|c| *c == '=').count();
    if level == 0 || level > 6 {
        return None;
    }
    let rest = &content[level..];
    let ws = rest.len() - rest.trim_start().len();
    if ws == 0 || rest.trim().is_empty() {
        return None;
    }
    Some((level as u8, level + ws))
}

/// `* item`, `- item`, `. item`, `12. item` → byte offset of the item text.
fn parse_list_item(content: &str) -> Option<usize> {
    let trimmed_start = content.len() - content.trim_start().len();
    let rest = &content[trimmed_start..];

    let first = rest.chars().next()?;
    let marker_len = match first {
        '*' | '.' => rest.chars().take_while(|c| *c == first).count(),
        '-' => 1,
        _ => {
            let dot = rest
                .find(". ")
                .filter(|&d| d > 0 && rest[..d].chars().all(|c| c.is_ascii_digit()))?;
            dot + 1
        }
    };

    let after = &rest[marker_len..];
    let ws = after.len() - after.trim_start().len();
    if ws == 0 || after.trim().is_empty() {
        return None; // no space after marker → not a list item
    }
    Some(trimmed_start + marker_len + ws)
}

/// `:attr: value` attribute entry line.
fn is_attribute_entry(trimmed: &str) -> bool {
    if !trimmed.starts_with(':') {
        return false;
    }
    match trimmed[1..].find(':') {
        Some(end) => !trimmed[1..1 + end].contains(char::is_whitespace) && end > 0,
        None => false,
    }
}

/// `NOTE: text` → byte offset of the text after the label.
fn parse_admonition(content: &str) -> Option<usize> {
    for label in ADMONITION_LABELS {
        if let Some(rest) = content.strip_prefix(label) {
            let ws = rest.len() - rest.trim_start().len();
            if ws > 0 && !rest.trim().is_empty() {
                return Some(label.len() + ws);
            }
        }
    }
    None
}

/// `.Block Title` (dot followed by non-space, non-dot) → offset of the title.
fn parse_block_title(content: &str) -> Option<usize> {
    let rest = content.strip_prefix('.')?;
    let first = rest.chars().next()?;
    if first == '.' || first.is_whitespace() {
        return None;
    }
    Some(1)
}

impl DocumentParser for AsciidocParser {
    fn parse(&self, source: &str) -> Document {
        let mut state = ParseState {
            source,
            sections: vec![Section { blocks: Vec::new() }],
            section_idx: 0,
            block_idx: 0,
            current: None,
            opaque: None,
            containers: Vec::new(),
            in_doc_header: false,
            seen_content: false,
        };

        let mut offset = 0usize;
        for line in source.split_inclusive('\n') {
            let line_start = offset;
            offset += line.len();
            let content = line.strip_suffix('\n').unwrap_or(line);
            let content = content.strip_suffix('\r').unwrap_or(content);
            let content_end = line_start + content.len();
            let trimmed = content.trim();

            // Inside an opaque delimited block: only the matching closer matters.
            if let Some((delimiter, start)) = &state.opaque {
                if trimmed == delimiter {
                    let block_type = if delimiter.starts_with('|') {
                        BlockType::Table
                    } else if delimiter.starts_with('/') {
                        BlockType::HtmlBlock
                    } else {
                        BlockType::CodeBlock
                    };
                    let raw = source[*start..content_end].to_string();
                    state.push_block(Block {
                        block_type,
                        segments: Vec::new(),
                        raw_content: raw,
                        heading_level: None,
                        span: None,
                    });
                    state.opaque = None;
                }
                continue;
            }

            // Blank line ends the current paragraph/list item (and the header).
            if trimmed.is_empty() {
                state.flush_current();
                state.in_doc_header = false;
                continue;
            }

            // Document header: skip author/revision lines after the doc title.
            if state.in_doc_header {
                continue;
            }

            if opaque_delimiter(trimmed) {
                state.flush_current();
                state.opaque = Some((trimmed.to_string(), line_start));
                continue;
            }

            if container_delimiter(trimmed) {
                state.flush_current();
                if state.containers.last().map(String::as_str) == Some(trimmed) {
                    state.containers.pop();
                } else {
                    state.containers.push(trimmed.to_string());
                }
                continue;
            }

            if let Some((level, text_offset)) = parse_heading(content) {
                state.flush_current();
                let is_doc_title = level == 1 && !state.seen_content;
                state.start_section_if_needed(level);
                let span = line_start + text_offset..content_end;
                let raw = &source[span.clone()];
                let segments = make_segments(
                    &normalize_inline_text(raw),
                    BlockType::Heading,
                    state.section_idx,
                    state.block_idx,
                );
                state.push_block(Block {
                    block_type: BlockType::Heading,
                    segments,
                    raw_content: raw.to_string(),
                    heading_level: Some(level),
                    span: Some(span),
                });
                if is_doc_title {
                    state.in_doc_header = true;
                }
                continue;
            }

            // Comment line, attribute entry, anchor, or block attribute line.
            if trimmed.starts_with("//")
                || is_attribute_entry(trimmed)
                || (trimmed.starts_with('[') && trimmed.ends_with(']'))
            {
                state.flush_current();
                continue;
            }

            // List continuation marker joins blocks to an item; treat as boundary.
            if trimmed == "+" {
                state.flush_current();
                continue;
            }

            if let Some(text_offset) = parse_list_item(content) {
                state.flush_current();
                state.current =
                    Some((BlockType::ListItem, line_start + text_offset..content_end));
                continue;
            }

            if let Some(text_offset) = parse_admonition(content) {
                state.flush_current();
                state.current =
                    Some((state.text_type(), line_start + text_offset..content_end));
                continue;
            }

            if state.current.is_none()
                && let Some(text_offset) = parse_block_title(content)
            {
                // Block title (`.Installation`) — translatable single line.
                state.current =
                    Some((BlockType::Paragraph, line_start + text_offset..content_end));
                state.flush_current();
                continue;
            }

            // Plain text: continue the current run or start a paragraph.
            let text_type = match &state.current {
                Some((block_type, _)) => *block_type,
                None => state.text_type(),
            };
            let start = line_start + (content.len() - content.trim_start().len());
            state.extend_current(text_type, start..content_end);
        }

        state.flush_current();

        let mut sections = state.sections;
        sections.retain(|s| !s.blocks.is_empty());

        Document {
            sections,
            source: source.to_string(),
        }
    }

    fn reconstruct(&self, document: &Document, translations: &TranslationMap) -> String {
        splice_reconstruct(document, translations)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_simple_paragraph() {
        let parser = AsciidocParser;
        let doc = parser.parse("Hello world. Goodbye world.");
        let segments = doc.translatable_segments();
        assert_eq!(segments.len(), 2);
        assert_eq!(segments[0].source, "Hello world.");
        assert_eq!(segments[1].source, "Goodbye world.");
    }

    #[test]
    fn parse_heading() {
        let parser = AsciidocParser;
        let doc = parser.parse("== Chapter Title\n\nSome text.");
        let all_segs = doc.translatable_segments();
        assert!(all_segs.iter().any(|s| s.source == "Chapter Title"));
        assert!(all_segs.iter().any(|s| s.source.contains("Some text")));
    }

    #[test]
    fn heading_creates_section() {
        let parser = AsciidocParser;
        let doc = parser.parse("= Chapter 1\n\nText one.\n\n== Chapter 2\n\nText two.");
        assert!(doc.sections.len() >= 2);
    }

    #[test]
    fn code_block_not_translatable() {
        let parser = AsciidocParser;
        let doc = parser.parse("Some text.\n\n----\nfn main() {}\n----\n\nMore text.");
        let translatable = doc.translatable_segments();
        for seg in &translatable {
            assert_ne!(seg.block_type, BlockType::CodeBlock);
        }
        assert_eq!(translatable.len(), 2);
    }

    #[test]
    fn parse_list_items() {
        let parser = AsciidocParser;
        let doc = parser.parse("* First item\n* Second item\n* Third item");
        let segments = doc.translatable_segments();
        assert_eq!(segments.len(), 3);
        assert_eq!(segments[0].block_type, BlockType::ListItem);
    }

    #[test]
    fn parse_block_quote() {
        let parser = AsciidocParser;
        let doc = parser.parse("[quote]\n____\nThis is a quote.\n____");
        let segments = doc.translatable_segments();
        assert!(segments.iter().any(|s| s.block_type == BlockType::BlockQuote));
    }

    #[test]
    fn reconstruct_with_translations() {
        let parser = AsciidocParser;
        let doc = parser.parse("Hello world.");
        let segments = doc.translatable_segments();
        assert!(!segments.is_empty());
        let mut translations = TranslationMap::new();
        translations.insert(segments[0].id.clone(), "안녕하세요.".to_string());
        let output = parser.reconstruct(&doc, &translations);
        assert_eq!(output, "안녕하세요.");
    }

    #[test]
    fn reconstruct_heading_preserves_level() {
        let parser = AsciidocParser;
        let doc = parser.parse("= Doc Title\n\n== Level 2 Heading");
        let segments = doc.translatable_segments();
        let heading_seg = segments.iter().find(|s| s.source == "Level 2 Heading").unwrap();
        let mut translations = TranslationMap::new();
        translations.insert(heading_seg.id.clone(), "레벨 2 제목".to_string());
        let output = parser.reconstruct(&doc, &translations);
        assert!(output.contains("== 레벨 2 제목"));
    }

    #[test]
    fn reconstruct_falls_back_to_original() {
        let parser = AsciidocParser;
        let doc = parser.parse("Hello world.");
        let translations = TranslationMap::new();
        let output = parser.reconstruct(&doc, &translations);
        assert!(output.contains("Hello world."));
    }

    #[test]
    fn heading_level_preserved() {
        let parser = AsciidocParser;
        let doc = parser.parse("= Level 1\n\n== Level 2\n\n=== Level 3");
        let blocks: Vec<&Block> = doc.sections.iter().flat_map(|s| &s.blocks).collect();
        let headings: Vec<&Block> = blocks.iter().filter(|b| b.block_type == BlockType::Heading).copied().collect();
        assert!(headings.len() >= 3);
        assert_eq!(headings[0].heading_level, Some(1));
        assert_eq!(headings[1].heading_level, Some(2));
        assert_eq!(headings[2].heading_level, Some(3));
    }

    #[test]
    fn segments_keep_inline_markup() {
        let parser = AsciidocParser;
        let doc = parser.parse("The *bold* text links to https://example.com[the site].");
        let segments = doc.translatable_segments();
        assert_eq!(segments.len(), 1);
        assert_eq!(
            segments[0].source,
            "The *bold* text links to https://example.com[the site]."
        );
    }

    #[test]
    fn reconstruct_preserves_structure() {
        let parser = AsciidocParser;
        let source = "= Title\nAuthor Name <author@example.com>\n:toc:\n\n[[intro]]\n== Intro\n\nBody text.\n\n[source,python]\n----\nprint(\"hi\")\n----\n\n* Item one.\n* Item two.\n";
        let doc = parser.parse(source);
        let segments = doc.translatable_segments();
        // Author line, attributes, anchors, and code must not be segments.
        assert!(segments.iter().all(|s| !s.source.contains("Author")));
        assert!(segments.iter().all(|s| !s.source.contains("print")));

        let mut translations = TranslationMap::new();
        for seg in &segments {
            let t = match seg.source.as_str() {
                "Title" => "제목",
                "Intro" => "소개",
                "Body text." => "본문.",
                "Item one." => "항목 하나.",
                "Item two." => "항목 둘.",
                other => panic!("unexpected segment: {other}"),
            };
            translations.insert(seg.id.clone(), t.to_string());
        }

        let output = parser.reconstruct(&doc, &translations);
        assert_eq!(
            output,
            "= 제목\nAuthor Name <author@example.com>\n:toc:\n\n[[intro]]\n== 소개\n\n본문.\n\n[source,python]\n----\nprint(\"hi\")\n----\n\n* 항목 하나.\n* 항목 둘.\n"
        );
    }

    #[test]
    fn admonition_label_preserved() {
        let parser = AsciidocParser;
        let source = "NOTE: Remember this fact.\n";
        let doc = parser.parse(source);
        let segments = doc.translatable_segments();
        assert_eq!(segments.len(), 1);
        assert_eq!(segments[0].source, "Remember this fact.");

        let mut translations = TranslationMap::new();
        translations.insert(segments[0].id.clone(), "이 사실을 기억해주세요.".to_string());
        let output = parser.reconstruct(&doc, &translations);
        assert_eq!(output, "NOTE: 이 사실을 기억해주세요.\n");
    }

    #[test]
    fn table_preserved_verbatim() {
        let parser = AsciidocParser;
        let source = "Before.\n\n|===\n|Cell A |Cell B\n|===\n\nAfter.\n";
        let doc = parser.parse(source);
        let segments = doc.translatable_segments();
        assert_eq!(segments.len(), 2);

        let mut translations = TranslationMap::new();
        translations.insert(segments[0].id.clone(), "이전.".to_string());
        translations.insert(segments[1].id.clone(), "이후.".to_string());
        let output = parser.reconstruct(&doc, &translations);
        assert_eq!(output, "이전.\n\n|===\n|Cell A |Cell B\n|===\n\n이후.\n");
    }

    #[test]
    fn multiline_paragraph_joins() {
        let parser = AsciidocParser;
        let doc = parser.parse("This is one\nwrapped sentence.\n");
        let segments = doc.translatable_segments();
        assert_eq!(segments.len(), 1);
        assert_eq!(segments[0].source, "This is one wrapped sentence.");
    }

    #[test]
    fn comment_block_preserved() {
        let parser = AsciidocParser;
        let source = "////\nhidden comment\n////\n\nVisible text.\n";
        let doc = parser.parse(source);
        let segments = doc.translatable_segments();
        assert_eq!(segments.len(), 1);
        assert_eq!(segments[0].source, "Visible text.");
    }

    #[test]
    fn numbered_and_nested_lists() {
        let parser = AsciidocParser;
        let source = "1. First step.\n2. Second step.\n\n** Nested bullet.\n";
        let doc = parser.parse(source);
        let segments = doc.translatable_segments();
        assert_eq!(segments.len(), 3);
        assert!(segments.iter().all(|s| s.block_type == BlockType::ListItem));

        let mut translations = TranslationMap::new();
        translations.insert(segments[0].id.clone(), "첫째.".to_string());
        translations.insert(segments[1].id.clone(), "둘째.".to_string());
        translations.insert(segments[2].id.clone(), "중첩.".to_string());
        let output = parser.reconstruct(&doc, &translations);
        assert_eq!(output, "1. 첫째.\n2. 둘째.\n\n** 중첩.\n");
    }
}
