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
