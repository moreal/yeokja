//! Line offsets of `.. include::` directives across a translation.
//!
//! `:start-line:` and `:end-line:` name 0-based line offsets into the included
//! file. A translation re-wraps paragraphs and so moves every line after the
//! first one it touches, which leaves those offsets pointing into the wrong
//! place of the translated target. The block sequence of a span-based parse is
//! 1:1 between a source and its reconstruction, though, and everything outside
//! the spliced spans is copied byte-for-byte, so a line can be carried across
//! through the nearest block boundary: the same offset within a block, clamped
//! to the block's translated extent, or the same distance from the next block
//! for a line in the verbatim gap between two blocks.

use yeokja_core::model::Document;

/// The lines a block's span covers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Extent {
    /// 0-based line of the block's first byte.
    start: usize,
    /// 0-based line of the block's last byte.
    end: usize,
}

/// Correspondence between the 0-based line numbers of a source text and
/// those of its translation, taken from the block spans of both parses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LineMap {
    blocks: Vec<(Extent, Extent)>,
}

impl LineMap {
    /// Pair the spanned blocks of a source parse with those of the parse of
    /// its translation. `None` when the two block sequences do not line up,
    /// in which case no offset can be trusted and a directive should be left
    /// as it was.
    pub fn between(source: &Document, translated: &Document) -> Option<Self> {
        let from = outer_spans(source);
        let to = outer_spans(translated);
        if from.len() != to.len() || from.iter().zip(&to).any(|((a, _), (b, _))| a != b) {
            return None;
        }
        let blocks = from
            .into_iter()
            .zip(to)
            .map(|((_, a), (_, b))| (extent(&source.source, a), extent(&translated.source, b)))
            .collect();
        Some(Self { blocks })
    }

    /// The translated line that corresponds to source line `line` (0-based).
    pub fn map(&self, line: usize) -> usize {
        if let Some((from, to)) = self
            .blocks
            .iter()
            .find(|(from, _)| from.start <= line && line <= from.end)
        {
            return to.start + (line - from.start).min(to.end - to.start);
        }
        match self.blocks.iter().position(|(from, _)| from.start > line) {
            Some(next) => {
                let (from, to) = &self.blocks[next];
                let mapped = to.start.saturating_sub(from.start - line);
                match next.checked_sub(1).map(|i| self.blocks[i].1) {
                    Some(prev) => mapped.max(prev.end + 1).min(to.start),
                    None => mapped,
                }
            }
            None => match self.blocks.last() {
                Some((from, to)) => to.end + (line - from.end),
                None => line,
            },
        }
    }
}

/// Spanned blocks in document order, without those nested inside an earlier
/// span (table cells inside their table's anchor).
fn outer_spans(
    document: &Document,
) -> Vec<(yeokja_core::model::BlockType, std::ops::Range<usize>)> {
    let mut spans = Vec::new();
    let mut covered = 0usize;
    for block in document.sections.iter().flat_map(|section| &section.blocks) {
        let Some(span) = &block.span else { continue };
        if span.start < covered {
            continue;
        }
        covered = span.end;
        spans.push((block.block_type, span.clone()));
    }
    spans
}

fn extent(text: &str, span: std::ops::Range<usize>) -> Extent {
    let last = span.end.saturating_sub(1).max(span.start);
    Extent {
        start: line_of(text, span.start),
        end: line_of(text, last),
    }
}

fn line_of(text: &str, offset: usize) -> usize {
    text.as_bytes()[..offset.min(text.len())]
        .iter()
        .filter(|byte| **byte == b'\n')
        .count()
}

/// Rewrite the `:start-line:` and `:end-line:` options of every `include`
/// directive in `text` through the line map `resolve` returns for its target
/// (the path exactly as written in the directive). A directive whose target
/// resolves to `None`, or whose option value is not a number, is left alone.
pub fn rewrite_line_options(
    text: &str,
    mut resolve: impl FnMut(&str) -> Option<LineMap>,
) -> String {
    let lines: Vec<&str> = text.split_inclusive('\n').collect();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        let Some((indent, target)) = include_directive(line) else {
            out.push_str(line);
            i += 1;
            continue;
        };
        out.push_str(line);
        i += 1;
        // Resolved once per directive, and only if it has an option to rewrite.
        let mut map: Option<Option<LineMap>> = None;
        while i < lines.len() {
            let option = lines[i];
            let content = option.trim_end_matches(['\n', '\r']);
            if content.trim().is_empty() || indent_of(content) <= indent {
                break;
            }
            match line_option(content) {
                Some((name, value_start, value_end)) if is_line_option(name) => {
                    let map = map.get_or_insert_with(|| resolve(target));
                    let rewritten = map.as_ref().and_then(|map| {
                        let value: usize = content[value_start..value_end].parse().ok()?;
                        Some(map.map(value))
                    });
                    match rewritten {
                        Some(value) => {
                            out.push_str(&option[..value_start]);
                            out.push_str(&value.to_string());
                            out.push_str(&option[value_end..]);
                        }
                        None => out.push_str(option),
                    }
                }
                _ => out.push_str(option),
            }
            i += 1;
        }
    }
    out
}

fn is_line_option(name: &str) -> bool {
    name.eq_ignore_ascii_case("start-line") || name.eq_ignore_ascii_case("end-line")
}

fn indent_of(line: &str) -> usize {
    line.len() - line.trim_start().len()
}

/// `(indent, target)` of an `.. include:: target` line.
fn include_directive(line: &str) -> Option<(usize, &str)> {
    let content = line.trim_end_matches(['\n', '\r']);
    let indent = indent_of(content);
    let rest = content[indent..].strip_prefix("..")?;
    let rest = rest.strip_prefix(char::is_whitespace)?.trim_start();
    let (name, rest) = rest.split_once("::")?;
    if !name.trim().eq_ignore_ascii_case("include") {
        return None;
    }
    let target = rest.trim();
    (!target.is_empty()).then_some((indent, target))
}

/// `(name, value byte range)` of a `:name: value` option line.
fn line_option(content: &str) -> Option<(&str, usize, usize)> {
    let indent = indent_of(content);
    let body = &content[indent..];
    let after_colon = body.strip_prefix(':')?;
    let close = after_colon.find(':')?;
    let name = &after_colon[..close];
    let value_offset = indent + 1 + close + 1;
    let value = &content[value_offset..];
    let leading = value.len() - value.trim_start().len();
    let value_start = value_offset + leading;
    let value_end = value_start + value.trim().len();
    Some((name, value_start, value_end))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::RstParser;
    use yeokja_core::parser::{DocumentParser, TranslationMap};

    /// Reconstruct `source` with every segment replaced by `translate(segment)`.
    fn translated(source: &str, translate: impl Fn(&str) -> String) -> String {
        let doc = RstParser.parse(source);
        let mut translations = TranslationMap::new();
        for seg in doc.translatable_segments() {
            translations.insert(seg.id.clone(), translate(&seg.source));
        }
        RstParser.reconstruct(&doc, &translations)
    }

    fn line_map(source: &str, output: &str) -> LineMap {
        LineMap::between(&RstParser.parse(source), &RstParser.parse(output)).unwrap()
    }

    /// Three-line paragraphs, a target, a heading, and the tuning prose that
    /// follows, laid out like gc_info.rst: the heading on 0-based line 304,
    /// its underline on 305, the prose from 307.
    fn gc_info_like() -> String {
        let mut source = String::new();
        for i in 0..75 {
            source.push_str(&format!(
                "Paragraph {i} explains the collector\nover three wrapped lines\nof English prose.\n\n"
            ));
        }
        source.push_str("Short.\n\n");
        assert_eq!(source.lines().count(), 302);
        source.push_str(".. _env:\n\nEnvironment variables\n---------------------\n\n");
        source.push_str("Set ``PYPY_GC_NURSERY``\nto tune the nursery.\n");
        source
    }

    fn one_line(seg: &str) -> String {
        format!(
            "번역: {}",
            seg.split_whitespace().collect::<Vec<_>>().join(" ")
        )
    }

    #[test]
    fn a_heading_follows_its_moved_translation() {
        let source = gc_info_like();
        let output = translated(&source, one_line);
        assert_eq!(source.lines().nth(304), Some("Environment variables"));
        // Every paragraph collapsed to one line: 76 * 2 lines, the target, a blank.
        let translated_heading = output
            .lines()
            .position(|l| l.starts_with("번역: Environment"))
            .unwrap();
        assert_eq!(translated_heading, 154);

        let map = line_map(&source, &output);
        assert_eq!(map.map(304), 154);
        // The verbatim target line before it rides along.
        assert_eq!(map.map(302), 152);
        assert_eq!(output.lines().nth(152), Some(".. _env:"));
        // The prose after the heading, and a line past the end.
        assert_eq!(map.map(307), 157);
        assert_eq!(map.map(308), 157);
        assert_eq!(map.map(309), 158);
    }

    #[test]
    fn a_start_line_on_the_underline_stays_on_the_underline() {
        let source = gc_info_like();
        let output = translated(&source, one_line);
        let map = line_map(&source, &output);
        assert!(source.lines().nth(305).unwrap().starts_with("---"));
        assert_eq!(map.map(305), 155);
        assert!(output.lines().nth(155).unwrap().starts_with("---"));
    }

    #[test]
    fn a_line_inside_a_rewrapped_paragraph_maps_to_its_start() {
        let source = "First line of the paragraph\nsecond line\nthird line.\n\nNext.\n";
        let output = translated(source, one_line);
        let map = line_map(source, &output);
        assert_eq!(map.map(0), 0);
        assert_eq!(map.map(1), 0);
        assert_eq!(map.map(2), 0);
        assert_eq!(map.map(4), 2);
    }

    #[test]
    fn an_offset_inside_a_verbatim_block_is_kept() {
        let source = "Intro text\nwrapped here::\n\n    line a\n    line b\n    line c\n\nAfter.\n";
        let output = translated(source, one_line);
        let map = line_map(source, &output);
        assert_eq!(output.lines().nth(map.map(4)), Some("    line b"));
        assert_eq!(output.lines().nth(map.map(7)), Some("번역: After."));
    }

    #[test]
    fn end_lines_past_the_end_stay_past_the_end() {
        let source = "One\ntwo\nthree.\n\nFour.\n";
        let output = translated(source, one_line);
        let map = line_map(source, &output);
        assert_eq!(output.lines().count(), 3);
        assert_eq!(map.map(5), 3);
        assert_eq!(map.map(50), 48);
    }

    #[test]
    fn a_mismatched_block_sequence_gives_no_map() {
        let source = "One.\n\nTwo.\n";
        let output = "One.\n";
        assert!(LineMap::between(&RstParser.parse(source), &RstParser.parse(output)).is_none());
    }

    fn map_of(source: &str) -> LineMap {
        line_map(source, &translated(source, one_line))
    }

    #[test]
    fn start_and_end_lines_are_rewritten_through_the_target_map() {
        let target = gc_info_like();
        let including = "ENVIRONMENT\n===========\n\n.. include:: ../gc_info.rst\n   :start-line: 305\n   :end-line: 309\n\nSEE ALSO\n========\n";
        let map = map_of(&target);
        let mut asked = Vec::new();
        let out = rewrite_line_options(including, |path| {
            asked.push(path.to_string());
            Some(map.clone())
        });
        assert_eq!(asked, vec!["../gc_info.rst"]);
        assert_eq!(
            out,
            format!(
                "ENVIRONMENT\n===========\n\n.. include:: ../gc_info.rst\n   :start-line: {}\n   :end-line: {}\n\nSEE ALSO\n========\n",
                map.map(305),
                map.map(309)
            )
        );
        assert_eq!(map.map(305), 155);
        assert_eq!(map.map(309), 158);
    }

    #[test]
    fn an_include_without_line_options_is_untouched() {
        let including = ".. include:: ../gc_info.rst\n   :literal:\n\nText.\n";
        let mut asked = 0;
        let out = rewrite_line_options(including, |_| {
            asked += 1;
            Some(map_of("A.\n"))
        });
        assert_eq!(out, including);
        assert_eq!(asked, 0);
    }

    #[test]
    fn an_unresolved_target_leaves_the_directive_alone() {
        let including = ".. include:: ../elsewhere.txt\n   :start-line: 12\n   :end-line: 20\n";
        assert_eq!(rewrite_line_options(including, |_| None), including);
    }

    #[test]
    fn an_indented_directive_and_other_options_survive() {
        let including = ".. note::\n\n   .. Include:: notes.rst\n      :encoding: utf-8\n      :start-line:   3\n      :tab-width: 3\n   Trailing.\n:start-line: 4\n";
        let map = map_of("One\ntwo.\n\nThree.\n\nFour.\n");
        assert_eq!(map.map(3), 2);
        assert_eq!(map.map(4), 3);
        let out = rewrite_line_options(including, |path| {
            assert_eq!(path, "notes.rst");
            Some(map.clone())
        });
        assert_eq!(
            out,
            ".. note::\n\n   .. Include:: notes.rst\n      :encoding: utf-8\n      :start-line:   2\n      :tab-width: 3\n   Trailing.\n:start-line: 4\n"
        );
    }

    #[test]
    fn a_non_numeric_value_is_left_as_written() {
        let including = ".. include:: a.rst\n   :start-line: seven\n";
        let out = rewrite_line_options(including, |_| Some(map_of("A.\n")));
        assert_eq!(out, including);
    }
}
