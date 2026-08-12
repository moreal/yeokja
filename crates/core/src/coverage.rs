//! What a parse passed over.
//!
//! A parser that fails to recognise a construct does not report an error — it
//! quietly produces fewer blocks, and the text inside is never offered for
//! translation. Progress then reads as complete because the missing lines were
//! never in the denominator. The only way to notice is to measure the parse
//! against the source it came from.
//!
//! Coverage deliberately counts a code block as *not offered*. Skipping it
//! would hide the failure worth catching most: prose that a parser mistook for
//! code and swallowed whole. So this reports every run it passed over and
//! leaves the reading to a human, who can tell Erlang from English at a glance.

use crate::model::Document;

/// Longest preview kept for a gap, in characters.
const PREVIEW_WIDTH: usize = 60;

/// A run of source lines no translatable span reaches.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Gap {
    /// 1-based, inclusive.
    pub first_line: usize,
    /// 1-based, inclusive. The last line in the run carrying a word.
    pub last_line: usize,
    /// Lines in the run carrying a word. Blank lines and bare delimiters are
    /// spanned by the run but not counted, so its size reflects lost text.
    pub lines: usize,
    /// The run's first line with a word in it, for the reader to judge by.
    pub preview: String,
}

/// How much of a document's text a parse offered for translation.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Coverage {
    /// Lines carrying at least one word. A blank line or a bare `----` is
    /// neither offered nor missing, so neither is counted.
    pub word_lines: usize,
    /// Of those, the ones a translatable block's span reaches.
    pub offered_lines: usize,
    /// Runs it did not reach, longest first.
    pub gaps: Vec<Gap>,
}

impl Coverage {
    /// Share of word-carrying lines offered for translation, 0.0 to 1.0.
    /// A document with no text at all counts as fully covered.
    pub fn ratio(&self) -> f64 {
        if self.word_lines == 0 {
            return 1.0;
        }
        self.offered_lines as f64 / self.word_lines as f64
    }

    /// The longest run passed over, if any.
    pub fn largest_gap(&self) -> Option<&Gap> {
        self.gaps.first()
    }
}

/// Measure `document` against the source it was parsed from.
///
/// Only `translatable` blocks with a span count as offering their lines, so
/// this measures the parse rather than the configuration. Run it before
/// applying selection rules; a column a rule excludes is a deliberate choice,
/// not a gap.
pub fn coverage(document: &Document) -> Coverage {
    let lines = Lines::of(&document.source);
    let mut offered = vec![false; lines.count()];

    for block in document.sections.iter().flat_map(|s| s.blocks.iter()) {
        let Some(span) = &block.span else { continue };
        if !block.translatable {
            continue;
        }
        for line in lines.covering(span.start, span.end) {
            offered[line] = true;
        }
    }

    let mut word_lines = 0usize;
    let mut offered_lines = 0usize;
    let mut gaps = Vec::new();
    let mut open: Option<Gap> = None;

    for (index, text) in lines.iter().enumerate() {
        let has_word = text.chars().any(char::is_alphanumeric);
        if has_word {
            word_lines += 1;
        }

        if offered[index] {
            // An offered line ends whatever run preceded it, whether or not it
            // carries a word — the parser reached this far.
            if has_word {
                offered_lines += 1;
            }
            gaps.extend(open.take());
            continue;
        }

        if !has_word {
            // Blank lines and bare delimiters sit inside a run without
            // extending it, so a gap never trails off into empty space.
            continue;
        }

        let number = index + 1;
        match &mut open {
            Some(gap) => {
                gap.last_line = number;
                gap.lines += 1;
            }
            None => {
                open = Some(Gap {
                    first_line: number,
                    last_line: number,
                    lines: 1,
                    preview: preview(text),
                })
            }
        }
    }
    gaps.extend(open);
    gaps.sort_by(|a, b| b.lines.cmp(&a.lines).then(a.first_line.cmp(&b.first_line)));

    Coverage {
        word_lines,
        offered_lines,
        gaps,
    }
}

fn preview(line: &str) -> String {
    let text = line.trim();
    match text.char_indices().nth(PREVIEW_WIDTH) {
        Some((cut, _)) => format!("{}…", &text[..cut]),
        None => text.to_string(),
    }
}

/// The source split into lines, with byte offsets kept so spans can be mapped
/// back onto line numbers.
struct Lines<'a> {
    text: Vec<&'a str>,
    /// Byte offset each line starts at, ascending.
    starts: Vec<usize>,
}

impl<'a> Lines<'a> {
    fn of(source: &'a str) -> Self {
        let mut text = Vec::new();
        let mut starts = Vec::new();
        let mut offset = 0usize;
        for line in source.split_inclusive('\n') {
            starts.push(offset);
            offset += line.len();
            text.push(line.trim_end_matches(['\n', '\r']));
        }
        Self { text, starts }
    }

    fn count(&self) -> usize {
        self.text.len()
    }

    fn iter(&self) -> impl Iterator<Item = &'a str> {
        self.text.clone().into_iter()
    }

    /// Indices of the lines the byte range `start..end` touches. An empty range
    /// still touches the line it sits on.
    fn covering(&self, start: usize, end: usize) -> std::ops::Range<usize> {
        let first = self.starts.partition_point(|&s| s <= start).saturating_sub(1);
        let last = self
            .starts
            .partition_point(|&s| s <= end.max(start))
            .saturating_sub(1);
        first..(last + 1).min(self.count())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::*;

    /// A block claiming `span`, as a translatable parser would emit.
    fn offered(source: &str, span: std::ops::Range<usize>) -> Block {
        Block {
            block_type: BlockType::Paragraph,
            segments: Vec::new(),
            raw_content: source[span.clone()].to_string(),
            heading_level: None,
            span: Some(span),
            role: BlockRole::None,
            translatable: true,
        }
    }

    fn document(source: &str, blocks: Vec<Block>) -> Document {
        Document {
            sections: vec![Section { blocks }],
            source: source.to_string(),
        }
    }

    fn span_of(source: &str, needle: &str) -> std::ops::Range<usize> {
        let start = source.find(needle).expect("needle is in the source");
        start..start + needle.len()
    }

    #[test]
    fn a_fully_parsed_document_has_no_gaps() {
        let source = "First line.\n\nSecond line.\n";
        let doc = document(
            source,
            vec![
                offered(source, span_of(source, "First line.")),
                offered(source, span_of(source, "Second line.")),
            ],
        );
        let result = coverage(&doc);
        assert_eq!(result.gaps, vec![]);
        assert_eq!(result.word_lines, 2);
        assert_eq!(result.offered_lines, 2);
        assert_eq!(result.ratio(), 1.0);
    }

    #[test]
    fn text_no_block_reaches_is_a_gap() {
        let source = "Kept.\n\nLost one.\nLost two.\n";
        let doc = document(source, vec![offered(source, span_of(source, "Kept."))]);
        let result = coverage(&doc);
        assert_eq!(result.gaps.len(), 1);
        let gap = &result.gaps[0];
        assert_eq!((gap.first_line, gap.last_line, gap.lines), (3, 4, 2));
        assert_eq!(gap.preview, "Lost one.");
        assert_eq!(result.offered_lines, 1);
        assert_eq!(result.word_lines, 3);
    }

    #[test]
    fn a_blank_line_does_not_split_a_gap() {
        let source = "One.\n\nTwo.\n\nThree.\n";
        let doc = document(source, vec![]);
        let result = coverage(&doc);
        assert_eq!(result.gaps.len(), 1);
        assert_eq!(result.gaps[0].lines, 3);
        assert_eq!((result.gaps[0].first_line, result.gaps[0].last_line), (1, 5));
    }

    #[test]
    fn an_offered_line_splits_a_gap_in_two() {
        let source = "Lost.\nKept.\nLost again.\n";
        let doc = document(source, vec![offered(source, span_of(source, "Kept."))]);
        let result = coverage(&doc);
        assert_eq!(result.gaps.len(), 2);
        // Equal-sized gaps keep source order.
        assert_eq!(result.gaps[0].first_line, 1);
        assert_eq!(result.gaps[1].first_line, 3);
    }

    #[test]
    fn a_gap_does_not_trail_into_blank_lines() {
        let source = "Lost.\n\n\n";
        let result = coverage(&document(source, vec![]));
        assert_eq!(result.gaps[0].last_line, 1);
    }

    /// Nothing distinguishes a code block from prose the parser mistook for
    /// one, so both are reported and the preview tells them apart.
    #[test]
    fn an_untranslatable_block_does_not_cover_its_lines() {
        let source = "----\ncode goes here\n----\n";
        let block = Block {
            block_type: BlockType::CodeBlock,
            segments: Vec::new(),
            raw_content: source.to_string(),
            heading_level: None,
            span: Some(0..source.len()),
            role: BlockRole::None,
            translatable: false,
        };
        let result = coverage(&document(source, vec![block]));
        assert_eq!(result.gaps.len(), 1);
        assert_eq!(result.gaps[0].preview, "code goes here");
    }

    #[test]
    fn gaps_are_ordered_longest_first() {
        let source = "a\nKept.\nb\nc\nd\n";
        let doc = document(source, vec![offered(source, span_of(source, "Kept."))]);
        let result = coverage(&doc);
        assert_eq!(result.gaps[0].lines, 3);
        assert_eq!(result.gaps[1].lines, 1);
        assert_eq!(result.largest_gap().map(|g| g.first_line), Some(3));
    }

    #[test]
    fn a_span_over_several_lines_covers_all_of_them() {
        let source = "wrapped over\ntwo lines.\n\nlost\n";
        let doc = document(
            source,
            vec![offered(source, span_of(source, "wrapped over\ntwo lines."))],
        );
        let result = coverage(&doc);
        assert_eq!(result.offered_lines, 2);
        assert_eq!(result.gaps.len(), 1);
        assert_eq!(result.gaps[0].first_line, 4);
    }

    #[test]
    fn a_long_line_is_previewed_in_part() {
        let source = "x".repeat(200);
        let result = coverage(&document(&source, vec![]));
        assert_eq!(result.gaps[0].preview.chars().count(), PREVIEW_WIDTH + 1);
    }

    #[test]
    fn a_document_with_no_text_is_fully_covered() {
        let result = coverage(&document("\n\n----\n", vec![]));
        assert_eq!(result.ratio(), 1.0);
        assert_eq!(result.gaps, vec![]);
    }
}
