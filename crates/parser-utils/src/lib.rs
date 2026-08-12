mod sentence;

pub use sentence::split_sentences;

use yeokja_core::hash::content_hash;
use yeokja_core::model::*;
use yeokja_core::parser::TranslationMap;

/// Helper to create segments from text using sentence splitting.
pub fn make_segments(text: &str, block_type: BlockType, section_idx: usize, block_idx: usize) -> Vec<Segment> {
    split_sentences(text)
        .into_iter()
        .enumerate()
        .map(|(seg_i, sent)| {
            let hash = content_hash(&sent);
            Segment {
                id: SegmentId::new(section_idx, block_idx, seg_i),
                source: sent,
                source_hash: hash,
                block_type,
            }
        })
        .collect()
}

/// Normalize a raw inline text span into a single line.
/// Continuation lines lose their leading blockquote markers (`>`) and indentation,
/// and line breaks collapse into single spaces, so segment text reads as prose.
pub fn normalize_inline_text(raw: &str) -> String {
    let mut lines = raw.lines();
    let mut out = String::new();
    if let Some(first) = lines.next() {
        out.push_str(first.trim_end());
    }
    for line in lines {
        let stripped = line
            .trim_start_matches(|c: char| c == '>' || c.is_whitespace())
            .trim_end();
        if stripped.is_empty() {
            continue;
        }
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(stripped);
    }
    out
}

/// Join segments using their translations (or source text as fallback), separated by spaces.
pub fn join_segments_with_translations(
    segments: &[Segment],
    translations: &TranslationMap,
) -> String {
    segments
        .iter()
        .enumerate()
        .map(|(i, seg)| {
            let text = translations.get(&seg.id).unwrap_or(&seg.source);
            if i > 0 {
                format!(" {text}")
            } else {
                text.to_string()
            }
        })
        .collect()
}

/// Reconstruct a document by splicing translations into the original source.
///
/// Every translatable block with a `span` and at least one translation has its
/// span replaced by the joined (translated or fallback) segments; everything
/// outside those spans — markers, delimiters, code, blank lines — is preserved
/// byte-for-byte. Shared by all span-based parsers.
pub fn splice_reconstruct(document: &Document, translations: &TranslationMap) -> String {
    let source = &document.source;
    let mut splices: Vec<(std::ops::Range<usize>, String)> = Vec::new();

    for section in &document.sections {
        for block in &section.blocks {
            let Some(span) = &block.span else { continue };
            if !block.block_type.is_translatable() || block.segments.is_empty() {
                continue;
            }
            let any_translated = block
                .segments
                .iter()
                .any(|seg| translations.contains_key(&seg.id));
            if !any_translated {
                // Keep the original raw text (including line wrapping) untouched.
                continue;
            }
            let joined = join_segments_with_translations(&block.segments, translations);
            splices.push((span.clone(), joined));
        }
    }

    splices.sort_by_key(|(range, _)| range.start);

    let mut output = String::with_capacity(source.len());
    let mut pos = 0usize;
    for (range, text) in splices {
        if range.start < pos {
            // Overlapping spans should not happen; skip defensively.
            continue;
        }
        output.push_str(&source[pos..range.start]);
        output.push_str(&text);
        pos = range.end;
    }
    output.push_str(&source[pos..]);
    output
}
