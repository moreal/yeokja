mod sentence;

use pulldown_cmark::{Event, HeadingLevel, Parser, Tag, TagEnd};
use yeokja_core::hash::content_hash;
use yeokja_core::model::*;
use yeokja_core::parser::{DocumentParser, TranslationMap};

pub struct MarkdownParser;

impl DocumentParser for MarkdownParser {
    fn parse(&self, source: &str) -> Document {
        let parser = Parser::new(source);
        let mut sections: Vec<Section> = vec![Section { blocks: Vec::new() }];
        let mut current_block_type: Option<BlockType> = None;
        let mut current_text = String::new();
        let mut current_heading_level: Option<u8> = None;
        let mut in_code_block = false;
        let mut section_idx: usize = 0;
        let mut block_idx: usize = 0;

        for event in parser {
            match event {
                Event::Start(Tag::Heading { level, .. }) => {
                    flush_block(
                        &mut sections,
                        &mut current_block_type,
                        &mut current_text,
                        &mut current_heading_level,
                        &mut section_idx,
                        &mut block_idx,
                    );
                    // Start new section on h1/h2
                    if matches!(level, HeadingLevel::H1 | HeadingLevel::H2) && !sections.last().unwrap().blocks.is_empty() {
                        sections.push(Section { blocks: Vec::new() });
                        section_idx += 1;
                        block_idx = 0;
                    }
                    current_block_type = Some(BlockType::Heading);
                    current_heading_level = Some(match level {
                        HeadingLevel::H1 => 1,
                        HeadingLevel::H2 => 2,
                        HeadingLevel::H3 => 3,
                        HeadingLevel::H4 => 4,
                        HeadingLevel::H5 => 5,
                        HeadingLevel::H6 => 6,
                    });
                }
                Event::End(TagEnd::Heading(_)) => {
                    flush_block(
                        &mut sections,
                        &mut current_block_type,
                        &mut current_text,
                        &mut current_heading_level,
                        &mut section_idx,
                        &mut block_idx,
                    );
                }
                Event::Start(Tag::Paragraph) => {
                    if !in_code_block {
                        current_block_type = Some(BlockType::Paragraph);
                    }
                }
                Event::End(TagEnd::Paragraph) => {
                    if !in_code_block {
                        flush_block(
                            &mut sections,
                            &mut current_block_type,
                            &mut current_text,
                            &mut current_heading_level,
                            &mut section_idx,
                            &mut block_idx,
                        );
                    }
                }
                Event::Start(Tag::Item) => {
                    current_block_type = Some(BlockType::ListItem);
                }
                Event::End(TagEnd::Item) => {
                    flush_block(
                        &mut sections,
                        &mut current_block_type,
                        &mut current_text,
                        &mut current_heading_level,
                        &mut section_idx,
                        &mut block_idx,
                    );
                }
                Event::Start(Tag::CodeBlock(_)) => {
                    in_code_block = true;
                    current_block_type = Some(BlockType::CodeBlock);
                }
                Event::End(TagEnd::CodeBlock) => {
                    in_code_block = false;
                    flush_block(
                        &mut sections,
                        &mut current_block_type,
                        &mut current_text,
                        &mut current_heading_level,
                        &mut section_idx,
                        &mut block_idx,
                    );
                }
                Event::Start(Tag::BlockQuote(_)) => {
                    current_block_type = Some(BlockType::BlockQuote);
                }
                Event::End(TagEnd::BlockQuote(_)) => {
                    flush_block(
                        &mut sections,
                        &mut current_block_type,
                        &mut current_text,
                        &mut current_heading_level,
                        &mut section_idx,
                        &mut block_idx,
                    );
                }
                Event::Text(text) | Event::Code(text) => {
                    current_text.push_str(&text);
                }
                Event::SoftBreak | Event::HardBreak => {
                    current_text.push(' ');
                }
                Event::Rule => {
                    flush_block(
                        &mut sections,
                        &mut current_block_type,
                        &mut current_text,
                        &mut current_heading_level,
                        &mut section_idx,
                        &mut block_idx,
                    );
                    // ThematicBreak is not translatable, add as empty block
                    let section = sections.last_mut().unwrap();
                    section.blocks.push(Block {
                        block_type: BlockType::ThematicBreak,
                        segments: Vec::new(),
                        raw_content: "---".to_string(),
                        heading_level: None,
                    });
                    block_idx += 1;
                }
                _ => {}
            }
        }

        // Flush any remaining content
        flush_block(
            &mut sections,
            &mut current_block_type,
            &mut current_text,
            &mut current_heading_level,
            &mut section_idx,
            &mut block_idx,
        );

        // Remove empty sections
        sections.retain(|s| !s.blocks.is_empty());

        Document { sections }
    }

    fn reconstruct(&self, document: &Document, translations: &TranslationMap) -> String {
        // For the initial implementation, reconstruct by walking segments
        // and substituting translations. This is a simplified version
        // that works for basic cases; a full implementation would preserve
        // the original markdown structure more precisely.
        let mut output = String::new();

        for section in &document.sections {
            for block in &section.blocks {
                match block.block_type {
                    BlockType::CodeBlock => {
                        output.push_str("```\n");
                        output.push_str(&block.raw_content);
                        output.push_str("\n```\n\n");
                    }
                    BlockType::ThematicBreak => {
                        output.push_str("---\n\n");
                    }
                    BlockType::Heading => {
                        let level = block.heading_level.unwrap_or(1);
                        let prefix = "#".repeat(level as usize);
                        output.push_str(&format!("{prefix} "));
                        for seg in &block.segments {
                            let text = translations
                                .get(&seg.id)
                                .unwrap_or(&seg.source);
                            output.push_str(text);
                        }
                        output.push_str("\n\n");
                    }
                    _ => {
                        for (i, seg) in block.segments.iter().enumerate() {
                            let text = translations
                                .get(&seg.id)
                                .unwrap_or(&seg.source);
                            if i > 0 {
                                output.push(' ');
                            }
                            output.push_str(text);
                        }
                        output.push_str("\n\n");
                    }
                }
            }
        }

        output.trim_end().to_string()
    }
}

fn flush_block(
    sections: &mut Vec<Section>,
    current_block_type: &mut Option<BlockType>,
    current_text: &mut String,
    current_heading_level: &mut Option<u8>,
    section_idx: &mut usize,
    block_idx: &mut usize,
) {
    if let Some(block_type) = current_block_type.take() {
        let text = std::mem::take(current_text);
        let heading_level = current_heading_level.take();
        if text.trim().is_empty() {
            return;
        }

        let raw_content = text.clone();
        let segments = if block_type.is_translatable() {
            sentence::split_sentences(&text)
                .into_iter()
                .enumerate()
                .map(|(seg_i, sent)| {
                    let hash = content_hash(&sent);
                    Segment {
                        id: SegmentId::new(*section_idx, *block_idx, seg_i),
                        source: sent,
                        source_hash: hash,
                        block_type,
                    }
                })
                .collect()
        } else {
            Vec::new()
        };

        let section = sections.last_mut().unwrap();
        section.blocks.push(Block {
            block_type,
            segments,
            raw_content,
            heading_level,
        });
        *block_idx += 1;
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
    }

    #[test]
    fn reconstruct_with_translations() {
        let parser = MarkdownParser;
        let doc = parser.parse("Hello world.");
        let segments = doc.translatable_segments();
        let mut translations = TranslationMap::new();
        translations.insert(segments[0].id.clone(), "안녕하세요.".to_string());

        let output = parser.reconstruct(&doc, &translations);
        assert!(output.contains("안녕하세요."));
    }

    #[test]
    fn reconstruct_falls_back_to_original() {
        let parser = MarkdownParser;
        let doc = parser.parse("Hello world.");
        let translations = TranslationMap::new(); // empty

        let output = parser.reconstruct(&doc, &translations);
        assert!(output.contains("Hello world."));
    }
}
