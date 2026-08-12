use asciidork_ast::{BlockContent, BlockContext, CellContent, DocContent};
use asciidork_parser::prelude::SourceFile;
use asciidork_parser::Parser;
use bumpalo::Bump;
use yeokja_core::model::{Block, BlockType, Document, Section};
use yeokja_core::parser::{DocumentParser, TranslationMap};
use yeokja_parser_utils::{join_segments_with_translations, make_segments};

pub struct AsciidocParser;

impl DocumentParser for AsciidocParser {
    fn parse(&self, source: &str) -> Document {
        let preprocessed = preprocess(source);
        let bump = Bump::new();
        let parser = Parser::from_str(&preprocessed, SourceFile::Tmp, &bump);

        let result = match parser.parse() {
            Ok(r) => r,
            Err(diagnostics) => {
                tracing::warn!(
                    errors = diagnostics.len(),
                    "AsciiDoc parse failed, falling back to line-based parser"
                );
                for d in diagnostics.iter().take(3) {
                    tracing::debug!(diagnostic = ?d, "Parse diagnostic");
                }
                return parse_fallback(source);
            }
        };

        let mut sections: Vec<Section> = vec![Section { blocks: Vec::new() }];
        let mut section_idx: usize = 0;
        let mut block_idx: usize = 0;

        // Handle document title (= Title) which is stored in the header
        if let Some(title) = result.document.title() {
            let title_text = title.main.plain_text().join("");
            if !title_text.trim().is_empty() {
                let segments = make_segments(&title_text, BlockType::Heading, section_idx, block_idx);
                sections.last_mut().unwrap().blocks.push(Block {
                    block_type: BlockType::Heading,
                    segments,
                    raw_content: title_text,
                    heading_level: Some(1),
                    span: None,
                });
                block_idx += 1;
            }
        }

        match &result.document.content {
            DocContent::Sections(sectioned) => {
                // Handle preamble blocks
                if let Some(preamble) = &sectioned.preamble {
                    for ast_block in preamble.iter() {
                        process_block(ast_block, &mut sections, &mut section_idx, &mut block_idx, None);
                    }
                }

                // Handle top-level sections
                for ast_section in sectioned.sections.iter() {
                    // Start a new section
                    sections.push(Section { blocks: Vec::new() });
                    section_idx = sections.len() - 1;
                    block_idx = 0;

                    // Add heading
                    let heading_text = ast_section.heading.plain_text().join("");
                    if !heading_text.trim().is_empty() {
                        let level = ast_section.level + 1; // asciidork uses 0-based levels
                        let segments = make_segments(&heading_text, BlockType::Heading, section_idx, block_idx);
                        sections.last_mut().unwrap().blocks.push(Block {
                            block_type: BlockType::Heading,
                            segments,
                            raw_content: heading_text,
                            heading_level: Some(level),
                            span: None,
                        });
                        block_idx += 1;
                    }

                    // Process blocks in section (may create new sections for nested headings)
                    for ast_block in ast_section.blocks.iter() {
                        process_block(ast_block, &mut sections, &mut section_idx, &mut block_idx, None);
                    }
                }
            }
            DocContent::Blocks(blocks) => {
                for ast_block in blocks.iter() {
                    process_block(ast_block, &mut sections, &mut section_idx, &mut block_idx, None);
                }
            }
            _ => {}
        }

        sections.retain(|s| !s.blocks.is_empty());
        Document {
            sections,
            source: source.to_string(),
        }
    }

    fn reconstruct(&self, document: &Document, translations: &TranslationMap) -> String {
        let mut output = String::new();
        let mut prev_was_list_item = false;

        for section in &document.sections {
            for block in &section.blocks {
                // A list only ends at a blank line in AsciiDoc; separate any
                // following non-list block so it does not attach to the item.
                if prev_was_list_item && block.block_type != BlockType::ListItem {
                    output.push('\n');
                }
                prev_was_list_item = block.block_type == BlockType::ListItem;

                match block.block_type {
                    BlockType::Heading => {
                        let level = block.heading_level.unwrap_or(1);
                        let prefix = "=".repeat(level as usize);
                        output.push_str(&prefix);
                        output.push(' ');
                        output.push_str(&join_segments_with_translations(&block.segments, translations));
                        output.push_str("\n\n");
                    }
                    BlockType::CodeBlock => {
                        output.push_str("----\n");
                        output.push_str(&block.raw_content);
                        output.push_str("\n----\n\n");
                    }
                    BlockType::BlockQuote => {
                        output.push_str("____\n");
                        output.push_str(&join_segments_with_translations(&block.segments, translations));
                        output.push_str("\n____\n\n");
                    }
                    BlockType::ThematicBreak => {
                        output.push_str("'''\n\n");
                    }
                    BlockType::ListItem => {
                        output.push_str("* ");
                        output.push_str(&join_segments_with_translations(&block.segments, translations));
                        output.push('\n');
                    }
                    _ => {
                        output.push_str(&join_segments_with_translations(&block.segments, translations));
                        output.push_str("\n\n");
                    }
                }
            }
        }

        output.trim_end().to_string()
    }
}

fn process_block(
    ast_block: &asciidork_ast::Block,
    sections: &mut Vec<Section>,
    section_idx: &mut usize,
    block_idx: &mut usize,
    block_type_override: Option<BlockType>,
) {
    match &ast_block.content {
        BlockContent::Section(sec) => {
            // Start a new section for this heading
            sections.push(Section { blocks: Vec::new() });
            *section_idx = sections.len() - 1;
            *block_idx = 0;

            let heading_text = sec.heading.plain_text().join("");
            if !heading_text.trim().is_empty() {
                let segments = make_segments(&heading_text, BlockType::Heading, *section_idx, *block_idx);
                sections.last_mut().unwrap().blocks.push(Block {
                    block_type: BlockType::Heading,
                    segments,
                    raw_content: heading_text,
                    heading_level: Some(sec.level + 1), // asciidork uses 0-based levels
                    span: None,
                });
                *block_idx += 1;
            }

            for nested in sec.blocks.iter() {
                process_block(nested, sections, section_idx, block_idx, None);
            }
        }
        BlockContent::Simple(inline_nodes) => {
            let text = inline_nodes.plain_text().join("");
            if text.trim().is_empty() {
                return;
            }
            let block_type = block_type_override.unwrap_or(match ast_block.context {
                BlockContext::Listing | BlockContext::Literal => BlockType::CodeBlock,
                BlockContext::BlockQuote | BlockContext::Verse => BlockType::BlockQuote,
                BlockContext::Passthrough => BlockType::HtmlBlock,
                _ => BlockType::Paragraph,
            });

            if block_type == BlockType::CodeBlock || block_type == BlockType::HtmlBlock {
                sections.last_mut().unwrap().blocks.push(Block {
                    block_type,
                    segments: Vec::new(),
                    raw_content: text,
                    heading_level: None,
                    span: None,
                });
            } else {
                let segments = make_segments(&text, block_type, *section_idx, *block_idx);
                sections.last_mut().unwrap().blocks.push(Block {
                    block_type,
                    segments,
                    raw_content: text,
                    heading_level: None,
                    span: None,
                });
            }
            *block_idx += 1;
        }
        BlockContent::Compound(blocks) => {
            // Determine if this compound block has a specific context (e.g., BlockQuote)
            let override_type = match ast_block.context {
                BlockContext::BlockQuote | BlockContext::Verse => Some(BlockType::BlockQuote),
                _ => block_type_override,
            };
            for nested in blocks.iter() {
                process_block(nested, sections, section_idx, block_idx, override_type);
            }
        }
        BlockContent::List { items, .. } => {
            for item in items.iter() {
                let text = item.principle.plain_text().join("");
                if !text.trim().is_empty() {
                    let segments = make_segments(&text, BlockType::ListItem, *section_idx, *block_idx);
                    sections.last_mut().unwrap().blocks.push(Block {
                        block_type: BlockType::ListItem,
                        segments,
                        raw_content: text,
                        heading_level: None,
                        span: None,
                    });
                    *block_idx += 1;
                }
                for nested in item.blocks.iter() {
                    process_block(nested, sections, section_idx, block_idx, None);
                }
            }
        }
        BlockContent::Empty(_) if ast_block.context == BlockContext::ThematicBreak => {
            sections.last_mut().unwrap().blocks.push(Block {
                block_type: BlockType::ThematicBreak,
                segments: Vec::new(),
                raw_content: "'''".to_string(),
                heading_level: None,
                span: None,
            });
            *block_idx += 1;
        }
        BlockContent::Table(table) => {
            for row in &table.rows {
                for cell in row.cells.iter() {
                    if let CellContent::Default(paras) = &cell.content {
                        let text: String = paras.iter().map(|p| p.plain_text().join("")).collect::<Vec<_>>().join(" ");
                        if !text.trim().is_empty() {
                            let segments = make_segments(&text, BlockType::Table, *section_idx, *block_idx);
                            sections.last_mut().unwrap().blocks.push(Block {
                                block_type: BlockType::Table,
                                segments,
                                raw_content: text,
                                heading_level: None,
                                span: None,
                            });
                            *block_idx += 1;
                        }
                    }
                }
            }
        }
        BlockContent::QuotedParagraph { quote, .. } => {
            let text = quote.plain_text().join("");
            if !text.trim().is_empty() {
                let segments = make_segments(&text, BlockType::BlockQuote, *section_idx, *block_idx);
                sections.last_mut().unwrap().blocks.push(Block {
                    block_type: BlockType::BlockQuote,
                    segments,
                    raw_content: text,
                    heading_level: None,
                    span: None,
                });
                *block_idx += 1;
            }
        }
        _ => {}
    }
}

/// Preprocess AsciiDoc source to fix common issues that cause strict parsers to fail:
/// - Remove standalone block anchors `[[...]]` (reattach as comments)
/// - Replace cross-references with plain text
/// - Fix section level skipping by inserting placeholder headings
fn preprocess(source: &str) -> String {
    use std::fmt::Write;

    let lines: Vec<&str> = source.lines().collect();
    let mut result = String::with_capacity(source.len());
    let mut last_heading_level: Option<u8> = None;

    for (i, line) in lines.iter().enumerate() {
        let trimmed = line.trim();

        // Remove standalone block anchors [[...]] that aren't followed by a block element
        if trimmed.starts_with("[[") && trimmed.ends_with("]]") && !trimmed.contains(' ') {
            // Check if next non-empty line is a heading or block delimiter
            let next_content = lines[i + 1..].iter().find(|l| !l.trim().is_empty());
            if let Some(next) = next_content {
                let next = next.trim();
                if next.starts_with('=') || next.starts_with("----") || next.starts_with("....") {
                    // Keep it - it's attached to a block
                    writeln!(result, "{line}").unwrap();
                    continue;
                }
            }
            // Remove unattached anchor
            continue;
        }

        // Fix section level skipping: if we see === without a preceding ==, add dummy ==
        if trimmed.starts_with('=') && !trimmed.starts_with("==== ") {
            let level = trimmed.chars().take_while(|c| *c == '=').count() as u8;
            if level >= 2 {
                if let Some(last) = last_heading_level {
                    // Insert missing intermediate levels
                    for missing in (last + 1)..level {
                        let prefix = "=".repeat(missing as usize);
                        writeln!(result, "{prefix} _").unwrap();
                        writeln!(result).unwrap();
                    }
                }
                last_heading_level = Some(level);
            }
        }

        // Replace cross-references <<target>> and xref:target[] with plain text
        let mut processed_line = line.to_string();
        // Handle <<Target Text>> style
        while let Some(start) = processed_line.find("<<") {
            if let Some(end) = processed_line[start..].find(">>") {
                let inner = &processed_line[start + 2..start + end];
                // Use the display text if provided (<<target,display>>), otherwise use target
                let display = if let Some(comma) = inner.find(',') {
                    inner[comma + 1..].trim()
                } else {
                    inner
                };
                processed_line = format!("{}{}{}", &processed_line[..start], display, &processed_line[start + end + 2..]);
            } else {
                break;
            }
        }
        // Handle xref:target[display] style
        while let Some(start) = processed_line.find("xref:") {
            if let Some(bracket_start) = processed_line[start..].find('[') {
                if let Some(bracket_end) = processed_line[start + bracket_start..].find(']') {
                    let display = &processed_line[start + bracket_start + 1..start + bracket_start + bracket_end];
                    let display = if display.is_empty() {
                        &processed_line[start + 5..start + bracket_start]
                    } else {
                        display
                    };
                    processed_line = format!("{}{}{}", &processed_line[..start], display, &processed_line[start + bracket_start + bracket_end + 1..]);
                } else {
                    break;
                }
            } else {
                break;
            }
        }

        writeln!(result, "{processed_line}").unwrap();
    }

    result
}

/// Simple line-based fallback parser for AsciiDoc files that the strict parser can't handle.
fn parse_fallback(source: &str) -> Document {
    let mut sections: Vec<Section> = vec![Section { blocks: Vec::new() }];
    let mut section_idx = 0usize;
    let mut block_idx = 0usize;
    let mut in_code_block = false;
    let mut current_paragraph = String::new();

    let flush_paragraph = |para: &mut String, sections: &mut Vec<Section>, section_idx: usize, block_idx: &mut usize| {
        let text = para.trim().to_string();
        if !text.is_empty() {
            let segments = make_segments(&text, BlockType::Paragraph, section_idx, *block_idx);
            sections.last_mut().unwrap().blocks.push(Block {
                block_type: BlockType::Paragraph,
                segments,
                raw_content: text,
                heading_level: None,
                span: None,
            });
            *block_idx += 1;
        }
        para.clear();
    };

    for line in source.lines() {
        let trimmed = line.trim();

        // Toggle code blocks
        if trimmed.starts_with("----") || trimmed.starts_with("....") {
            if in_code_block {
                in_code_block = false;
            } else {
                flush_paragraph(&mut current_paragraph, &mut sections, section_idx, &mut block_idx);
                in_code_block = true;
            }
            continue;
        }

        if in_code_block {
            continue;
        }

        // Skip block metadata like [[anchor]], [source,erlang], etc.
        if trimmed.starts_with("[[") || trimmed.starts_with("[") && trimmed.ends_with("]") {
            continue;
        }

        // Skip include directives
        if trimmed.starts_with("include::") {
            continue;
        }

        // Skip image macros
        if trimmed.starts_with("image::") || trimmed.starts_with("image:") {
            continue;
        }

        // Headings
        if trimmed.starts_with('=') && trimmed.contains(' ') {
            let level = trimmed.chars().take_while(|c| *c == '=').count() as u8;
            if (1..=6).contains(&level) {
                flush_paragraph(&mut current_paragraph, &mut sections, section_idx, &mut block_idx);

                sections.push(Section { blocks: Vec::new() });
                section_idx = sections.len() - 1;
                block_idx = 0;

                let heading_text = trimmed[level as usize..].trim().to_string();
                if !heading_text.is_empty() {
                    let segments = make_segments(&heading_text, BlockType::Heading, section_idx, block_idx);
                    sections.last_mut().unwrap().blocks.push(Block {
                        block_type: BlockType::Heading,
                        segments,
                        raw_content: heading_text,
                        heading_level: Some(level),
                        span: None,
                    });
                    block_idx += 1;
                }
                continue;
            }
        }

        // List items
        if trimmed.starts_with("* ") || trimmed.starts_with(". ") {
            flush_paragraph(&mut current_paragraph, &mut sections, section_idx, &mut block_idx);
            let text = trimmed[2..].trim().to_string();
            if !text.is_empty() {
                let segments = make_segments(&text, BlockType::ListItem, section_idx, block_idx);
                sections.last_mut().unwrap().blocks.push(Block {
                    block_type: BlockType::ListItem,
                    segments,
                    raw_content: text,
                    heading_level: None,
                    span: None,
                });
                block_idx += 1;
            }
            continue;
        }

        // Empty line → flush paragraph
        if trimmed.is_empty() {
            flush_paragraph(&mut current_paragraph, &mut sections, section_idx, &mut block_idx);
            continue;
        }

        // Accumulate paragraph text
        if !current_paragraph.is_empty() {
            current_paragraph.push(' ');
        }
        current_paragraph.push_str(trimmed);
    }

    flush_paragraph(&mut current_paragraph, &mut sections, section_idx, &mut block_idx);
    sections.retain(|s| !s.blocks.is_empty());
    Document {
        sections,
        source: source.to_string(),
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
        assert!(translatable.len() >= 2);
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
        assert!(output.contains("안녕하세요."));
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
}
