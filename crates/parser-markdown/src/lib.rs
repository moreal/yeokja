use pulldown_cmark::{Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use std::ops::Range;
use yeokja_core::model::*;
use yeokja_core::parser::{DocumentParser, TranslationMap};
use yeokja_parser_utils::{make_segments, normalize_inline_text, splice_reconstruct};

/// Span-based Markdown parser.
///
/// `parse` records the byte range of each block's inline content in the source.
/// Segments therefore carry the raw inline markdown (links, emphasis, code spans),
/// which lets evaluators verify markup preservation and lets the LLM see it.
/// `reconstruct` splices translations into the original source, leaving everything
/// outside the translated spans (code fences, list markers, blockquote prefixes,
/// front matter, blank lines) byte-for-byte intact.
pub struct MarkdownParser;

#[derive(Debug, Clone, Copy, PartialEq)]
enum Frame {
    Heading(u8),
    Paragraph,
    Item,
    BlockQuote,
    TableCell,
    /// Any container whose text content must not be translated (code blocks,
    /// metadata blocks, raw HTML blocks).
    Opaque,
}

struct ParseState<'a> {
    source: &'a str,
    sections: Vec<Section>,
    section_idx: usize,
    block_idx: usize,
    stack: Vec<Frame>,
    run: Option<Range<usize>>,
}

impl ParseState<'_> {
    fn in_opaque(&self) -> bool {
        self.stack.contains(&Frame::Opaque)
    }

    fn current_block_type(&self) -> (BlockType, Option<u8>) {
        for frame in self.stack.iter().rev() {
            match frame {
                Frame::Heading(level) => return (BlockType::Heading, Some(*level)),
                Frame::TableCell => return (BlockType::Table, None),
                Frame::Item => return (BlockType::ListItem, None),
                Frame::BlockQuote => return (BlockType::BlockQuote, None),
                Frame::Opaque => return (BlockType::CodeBlock, None),
                // A paragraph inherits the label of its enclosing container
                // (blockquote, list item), so keep scanning outward.
                Frame::Paragraph => continue,
            }
        }
        (BlockType::Paragraph, None)
    }

    fn extend_run(&mut self, range: Range<usize>) {
        if self.in_opaque() {
            return;
        }
        match &mut self.run {
            Some(run) => run.end = run.end.max(range.end),
            None => self.run = Some(range),
        }
    }

    /// Close the current inline run and emit it as a translatable block.
    fn flush_run(&mut self) {
        let Some(span) = self.run.take() else { return };
        let raw = &self.source[span.clone()];
        if raw.trim().is_empty() {
            return;
        }
        let (block_type, heading_level) = self.current_block_type();
        if !block_type.is_translatable() {
            return;
        }
        let normalized = normalize_inline_text(raw);
        let segments = make_segments(&normalized, block_type, self.section_idx, self.block_idx);
        self.push_block(Block {
            block_type,
            segments,
            raw_content: raw.to_string(),
            heading_level,
            span: Some(span),
        });
    }

    fn push_block(&mut self, block: Block) {
        self.sections.last_mut().unwrap().blocks.push(block);
        self.block_idx += 1;
    }

    fn push_opaque_block(&mut self, block_type: BlockType, range: Range<usize>) {
        let raw = self.source[range].to_string();
        self.push_block(Block {
            block_type,
            segments: Vec::new(),
            raw_content: raw,
            heading_level: None,
            span: None,
        });
    }

    fn maybe_start_section(&mut self, level: HeadingLevel) {
        let splits_section = matches!(level, HeadingLevel::H1 | HeadingLevel::H2);
        if splits_section && !self.sections.last().unwrap().blocks.is_empty() {
            self.sections.push(Section { blocks: Vec::new() });
            self.section_idx += 1;
            self.block_idx = 0;
        }
    }
}

fn heading_level_num(level: HeadingLevel) -> u8 {
    match level {
        HeadingLevel::H1 => 1,
        HeadingLevel::H2 => 2,
        HeadingLevel::H3 => 3,
        HeadingLevel::H4 => 4,
        HeadingLevel::H5 => 5,
        HeadingLevel::H6 => 6,
    }
}

impl DocumentParser for MarkdownParser {
    fn parse(&self, source: &str) -> Document {
        let mut options = Options::empty();
        options.insert(Options::ENABLE_TABLES);
        options.insert(Options::ENABLE_TASKLISTS);
        options.insert(Options::ENABLE_FOOTNOTES);
        options.insert(Options::ENABLE_YAML_STYLE_METADATA_BLOCKS);

        let mut state = ParseState {
            source,
            sections: vec![Section { blocks: Vec::new() }],
            section_idx: 0,
            block_idx: 0,
            stack: Vec::new(),
            run: None,
        };

        for (event, range) in Parser::new_ext(source, options).into_offset_iter() {
            match event {
                Event::Start(tag) => match tag {
                    Tag::Heading { level, .. } => {
                        state.flush_run();
                        state.maybe_start_section(level);
                        state.stack.push(Frame::Heading(heading_level_num(level)));
                    }
                    Tag::Paragraph => {
                        state.flush_run();
                        state.stack.push(Frame::Paragraph);
                    }
                    Tag::Item => {
                        state.flush_run();
                        state.stack.push(Frame::Item);
                    }
                    Tag::BlockQuote(_) => {
                        state.flush_run();
                        state.stack.push(Frame::BlockQuote);
                    }
                    Tag::TableCell => {
                        state.flush_run();
                        state.stack.push(Frame::TableCell);
                    }
                    Tag::CodeBlock(_) | Tag::MetadataBlock(_) | Tag::HtmlBlock => {
                        state.flush_run();
                        state.stack.push(Frame::Opaque);
                    }
                    Tag::List(_) | Tag::Table(_) | Tag::TableHead | Tag::TableRow
                    | Tag::FootnoteDefinition(_) => {
                        state.flush_run();
                    }
                    // Inline containers: their full range (including markers) is
                    // part of the surrounding run.
                    Tag::Emphasis | Tag::Strong | Tag::Strikethrough
                    | Tag::Link { .. } | Tag::Image { .. } => {
                        state.extend_run(range);
                    }
                    _ => {}
                },
                Event::End(tag_end) => match tag_end {
                    TagEnd::Heading(_)
                    | TagEnd::Paragraph
                    | TagEnd::Item
                    | TagEnd::BlockQuote(_)
                    | TagEnd::TableCell => {
                        state.flush_run();
                        state.stack.pop();
                    }
                    TagEnd::CodeBlock => {
                        state.stack.pop();
                        state.push_opaque_block(BlockType::CodeBlock, range);
                    }
                    TagEnd::MetadataBlock(_) => {
                        state.stack.pop();
                        state.push_opaque_block(BlockType::HtmlBlock, range);
                    }
                    TagEnd::HtmlBlock => {
                        state.stack.pop();
                        state.push_opaque_block(BlockType::HtmlBlock, range);
                    }
                    _ => {}
                },
                Event::Text(_)
                | Event::Code(_)
                | Event::InlineMath(_)
                | Event::DisplayMath(_)
                | Event::InlineHtml(_)
                | Event::FootnoteReference(_) => {
                    state.extend_run(range);
                }
                // Breaks only extend an already-started run; they never start one.
                Event::SoftBreak | Event::HardBreak => {
                    if state.run.is_some() {
                        state.extend_run(range);
                    }
                }
                Event::Rule => {
                    state.flush_run();
                    state.push_opaque_block(BlockType::ThematicBreak, range);
                }
                Event::Html(_) => {
                    // Block-level HTML outside an HtmlBlock tag (rare); keep verbatim.
                    state.flush_run();
                }
                Event::TaskListMarker(_) => {}
            }
        }
        state.flush_run();

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
        let parser = MarkdownParser;
        let doc = parser.parse("Hello world. Goodbye world.");
        assert_eq!(doc.sections.len(), 1);
        let segments = doc.translatable_segments();
        assert_eq!(segments.len(), 2);
        assert_eq!(segments[0].source, "Hello world.");
        assert_eq!(segments[1].source, "Goodbye world.");
    }

    #[test]
    fn parse_heading_starts_new_section() {
        let parser = MarkdownParser;
        let doc = parser.parse("# Chapter 1\n\nSome text.\n\n## Chapter 2\n\nMore text.");
        assert!(doc.sections.len() >= 2);
    }

    #[test]
    fn code_blocks_not_translatable() {
        let parser = MarkdownParser;
        let doc = parser.parse("Some text.\n\n```\nfn main() {}\n```\n\nMore text.");
        let translatable = doc.translatable_segments();
        for seg in &translatable {
            assert_ne!(seg.block_type, BlockType::CodeBlock);
        }
        assert_eq!(translatable.len(), 2);
    }

    #[test]
    fn segments_keep_inline_markup() {
        let parser = MarkdownParser;
        let doc = parser.parse(
            "Visit [the docs](https://example.com/docs) for **more details** about `git init`.",
        );
        let segments = doc.translatable_segments();
        assert_eq!(segments.len(), 1);
        assert_eq!(
            segments[0].source,
            "Visit [the docs](https://example.com/docs) for **more details** about `git init`."
        );
    }

    #[test]
    fn reconstruct_with_translations() {
        let parser = MarkdownParser;
        let doc = parser.parse("Hello world.");
        let segments = doc.translatable_segments();
        let mut translations = TranslationMap::new();
        translations.insert(segments[0].id.clone(), "안녕하세요.".to_string());

        let output = parser.reconstruct(&doc, &translations);
        assert_eq!(output, "안녕하세요.");
    }

    #[test]
    fn reconstruct_falls_back_to_original() {
        let parser = MarkdownParser;
        let doc = parser.parse("Hello world.");
        let translations = TranslationMap::new(); // empty

        let output = parser.reconstruct(&doc, &translations);
        assert!(output.contains("Hello world."));
    }

    #[test]
    fn reconstruct_preserves_code_fence_language() {
        let parser = MarkdownParser;
        let source = "Intro text.\n\n```sh\ngit init\n```\n\nOutro text.\n";
        let doc = parser.parse(source);
        let segments = doc.translatable_segments();
        let mut translations = TranslationMap::new();
        translations.insert(segments[0].id.clone(), "소개.".to_string());
        translations.insert(segments[1].id.clone(), "마무리.".to_string());

        let output = parser.reconstruct(&doc, &translations);
        assert_eq!(output, "소개.\n\n```sh\ngit init\n```\n\n마무리.\n");
    }

    #[test]
    fn reconstruct_preserves_list_markers() {
        let parser = MarkdownParser;
        let source = "- First item.\n- Second item.\n";
        let doc = parser.parse(source);
        let segments = doc.translatable_segments();
        assert_eq!(segments.len(), 2);
        assert_eq!(segments[0].block_type, BlockType::ListItem);

        let mut translations = TranslationMap::new();
        translations.insert(segments[0].id.clone(), "첫째.".to_string());
        translations.insert(segments[1].id.clone(), "둘째.".to_string());

        let output = parser.reconstruct(&doc, &translations);
        assert_eq!(output, "- 첫째.\n- 둘째.\n");
    }

    #[test]
    fn reconstruct_preserves_nested_list_structure() {
        let parser = MarkdownParser;
        let source = "- Parent item.\n  - Child item.\n";
        let doc = parser.parse(source);
        let segments = doc.translatable_segments();
        assert_eq!(segments.len(), 2);

        let mut translations = TranslationMap::new();
        translations.insert(segments[0].id.clone(), "부모.".to_string());
        translations.insert(segments[1].id.clone(), "자식.".to_string());

        let output = parser.reconstruct(&doc, &translations);
        assert_eq!(output, "- 부모.\n  - 자식.\n");
    }

    #[test]
    fn reconstruct_preserves_heading_markers() {
        let parser = MarkdownParser;
        let source = "# Title\n\nBody text.\n";
        let doc = parser.parse(source);
        let segments = doc.translatable_segments();
        let mut translations = TranslationMap::new();
        translations.insert(segments[0].id.clone(), "제목".to_string());
        translations.insert(segments[1].id.clone(), "본문.".to_string());

        let output = parser.reconstruct(&doc, &translations);
        assert_eq!(output, "# 제목\n\n본문.\n");
    }

    #[test]
    fn reconstruct_preserves_blockquote_prefix() {
        let parser = MarkdownParser;
        let source = "> Quoted text.\n";
        let doc = parser.parse(source);
        let segments = doc.translatable_segments();
        assert_eq!(segments.len(), 1);
        assert_eq!(segments[0].block_type, BlockType::BlockQuote);

        let mut translations = TranslationMap::new();
        translations.insert(segments[0].id.clone(), "인용문.".to_string());

        let output = parser.reconstruct(&doc, &translations);
        assert_eq!(output, "> 인용문.\n");
    }

    #[test]
    fn reconstruct_preserves_table_structure() {
        let parser = MarkdownParser;
        let source = "| Name | Desc |\n|------|------|\n| Repo | The repository. |\n";
        let doc = parser.parse(source);
        let segments = doc.translatable_segments();
        assert!(segments.iter().any(|s| s.source == "The repository."));

        let mut translations = TranslationMap::new();
        for seg in &segments {
            if seg.source == "The repository." {
                translations.insert(seg.id.clone(), "저장소.".to_string());
            }
        }

        let output = parser.reconstruct(&doc, &translations);
        assert!(output.contains("| Repo | 저장소. |"));
        assert!(output.contains("|------|------|"));
    }

    #[test]
    fn front_matter_not_translated() {
        let parser = MarkdownParser;
        let source = "---\ntitle: Hello\n---\n\nBody text.\n";
        let doc = parser.parse(source);
        let segments = doc.translatable_segments();
        assert_eq!(segments.len(), 1);
        assert_eq!(segments[0].source, "Body text.");

        let mut translations = TranslationMap::new();
        translations.insert(segments[0].id.clone(), "본문.".to_string());
        let output = parser.reconstruct(&doc, &translations);
        assert_eq!(output, "---\ntitle: Hello\n---\n\n본문.\n");
    }

    #[test]
    fn multiline_paragraph_normalized_to_single_segment_text() {
        let parser = MarkdownParser;
        let source = "This is one\nwrapped sentence.\n";
        let doc = parser.parse(source);
        let segments = doc.translatable_segments();
        assert_eq!(segments.len(), 1);
        assert_eq!(segments[0].source, "This is one wrapped sentence.");
    }

    #[test]
    fn task_list_marker_preserved() {
        let parser = MarkdownParser;
        let source = "- [x] Done task.\n- [ ] Open task.\n";
        let doc = parser.parse(source);
        let segments = doc.translatable_segments();
        assert_eq!(segments.len(), 2);
        assert_eq!(segments[0].source, "Done task.");

        let mut translations = TranslationMap::new();
        translations.insert(segments[0].id.clone(), "완료됨.".to_string());
        translations.insert(segments[1].id.clone(), "미완료.".to_string());
        let output = parser.reconstruct(&doc, &translations);
        assert_eq!(output, "- [x] 완료됨.\n- [ ] 미완료.\n");
    }

    #[test]
    fn html_block_preserved_verbatim() {
        let parser = MarkdownParser;
        let source = "Text before.\n\n<div class=\"note\">\nraw html\n</div>\n\nText after.\n";
        let doc = parser.parse(source);
        let segments = doc.translatable_segments();
        assert_eq!(segments.len(), 2);

        let mut translations = TranslationMap::new();
        translations.insert(segments[0].id.clone(), "이전.".to_string());
        translations.insert(segments[1].id.clone(), "이후.".to_string());
        let output = parser.reconstruct(&doc, &translations);
        assert_eq!(output, "이전.\n\n<div class=\"note\">\nraw html\n</div>\n\n이후.\n");
    }
}
