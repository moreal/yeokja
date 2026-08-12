use crate::change::{compute_status, SegmentStatus};
use crate::glossary::Glossary;
use crate::hash::context_hash;
use crate::model::{Document, Segment};
use crate::state::{SegmentState, StateFile};
use std::collections::HashMap;

pub struct ReconcileResult {
    pub segments: Vec<SegmentState>,
}

pub struct ReconciledSegment {
    pub state: SegmentState,
    pub status: SegmentStatus,
    pub context_hash: u64,
}

/// Reconcile a document against existing state and compute status for each segment.
pub fn reconcile_with_status(
    document: &Document,
    existing: &StateFile,
    glossary: &Glossary,
) -> Vec<ReconciledSegment> {
    let result = reconcile(document, existing);
    let segments = document.translatable_segments();
    let hashes: Vec<u64> = segments.iter().map(|s| s.source_hash).collect();

    result
        .segments
        .into_iter()
        .enumerate()
        .map(|(i, seg_state)| {
            let prev = if i > 0 { Some(hashes[i - 1]) } else { None };
            let next = hashes.get(i + 1).copied();
            let ctx = context_hash(prev, next);
            let status = compute_status(&seg_state, seg_state.source_hash, ctx, glossary);
            ReconciledSegment {
                state: seg_state,
                status,
                context_hash: ctx,
            }
        })
        .collect()
}

pub fn reconcile(document: &Document, existing: &StateFile) -> ReconcileResult {
    let new_segments = document.translatable_segments();
    tracing::trace!(segments = new_segments.len(), existing = existing.segments.len(), "Reconciling");
    let new_with_context: Vec<(&Segment, u64)> = {
        let hashes: Vec<u64> = new_segments.iter().map(|s| s.source_hash).collect();
        new_segments
            .iter()
            .enumerate()
            .map(|(i, seg)| {
                let prev = if i > 0 { Some(hashes[i - 1]) } else { None };
                let next = hashes.get(i + 1).copied();
                let ctx = context_hash(prev, next);
                (*seg, ctx)
            })
            .collect()
    };

    let mut existing_by_hash: HashMap<u64, Vec<&SegmentState>> = HashMap::new();
    for seg in &existing.segments {
        existing_by_hash.entry(seg.source_hash).or_default().push(seg);
    }

    let mut matched_existing: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut result_segments = Vec::new();

    for (new_seg, ctx_hash) in &new_with_context {
        let matched = existing_by_hash
            .get(&new_seg.source_hash)
            .and_then(|candidates| {
                let new_idx = new_seg.id.flat_index();
                candidates
                    .iter()
                    .filter(|c| !matched_existing.contains(&c.id.0))
                    .min_by_key(|c| {
                        let old_idx = c.id.flat_index();
                        (new_idx as isize - old_idx as isize).unsigned_abs()
                    })
            });

        match matched {
            Some(old) => {
                matched_existing.insert(old.id.0.clone());
                result_segments.push(SegmentState {
                    id: new_seg.id.clone(),
                    source: new_seg.source.clone(),
                    source_hash: new_seg.source_hash,
                    // Preserve the old context_hash so compute_status can detect ContextChanged
                    context_hash: old.context_hash,
                    translation: old.translation.clone(),
                    glossary_snapshot: old.glossary_snapshot.clone(),
                    translated_at: old.translated_at,
                    issues: old.issues.clone(),
                });
            }
            None => {
                result_segments.push(SegmentState {
                    id: new_seg.id.clone(),
                    source: new_seg.source.clone(),
                    source_hash: new_seg.source_hash,
                    context_hash: *ctx_hash,
                    translation: None,
                    glossary_snapshot: HashMap::new(),
                    translated_at: None,
                    issues: Vec::new(),
                });
            }
        }
    }

    ReconcileResult { segments: result_segments }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hash::content_hash;
    use crate::model::*;
    use chrono::Utc;

    fn make_segment(section: usize, block: usize, seg: usize, text: &str) -> Segment {
        Segment {
            id: SegmentId::new(section, block, seg),
            source: text.to_string(),
            source_hash: content_hash(text),
            block_type: BlockType::Paragraph,
        }
    }

    fn make_document(segments: Vec<Segment>) -> Document {
        Document {
            sections: vec![Section {
                blocks: vec![Block {
                    block_type: BlockType::Paragraph,
                    segments,
                    raw_content: String::new(),
                    heading_level: None,
                    span: None,
                }],
            }],
            source: String::new(),
        }
    }

    fn make_state(entries: Vec<(&str, &str)>) -> StateFile {
        let mut state = StateFile::new(0);
        for (i, (source, translation)) in entries.iter().enumerate() {
            state.segments.push(SegmentState {
                id: SegmentId::new(0, 0, i),
                source: source.to_string(),
                source_hash: content_hash(source),
                context_hash: 0,
                translation: Some(translation.to_string()),
                glossary_snapshot: HashMap::new(),
                translated_at: Some(Utc::now()),
                issues: Vec::new(),
            });
        }
        state
    }

    #[test]
    fn new_segments_get_no_translation() {
        let doc = make_document(vec![make_segment(0, 0, 0, "Hello.")]);
        let state = StateFile::new(0);
        let result = reconcile(&doc, &state);
        assert_eq!(result.segments.len(), 1);
        assert!(result.segments[0].translation.is_none());
    }

    #[test]
    fn unchanged_segments_keep_translation() {
        let doc = make_document(vec![make_segment(0, 0, 0, "Hello.")]);
        let state = make_state(vec![("Hello.", "안녕하세요.")]);
        let result = reconcile(&doc, &state);
        assert_eq!(result.segments[0].translation.as_deref(), Some("안녕하세요."));
    }

    #[test]
    fn moved_segment_keeps_translation() {
        let doc = make_document(vec![
            make_segment(0, 0, 0, "New sentence."),
            make_segment(0, 0, 1, "World."),
        ]);
        let state = make_state(vec![("World.", "세계.")]);
        let result = reconcile(&doc, &state);
        assert_eq!(result.segments.len(), 2);
        assert!(result.segments[0].translation.is_none());
        assert_eq!(result.segments[1].translation.as_deref(), Some("세계."));
    }

    #[test]
    fn deleted_segments_are_dropped() {
        let doc = make_document(vec![make_segment(0, 0, 0, "Hello.")]);
        let state = make_state(vec![("Hello.", "안녕."), ("Goodbye.", "잘가.")]);
        let result = reconcile(&doc, &state);
        assert_eq!(result.segments.len(), 1);
        assert_eq!(result.segments[0].source, "Hello.");
    }

    #[test]
    fn duplicate_segments_matched_by_position() {
        let doc = make_document(vec![
            make_segment(0, 0, 0, "Same."),
            make_segment(0, 0, 1, "Same."),
        ]);
        let state = make_state(vec![("Same.", "같다1."), ("Same.", "같다2.")]);
        let result = reconcile(&doc, &state);
        assert_eq!(result.segments.len(), 2);
        assert!(result.segments[0].translation.is_some());
        assert!(result.segments[1].translation.is_some());
        assert_ne!(result.segments[0].translation, result.segments[1].translation);
    }
}
