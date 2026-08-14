//! Span-based reStructuredText parser.
//!
//! reStructuredText block structure hangs on indentation and on two-line
//! constructs (a title and its underline, a paragraph and the literal block its
//! trailing `::` announces), so this parser works over a pre-split vector of
//! lines with lookahead rather than a streaming scan. Each translatable block
//! records the byte range (`Block::span`) of its text — excluding list markers,
//! directive lines, field names, and the `::` that introduces a literal block —
//! and segments keep the raw inline markup (`**bold**`, ``` ``literal`` ```,
//! :ref:`target`, `link <url>`_). Reconstruction splices translations into the
//! original source, preserving everything outside the translated spans
//! byte-for-byte; title underlines and overlines are redrawn to the
//! translation's display width, since docutils requires them to cover it.

use std::ops::Range;
use yeokja_core::model::*;
use yeokja_core::parser::{DocumentParser, Markup, TranslationMap};
use yeokja_parser_utils::{
    apply_splices, collect_splices, join_segments_with_translations, make_segments,
    normalize_inline_text,
};

pub struct RstParser;

/// Directives whose body is data, not prose: code, markup passed through raw,
/// document structure, or references resolved elsewhere. Everything not listed
/// keeps its body in the document's language — admonitions, `function::` and
/// friends describe things in prose — so an unknown directive stays
/// translatable and only its marker line is held back.
const OPAQUE_DIRECTIVES: [&str; 18] = [
    "code",
    "code-block",
    "sourcecode",
    "literalinclude",
    "parsed-literal",
    "doctest",
    "raw",
    "math",
    "toctree",
    "contents",
    "include",
    "highlight",
    "image",
    "figure",
    "csv-table",
    "list-table",
    "productionlist",
    "tabularcolumns",
];

/// The punctuation characters docutils accepts as a title adornment.
const ADORNMENT_CHARS: &str = "!\"#$%&'()*+,-./:;<=>?@[\\]^_`{|}~";

struct Line<'a> {
    start: usize,
    /// Line text without its newline.
    content: &'a str,
}

impl Line<'_> {
    fn end(&self) -> usize {
        self.start + self.content.len()
    }

    fn trimmed(&self) -> &str {
        self.content.trim()
    }

    fn is_blank(&self) -> bool {
        self.trimmed().is_empty()
    }

    /// Byte column of the first non-space character.
    fn indent(&self) -> usize {
        self.content.len() - self.content.trim_start().len()
    }
}

/// A translatable run being accumulated: its type, the byte span so far, and
/// the column its text starts at — continuation lines have to match it, since
/// in reStructuredText a change of indent is a change of structure.
struct Run {
    block_type: BlockType,
    span: Range<usize>,
    text_col: usize,
    /// Index of the line the run started on, so an adornment line directly
    /// under a single-line run can claim it as a title.
    start_line: usize,
    last_line: usize,
}

struct ParseState<'a> {
    source: &'a str,
    sections: Vec<Section>,
    section_idx: usize,
    block_idx: usize,
    run: Option<Run>,
    /// Adornment styles in order of first appearance; a style's position gives
    /// the heading level, the way docutils assigns them.
    styles: Vec<(char, bool)>,
    /// Set when a flushed paragraph ended with `::`: the column its text
    /// started at. The next block indented past it is a literal block.
    pending_literal: Option<usize>,
}

impl ParseState<'_> {
    /// Close the current run and emit it as a translatable block. A run whose
    /// text ends with `::` announces a literal block; the marker stays outside
    /// the span so the announcement survives the translation.
    fn flush_run(&mut self) {
        let Some(run) = self.run.take() else { return };
        let mut span = run.span;
        let raw = &self.source[span.clone()];
        if let Some(before) = raw.strip_suffix("::") {
            self.pending_literal = Some(run.text_col);
            span.end = span.start + before.trim_end().len();
        }
        self.push_span_block(run.block_type, span, BlockRole::None);
    }

    /// Emit a translatable block covering `span`, unless it carries no word —
    /// a `::` paragraph reduced to nothing, a decorative line. Emitting no
    /// block leaves the source untouched.
    fn push_span_block(&mut self, block_type: BlockType, span: Range<usize>, role: BlockRole) {
        let raw = &self.source[span.clone()];
        if !raw.chars().any(char::is_alphanumeric) {
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
            translatable: block_type.is_translatable(),
            role,
        });
    }

    /// Emit an untranslatable block kept verbatim (code, tables, raw markup).
    fn push_opaque_block(&mut self, block_type: BlockType, range: Range<usize>) {
        self.push_block(Block {
            block_type,
            segments: Vec::new(),
            raw_content: self.source[range].to_string(),
            heading_level: None,
            span: None,
            translatable: false,
            role: BlockRole::None,
        });
    }

    fn push_block(&mut self, block: Block) {
        self.sections.last_mut().unwrap().blocks.push(block);
        self.block_idx += 1;
    }

    /// Heading level of an adornment style: its position among the styles seen,
    /// the way docutils assigns levels.
    fn style_level(&mut self, marker: char, has_overline: bool) -> u8 {
        let key = (marker, has_overline);
        let pos = match self.styles.iter().position(|s| *s == key) {
            Some(pos) => pos,
            None => {
                self.styles.push(key);
                self.styles.len() - 1
            }
        };
        (pos + 1).min(u8::MAX as usize) as u8
    }

    fn start_section_if_needed(&mut self, level: u8) {
        if level <= 2 && !self.sections.last().unwrap().blocks.is_empty() {
            self.sections.push(Section { blocks: Vec::new() });
            self.section_idx += 1;
            self.block_idx = 0;
        }
    }

    fn push_title(&mut self, span: Range<usize>, level: u8, role: BlockRole) {
        self.start_section_if_needed(level);
        let raw = &self.source[span.clone()];
        let segments = make_segments(
            &normalize_inline_text(raw),
            BlockType::Heading,
            self.section_idx,
            self.block_idx,
        );
        self.push_block(Block {
            block_type: BlockType::Heading,
            segments,
            raw_content: raw.to_string(),
            heading_level: Some(level),
            span: Some(span),
            translatable: BlockType::Heading.is_translatable(),
            role,
        });
    }
}

/// The adornment character of a line made of one punctuation character
/// repeated, at least twice. `..` is explicit markup, never an adornment.
fn adornment_char(trimmed: &str) -> Option<char> {
    let first = trimmed.chars().next()?;
    (ADORNMENT_CHARS.contains(first)
        && trimmed.len() >= 2
        && trimmed != ".."
        && trimmed.chars().all(|c| c == first))
    .then_some(first)
}

/// Does an adornment of this length hold under `title`? Docutils warns on a
/// short underline but reads the title anyway once the line is unambiguous, so
/// length gates only the short lines that are likelier prose or a delimiter.
fn adornment_covers(adornment: &str, title: &str) -> bool {
    let len = adornment.chars().count();
    len >= 4 || len >= title.trim().chars().count()
}

/// `- item`, `* item`, `+ item` → byte offset of the item text.
fn parse_bullet_item(content: &str) -> Option<usize> {
    let indent = content.len() - content.trim_start().len();
    let rest = &content[indent..];
    let first = rest.chars().next()?;
    if !matches!(first, '-' | '*' | '+') {
        return None;
    }
    let after = &rest[first.len_utf8()..];
    let ws = after.len() - after.trim_start().len();
    if ws == 0 || after.trim().is_empty() {
        return None;
    }
    Some(indent + first.len_utf8() + ws)
}

/// `1. item`, `#. item`, `a) item`, `(3) item` → byte offset of the item text.
fn parse_enumerated_item(content: &str) -> Option<usize> {
    let indent = content.len() - content.trim_start().len();
    let rest = &content[indent..];

    let (open_paren, body) = match rest.strip_prefix('(') {
        Some(body) => (true, body),
        None => (false, rest),
    };
    let marker_len = if body.starts_with(|c: char| c.is_ascii_digit()) {
        body.chars().take_while(|c| c.is_ascii_digit()).count()
    } else if body.starts_with('#') {
        1
    } else if body.starts_with(|c: char| c.is_ascii_alphabetic()) {
        // A single letter; two or more would be a word.
        1
    } else {
        return None;
    };
    let after_marker = &body[marker_len..];
    let suffix = after_marker.chars().next()?;
    let valid_suffix = if open_paren { suffix == ')' } else { matches!(suffix, '.' | ')') };
    if !valid_suffix {
        return None;
    }
    // A single letter with a period reads as an initial ("J. Smith") more
    // often than as a list; docutils shares the ambiguity and authors avoid
    // it, so only digits and `#` take the period form here.
    if suffix == '.' && !body.starts_with(|c: char| c.is_ascii_digit()) && !body.starts_with('#') {
        return None;
    }
    let after = &after_marker[1..];
    let ws = after.len() - after.trim_start().len();
    if ws == 0 || after.trim().is_empty() {
        return None;
    }
    let paren = if open_paren { 1 } else { 0 };
    Some(indent + paren + marker_len + 1 + ws)
}

/// `:name: value` field marker → whether this line opens a field. The marker's
/// closing colon must be followed by a space or end the line; a backtick there
/// means an inline role (`:ref:`target``), which is prose.
fn is_field_marker(trimmed: &str) -> bool {
    let Some(rest) = trimmed.strip_prefix(':') else {
        return false;
    };
    let Some(end) = rest.find(':') else {
        return false;
    };
    let name = &rest[..end];
    if name.is_empty() || name.contains(char::is_whitespace) {
        return false;
    }
    let after = &rest[end + 1..];
    after.is_empty() || after.starts_with(' ')
}

/// `.. name:: arguments` → the directive's name.
fn parse_directive<'a>(trimmed: &'a str) -> Option<&'a str> {
    let rest = trimmed.strip_prefix("..")?.strip_prefix(char::is_whitespace)?;
    let end = rest.find("::")?;
    let name = &rest[..end];
    (!name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '+')))
    .then_some(name)
}

/// `.. [1] text`, `.. [#label] text`, `.. [CIT2002] text` → byte offset of the
/// footnote or citation text within the line.
fn parse_footnote(content: &str) -> Option<usize> {
    let indent = content.len() - content.trim_start().len();
    let rest = &content[indent..];
    let after_dots = rest.strip_prefix("..")?.strip_prefix(' ')?;
    let inner = after_dots.strip_prefix('[')?;
    let close = inner.find(']')?;
    if inner[..close].is_empty() || inner[..close].contains(char::is_whitespace) {
        return None;
    }
    let after = &inner[close + 1..];
    let ws = after.len() - after.trim_start().len();
    if ws == 0 || after.trim().is_empty() {
        return None;
    }
    Some(indent + 3 + 1 + close + 1 + ws)
}

/// A grid table border: `+---+---+` or `+===+`.
fn is_grid_border(trimmed: &str) -> bool {
    trimmed.len() >= 2
        && trimmed.starts_with('+')
        && trimmed.ends_with('+')
        && trimmed.chars().all(|c| matches!(c, '+' | '-' | '='))
        && trimmed.chars().any(|c| c == '-' || c == '=')
}

/// A simple table border: runs of `=` separated by spaces, at least two runs.
fn is_simple_table_border(trimmed: &str) -> bool {
    trimmed.chars().all(|c| c == '=' || c == ' ')
        && trimmed.split_whitespace().count() >= 2
        && trimmed.split_whitespace().all(|run| run.chars().all(|c| c == '='))
}

/// Display width the way docutils counts it for title adornments: East Asian
/// Wide and Fullwidth characters occupy two columns.
fn display_width(text: &str) -> usize {
    text.chars().map(|c| if is_wide(c) { 2 } else { 1 }).sum()
}

/// Unicode East Asian Width W or F, over the ranges that occur in practice.
fn is_wide(c: char) -> bool {
    matches!(c as u32,
        0x1100..=0x115F           // Hangul Jamo
        | 0x2E80..=0x303E         // CJK Radicals .. CJK Symbols and Punctuation
        | 0x3041..=0x33FF         // Hiragana .. CJK Compatibility
        | 0x3400..=0x4DBF         // CJK Extension A
        | 0x4E00..=0x9FFF         // CJK Unified Ideographs
        | 0xA000..=0xA4CF         // Yi
        | 0xAC00..=0xD7A3         // Hangul Syllables
        | 0xF900..=0xFAFF         // CJK Compatibility Ideographs
        | 0xFE30..=0xFE4F         // CJK Compatibility Forms
        | 0xFF00..=0xFF60         // Fullwidth Forms
        | 0xFFE0..=0xFFE6
        | 0x20000..=0x2FFFD
        | 0x30000..=0x3FFFD)
}

impl DocumentParser for RstParser {
    fn markup(&self) -> Markup {
        Markup::Rst
    }

    fn parse(&self, source: &str) -> Document {
        let lines: Vec<Line> = {
            let mut offset = 0;
            source
                .split_inclusive('\n')
                .map(|raw| {
                    let start = offset;
                    offset += raw.len();
                    let content = raw.strip_suffix('\n').unwrap_or(raw);
                    let content = content.strip_suffix('\r').unwrap_or(content);
                    Line { start, content }
                })
                .collect()
        };

        let mut state = ParseState {
            source,
            sections: vec![Section { blocks: Vec::new() }],
            section_idx: 0,
            block_idx: 0,
            run: None,
            styles: Vec::new(),
            pending_literal: None,
        };

        let mut i = 0;
        while i < lines.len() {
            let line = &lines[i];
            let trimmed = line.trimmed();

            if line.is_blank() {
                state.flush_run();
                i += 1;
                continue;
            }

            // A literal block announced by `::` claims everything indented
            // past the announcing paragraph, before any other reading: inside
            // it, a directive is code and a bullet is a bullet character.
            if let Some(threshold) = state.pending_literal {
                if line.indent() > threshold {
                    let end = consume_while_indented(&lines, i, threshold);
                    state.push_opaque_block(
                        BlockType::CodeBlock,
                        line.start..lines[end].end(),
                    );
                    state.pending_literal = None;
                    i = end + 1;
                    continue;
                }
                state.pending_literal = None;
            }

            // `::` alone is the literal-block paragraph, or the continuation
            // of one — never a transition, though the colon is an adornment
            // character.
            if trimmed == "::" {
                match &mut state.run {
                    Some(run) if run.text_col == line.indent() => {
                        run.span.end = line.end();
                        run.last_line = i;
                    }
                    _ => {
                        state.flush_run();
                        state.pending_literal = Some(line.indent());
                    }
                }
                i += 1;
                continue;
            }

            // An adornment line under a single-line run is that run's
            // underline; the pair is a section title.
            if let Some(run) = &state.run
                && run.start_line == run.last_line
                && run.last_line + 1 == i
                && run.block_type == BlockType::Paragraph
                && let Some(marker) = adornment_char(trimmed)
                && adornment_covers(trimmed, &source[run.span.clone()])
            {
                let run = state.run.take().unwrap();
                let level = state.style_level(marker, false);
                state.pending_literal = None;
                state.push_title(
                    run.span,
                    level,
                    BlockRole::AdornedTitle {
                        underline: line.start..line.end(),
                        overline: None,
                    },
                );
                i += 1;
                continue;
            }

            // An adornment line with a title and a matching adornment right
            // below opens an overlined title; anything else all-punctuation is
            // a transition, kept verbatim.
            if let Some(marker) = adornment_char(trimmed) {
                state.flush_run();
                if let (Some(title), Some(under)) = (lines.get(i + 1), lines.get(i + 2))
                    && !title.is_blank()
                    && adornment_char(title.trimmed()).is_none()
                    && under.trimmed() == trimmed
                    && adornment_covers(trimmed, title.trimmed())
                {
                    let level = state.style_level(marker, true);
                    let text_start = title.start + title.indent();
                    state.push_title(
                        text_start..title.start + title.indent() + title.trimmed().len(),
                        level,
                        BlockRole::AdornedTitle {
                            underline: under.start..under.end(),
                            overline: Some(line.start..line.end()),
                        },
                    );
                    i += 3;
                    continue;
                }
                i += 1;
                continue;
            }

            // Explicit markup: directives, targets, substitutions, footnotes,
            // comments. All begin with `..` — alone, or followed by a space.
            if trimmed == ".."
                || (trimmed.starts_with("..") && trimmed[2..].starts_with(char::is_whitespace))
            {
                state.flush_run();
                if let Some(name) = parse_directive(trimmed) {
                    if OPAQUE_DIRECTIVES.contains(&name.to_ascii_lowercase().as_str()) {
                        let end = consume_while_indented(&lines, i, line.indent());
                        state.push_opaque_block(
                            BlockType::CodeBlock,
                            line.start..lines[end].end(),
                        );
                        i = end + 1;
                    } else {
                        // The marker line stays verbatim; the indented body
                        // parses as ordinary content.
                        i += 1;
                    }
                    continue;
                }
                if let Some(text_offset) = parse_footnote(line.content) {
                    state.run = Some(Run {
                        block_type: BlockType::Paragraph,
                        span: line.start + text_offset..line.end(),
                        text_col: text_offset,
                        start_line: i,
                        last_line: i,
                    });
                    state.flush_run();
                    i += 1;
                    continue;
                }
                // Target, substitution definition, or comment: verbatim, along
                // with its indented continuation.
                let end = consume_while_indented(&lines, i, line.indent());
                i = end + 1;
                continue;
            }

            // Doctest block: opaque until the next blank line.
            if state.run.is_none() && trimmed.starts_with(">>>") {
                let mut end = i;
                while end + 1 < lines.len() && !lines[end + 1].is_blank() {
                    end += 1;
                }
                state.push_opaque_block(BlockType::CodeBlock, line.start..lines[end].end());
                i = end + 1;
                continue;
            }

            // Grid table: consecutive `+--+` / `| … |` lines, kept verbatim.
            if state.run.is_none() && is_grid_border(trimmed) {
                let mut end = i;
                while end + 1 < lines.len() {
                    let t = lines[end + 1].trimmed();
                    if t.starts_with('+') || t.starts_with('|') {
                        end += 1;
                    } else {
                        break;
                    }
                }
                state.push_opaque_block(BlockType::Table, line.start..lines[end].end());
                i = end + 1;
                continue;
            }

            // Simple table: from one `==  ==` border to the one a blank line
            // (or the file's end) follows.
            if state.run.is_none() && is_simple_table_border(trimmed) {
                let mut end = None;
                for j in i + 1..lines.len() {
                    if is_simple_table_border(lines[j].trimmed())
                        && lines.get(j + 1).is_none_or(|l| l.is_blank())
                    {
                        end = Some(j);
                        break;
                    }
                }
                if let Some(end) = end {
                    state.push_opaque_block(BlockType::Table, line.start..lines[end].end());
                    i = end + 1;
                    continue;
                }
                // No closing border: not a table after all; fall through.
            }

            // Field list entry (`:name: value`) or line block (`| text`):
            // structure and metadata, kept verbatim.
            if is_field_marker(trimmed) || trimmed == "|" || trimmed.starts_with("| ") {
                state.flush_run();
                i += 1;
                continue;
            }

            if let Some(text_offset) =
                parse_bullet_item(line.content).or_else(|| parse_enumerated_item(line.content))
            {
                state.flush_run();
                state.run = Some(Run {
                    block_type: BlockType::ListItem,
                    span: line.start + text_offset..line.end(),
                    text_col: text_offset,
                    start_line: i,
                    last_line: i,
                });
                i += 1;
                continue;
            }

            // Plain text: continue the run when the indent holds, break the
            // block when it does not — in reStructuredText a change of indent
            // is a change of structure (a definition, a quote), and joining
            // across it would splice that structure away.
            let col = line.indent();
            match &mut state.run {
                Some(run) if run.text_col == col => {
                    run.span.end = line.end();
                    run.last_line = i;
                }
                _ => {
                    state.flush_run();
                    state.run = Some(Run {
                        block_type: BlockType::Paragraph,
                        span: line.start + col..line.end(),
                        text_col: col,
                        start_line: i,
                        last_line: i,
                    });
                }
            }
            i += 1;
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
        let mut splices = collect_splices(document, translations);
        splices.extend(adornment_edits(document, translations));
        apply_splices(&document.source, splices)
    }
}

/// The last line of the indented block starting under line `at`: lines more
/// indented than `threshold` belong to it, and blank lines do not end it —
/// only a dedent does.
fn consume_while_indented(lines: &[Line], at: usize, threshold: usize) -> usize {
    let mut last = at;
    let mut j = at + 1;
    while j < lines.len() {
        if lines[j].is_blank() {
            j += 1;
            continue;
        }
        if lines[j].indent() > threshold {
            last = j;
            j += 1;
        } else {
            break;
        }
    }
    last
}

/// Redraw the adornment lines of each translated title. Docutils requires an
/// underline (and overline) to cover the title's display width, where a Hangul
/// syllable is two columns; a translation almost never keeps the original
/// width, so leaving the lines verbatim would demote the title or fail the
/// build.
fn adornment_edits(
    document: &Document,
    translations: &TranslationMap,
) -> Vec<(Range<usize>, String)> {
    let mut edits = Vec::new();
    for block in document.sections.iter().flat_map(|s| s.blocks.iter()) {
        let BlockRole::AdornedTitle { underline, overline } = &block.role else {
            continue;
        };
        if !block
            .segments
            .iter()
            .any(|seg| translations.contains_key(&seg.id))
        {
            continue;
        }
        let Some(marker) = document.source[underline.clone()].chars().next() else {
            continue;
        };
        let width = display_width(&join_segments_with_translations(
            &block.segments,
            translations,
        ));
        let drawn = marker.to_string().repeat(width.max(4));
        edits.push((underline.clone(), drawn.clone()));
        if let Some(over) = overline {
            edits.push((over.clone(), drawn));
        }
    }
    edits
}

#[cfg(test)]
mod tests {
    use super::*;

    fn translate_all(parser: &RstParser, source: &str, pairs: &[(&str, &str)]) -> String {
        let doc = parser.parse(source);
        let mut translations = TranslationMap::new();
        for seg in doc.all_segments() {
            if let Some((_, t)) = pairs.iter().find(|(s, _)| *s == seg.source) {
                translations.insert(seg.id.clone(), t.to_string());
            }
        }
        parser.reconstruct(&doc, &translations)
    }

    #[test]
    fn parse_simple_paragraph() {
        let doc = RstParser.parse("Hello world. Goodbye world.");
        let segments = doc.translatable_segments();
        assert_eq!(segments.len(), 2);
        assert_eq!(segments[0].source, "Hello world.");
        assert_eq!(segments[1].source, "Goodbye world.");
    }

    #[test]
    fn wrapped_paragraph_is_one_block() {
        let doc = RstParser.parse("This is one\nwrapped sentence.\n");
        let segments = doc.translatable_segments();
        assert_eq!(segments.len(), 1);
        assert_eq!(segments[0].source, "This is one wrapped sentence.");
    }

    #[test]
    fn underlined_title_is_a_heading() {
        let doc = RstParser.parse("Chapter One\n===========\n\nBody text.\n");
        let segments = doc.translatable_segments();
        assert_eq!(segments[0].source, "Chapter One");
        assert_eq!(segments[0].block_type, BlockType::Heading);
    }

    #[test]
    fn adornment_styles_order_gives_levels() {
        let doc = RstParser.parse(
            "Title\n=====\n\nA.\n\nSection\n-------\n\nB.\n\nOther\n=====\n\nC.\n",
        );
        let blocks: Vec<&Block> = doc.sections.iter().flat_map(|s| &s.blocks).collect();
        let levels: Vec<u8> = blocks
            .iter()
            .filter(|b| b.block_type == BlockType::Heading)
            .map(|b| b.heading_level.unwrap())
            .collect();
        assert_eq!(levels, vec![1, 2, 1]);
    }

    #[test]
    fn overlined_title_is_a_heading() {
        let doc = RstParser.parse("=====\nTitle\n=====\n\nBody.\n");
        let segments = doc.translatable_segments();
        assert_eq!(segments[0].source, "Title");
        assert_eq!(segments[0].block_type, BlockType::Heading);
    }

    #[test]
    fn translated_title_redraws_its_underline_to_display_width() {
        let output = translate_all(
            &RstParser,
            "Getting Started\n===============\n\nBody.\n",
            &[("Getting Started", "시작하기"), ("Body.", "본문.")],
        );
        // Four Hangul syllables are eight columns wide.
        assert_eq!(output, "시작하기\n========\n\n본문.\n");
    }

    #[test]
    fn translated_overlined_title_redraws_both_lines() {
        let output = translate_all(
            &RstParser,
            "===============\nGetting Started\n===============\n\nBody.\n",
            &[("Getting Started", "시작하기"), ("Body.", "본문.")],
        );
        assert_eq!(output, "========\n시작하기\n========\n\n본문.\n");
    }

    #[test]
    fn literal_block_after_double_colon_is_opaque() {
        let source = "Some code::\n\n    print(\"hi\")\n    more()\n\nAfter text.\n";
        let doc = RstParser.parse(source);
        let segments = doc.translatable_segments();
        assert_eq!(segments.len(), 2);
        assert_eq!(segments[0].source, "Some code");
        assert_eq!(segments[1].source, "After text.");

        let output = translate_all(
            &RstParser,
            source,
            &[("Some code", "예시 코드"), ("After text.", "이후.")],
        );
        assert_eq!(output, "예시 코드::\n\n    print(\"hi\")\n    more()\n\n이후.\n");
    }

    #[test]
    fn standalone_double_colon_paragraph_stays_verbatim() {
        let source = "Intro.\n\n::\n\n    literal\n\nAfter.\n";
        let doc = RstParser.parse(source);
        let segments = doc.translatable_segments();
        assert_eq!(segments.len(), 2);
        let output = translate_all(&RstParser, source, &[("Intro.", "소개."), ("After.", "이후.")]);
        assert_eq!(output, "소개.\n\n::\n\n    literal\n\n이후.\n");
    }

    #[test]
    fn double_colon_directly_under_a_paragraph_continues_it() {
        let source = "Intro.\n::\n\n    literal\n\nAfter.\n";
        let doc = RstParser.parse(source);
        let segments = doc.translatable_segments();
        assert_eq!(segments.len(), 2);
        assert_eq!(segments[0].source, "Intro.");
        assert_eq!(segments[1].source, "After.");
    }

    #[test]
    fn expanded_double_colon_keeps_its_space() {
        // `text ::` renders without the colon; the marker still stays outside
        // the span.
        let source = "As follows ::\n\n    x = 1\n";
        let doc = RstParser.parse(source);
        assert_eq!(doc.translatable_segments()[0].source, "As follows");
        let output = translate_all(&RstParser, source, &[("As follows", "다음과 같이")]);
        assert_eq!(output, "다음과 같이 ::\n\n    x = 1\n");
    }

    #[test]
    fn code_block_directive_is_opaque() {
        let source = "Before.\n\n.. code-block:: python\n   :linenos:\n\n   def f():\n       pass\n\nAfter.\n";
        let doc = RstParser.parse(source);
        let segments = doc.translatable_segments();
        assert_eq!(segments.len(), 2);
        assert_eq!(segments[0].source, "Before.");
        assert_eq!(segments[1].source, "After.");
    }

    #[test]
    fn note_directive_body_is_translatable() {
        let source = ".. note::\n\n   Remember this fact.\n\nAfter.\n";
        let doc = RstParser.parse(source);
        let segments = doc.translatable_segments();
        assert_eq!(segments.len(), 2);
        assert_eq!(segments[0].source, "Remember this fact.");

        let output = translate_all(
            &RstParser,
            source,
            &[("Remember this fact.", "이 사실을 기억하세요."), ("After.", "이후.")],
        );
        assert_eq!(output, ".. note::\n\n   이 사실을 기억하세요.\n\n이후.\n");
    }

    #[test]
    fn function_directive_body_is_translatable_but_signature_is_not() {
        let source = ".. function:: attach_gdb()\n\n   Run an interp-level gdb.\n";
        let doc = RstParser.parse(source);
        let segments = doc.translatable_segments();
        assert_eq!(segments.len(), 1);
        assert_eq!(segments[0].source, "Run an interp-level gdb.");
    }

    #[test]
    fn toctree_is_opaque() {
        let source = "Head.\n\n.. toctree::\n  :maxdepth: 1\n\n  introduction\n  architecture\n\nTail.\n";
        let doc = RstParser.parse(source);
        let segments = doc.translatable_segments();
        assert_eq!(segments.len(), 2);
        let output = translate_all(&RstParser, source, &[("Head.", "머리."), ("Tail.", "꼬리.")]);
        assert_eq!(
            output,
            "머리.\n\n.. toctree::\n  :maxdepth: 1\n\n  introduction\n  architecture\n\n꼬리.\n"
        );
    }

    #[test]
    fn hyperlink_targets_stay_verbatim() {
        let source = "See the site.\n\n.. _fast: https://speed.pypy.org\n.. _Python: https://python.org/\n";
        let doc = RstParser.parse(source);
        let segments = doc.translatable_segments();
        assert_eq!(segments.len(), 1);
        assert_eq!(segments[0].source, "See the site.");
    }

    #[test]
    fn comment_is_opaque() {
        let source = ".. this is a comment\n   continued here\n\nReal text.\n";
        let doc = RstParser.parse(source);
        let segments = doc.translatable_segments();
        assert_eq!(segments.len(), 1);
        assert_eq!(segments[0].source, "Real text.");
    }

    #[test]
    fn doctest_block_is_opaque() {
        let source = "Try it:\n\n>>> 1 + 1\n2\n\nDone.\n";
        let doc = RstParser.parse(source);
        let segments = doc.translatable_segments();
        assert_eq!(segments.len(), 2);
        assert_eq!(segments[0].source, "Try it:");
        assert_eq!(segments[1].source, "Done.");
    }

    #[test]
    fn bullet_list_items_are_separate() {
        let source = "* First item.\n* Second item.\n";
        let doc = RstParser.parse(source);
        let segments = doc.translatable_segments();
        assert_eq!(segments.len(), 2);
        assert_eq!(segments[0].block_type, BlockType::ListItem);
        let output = translate_all(
            &RstParser,
            source,
            &[("First item.", "첫째."), ("Second item.", "둘째.")],
        );
        assert_eq!(output, "* 첫째.\n* 둘째.\n");
    }

    #[test]
    fn wrapped_list_item_joins_its_continuation() {
        let source = "* If you want to help develop PyPy, have a look\n  at contributing.\n";
        let doc = RstParser.parse(source);
        let segments = doc.translatable_segments();
        assert_eq!(segments.len(), 1);
        assert_eq!(
            segments[0].source,
            "If you want to help develop PyPy, have a look at contributing."
        );
    }

    #[test]
    fn enumerated_list_items_are_separate() {
        let source = "1. First step.\n2. Second step.\n#. Third step.\n";
        let doc = RstParser.parse(source);
        let segments = doc.translatable_segments();
        assert_eq!(segments.len(), 3);
        assert!(segments.iter().all(|s| s.block_type == BlockType::ListItem));
    }

    #[test]
    fn an_initial_is_not_a_list_marker() {
        assert_eq!(parse_enumerated_item("J. Smith wrote this"), None);
        assert!(parse_enumerated_item("1. A real item").is_some());
        assert!(parse_enumerated_item("a) Also real").is_some());
    }

    #[test]
    fn field_list_stays_verbatim() {
        let source = ":author: Someone\n:date: today\n\nBody.\n";
        let doc = RstParser.parse(source);
        let segments = doc.translatable_segments();
        assert_eq!(segments.len(), 1);
        assert_eq!(segments[0].source, "Body.");
    }

    #[test]
    fn a_role_at_line_start_is_not_a_field() {
        let source = "See\n:ref:`contact` for details.\n";
        let doc = RstParser.parse(source);
        let segments = doc.translatable_segments();
        assert_eq!(segments.len(), 1);
        assert_eq!(segments[0].source, "See :ref:`contact` for details.");
    }

    #[test]
    fn definition_body_splits_from_its_term() {
        // The indent change is structure; joining across it would splice the
        // definition list away.
        let source = "term\n    the definition text\n";
        let doc = RstParser.parse(source);
        let segments = doc.translatable_segments();
        assert_eq!(segments.len(), 2);
        assert_eq!(segments[0].source, "term");
        assert_eq!(segments[1].source, "the definition text");

        let output = translate_all(
            &RstParser,
            source,
            &[("term", "용어"), ("the definition text", "정의 텍스트")],
        );
        assert_eq!(output, "용어\n    정의 텍스트\n");
    }

    #[test]
    fn grid_table_is_opaque() {
        let source = "+----+----+\n| a  | b  |\n+====+====+\n| c  | d  |\n+----+----+\n\nAfter.\n";
        let doc = RstParser.parse(source);
        let segments = doc.translatable_segments();
        assert_eq!(segments.len(), 1);
        assert_eq!(segments[0].source, "After.");
    }

    #[test]
    fn simple_table_is_opaque() {
        let source = "=====  =====\ncol A  col B\n=====  =====\nx      y\n=====  =====\n\nAfter.\n";
        let doc = RstParser.parse(source);
        let segments = doc.translatable_segments();
        assert_eq!(segments.len(), 1);
        assert_eq!(segments[0].source, "After.");
    }

    #[test]
    fn footnote_text_is_translatable() {
        let source = ".. [1] The footnote text.\n";
        let doc = RstParser.parse(source);
        let segments = doc.translatable_segments();
        assert_eq!(segments.len(), 1);
        assert_eq!(segments[0].source, "The footnote text.");
        let output = translate_all(&RstParser, source, &[("The footnote text.", "각주.")]);
        assert_eq!(output, ".. [1] 각주.\n");
    }

    #[test]
    fn segments_keep_inline_markup() {
        let doc = RstParser.parse(
            "Consult :source:`LICENSE` or the `PyPy website`_ for ``code`` and **bold**.\n",
        );
        let segments = doc.translatable_segments();
        assert_eq!(segments.len(), 1);
        assert_eq!(
            segments[0].source,
            "Consult :source:`LICENSE` or the `PyPy website`_ for ``code`` and **bold**."
        );
    }

    #[test]
    fn transition_stays_verbatim() {
        let source = "Before.\n\n----\n\nAfter.\n";
        let doc = RstParser.parse(source);
        let segments = doc.translatable_segments();
        assert_eq!(segments.len(), 2);
        let output = translate_all(&RstParser, source, &[("Before.", "전."), ("After.", "후.")]);
        assert_eq!(output, "전.\n\n----\n\n후.\n");
    }

    #[test]
    fn reconstruct_without_translations_is_identity() {
        let source = "Title\n=====\n\nBody text::\n\n    code\n\n* item\n\n.. note::\n\n   text\n";
        let doc = RstParser.parse(source);
        assert_eq!(RstParser.reconstruct(&doc, &TranslationMap::new()), source);
    }

    #[test]
    fn nested_directive_inside_note_body() {
        let source = ".. note::\n\n   Prose here.\n\n   .. code-block:: python\n\n      x = 1\n\nAfter.\n";
        let doc = RstParser.parse(source);
        let segments = doc.translatable_segments();
        assert_eq!(segments.len(), 2);
        assert_eq!(segments[0].source, "Prose here.");
        assert_eq!(segments[1].source, "After.");
    }

    #[test]
    fn titles_split_sections() {
        let doc = RstParser.parse("One\n===\n\nA.\n\nTwo\n===\n\nB.\n");
        assert_eq!(doc.sections.len(), 2);
    }
}
