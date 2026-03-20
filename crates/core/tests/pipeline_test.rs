// crates/core/tests/pipeline_test.rs
use std::collections::HashMap;
use yeokja_core::change::{compute_status, SegmentStatus};
use yeokja_core::glossary::Glossary;
use yeokja_core::hash::{content_hash, context_hash};
use yeokja_core::model::SegmentId;
use yeokja_core::reconcile::reconcile;
use yeokja_core::state::{SegmentState, StateFile};
use chrono::Utc;

// We can't import yeokja_parser_markdown from core's tests, so we'll build
// a Document manually for this test.
use yeokja_core::model::*;

fn make_document(texts: &[&str]) -> Document {
    let segments: Vec<Segment> = texts
        .iter()
        .enumerate()
        .map(|(i, text)| Segment {
            id: SegmentId::new(0, 0, i),
            source: text.to_string(),
            source_hash: content_hash(text),
            block_type: BlockType::Paragraph,
        })
        .collect();

    Document {
        sections: vec![Section {
            blocks: vec![Block {
                block_type: BlockType::Paragraph,
                segments,
                raw_content: texts.join(" "),
                heading_level: None,
            }],
        }],
    }
}

#[test]
fn full_pipeline_new_document() {
    // 1. Parse a new document
    let doc = make_document(&["Hello world.", "Goodbye world."]);
    let state = StateFile::new(0);
    let glossary = Glossary::empty();

    // 2. Reconcile — all segments should be new
    let result = reconcile(&doc, &state);
    assert_eq!(result.segments.len(), 2);

    // 3. Check status — all should be Pending
    let segs = doc.translatable_segments();
    let hashes: Vec<u64> = segs.iter().map(|s| s.source_hash).collect();
    for (i, seg_state) in result.segments.iter().enumerate() {
        let prev = if i > 0 { Some(hashes[i - 1]) } else { None };
        let next = hashes.get(i + 1).copied();
        let ctx = context_hash(prev, next);
        let status = compute_status(seg_state, seg_state.source_hash, ctx, &glossary);
        assert_eq!(status, SegmentStatus::Pending);
    }
}

#[test]
fn full_pipeline_incremental_update() {
    let glossary = Glossary::empty();

    // 1. First run: translate everything
    let doc1 = make_document(&["Hello.", "World."]);
    let state1 = StateFile::new(0);
    let result1 = reconcile(&doc1, &state1);

    // Simulate translation
    let mut state2 = StateFile::new(content_hash("Hello. World."));
    let segs1 = doc1.translatable_segments();
    let hashes1: Vec<u64> = segs1.iter().map(|s| s.source_hash).collect();
    for (i, seg) in result1.segments.iter().enumerate() {
        let prev = if i > 0 { Some(hashes1[i - 1]) } else { None };
        let next = hashes1.get(i + 1).copied();
        let ctx = context_hash(prev, next);
        state2.segments.push(SegmentState {
            id: seg.id.clone(),
            source: seg.source.clone(),
            source_hash: seg.source_hash,
            context_hash: ctx,
            translation: Some(format!("translated_{}", seg.source)),
            glossary_snapshot: HashMap::new(),
            translated_at: Some(Utc::now()),
            issues: Vec::new(),
        });
    }

    // 2. Second run: change first sentence only
    let doc2 = make_document(&["Hi.", "World."]);
    let result2 = reconcile(&doc2, &state2);

    let segs2 = doc2.translatable_segments();
    let hashes2: Vec<u64> = segs2.iter().map(|s| s.source_hash).collect();

    // First segment: new (no match), should be Pending
    let ctx0 = context_hash(None, Some(hashes2[1]));
    let status0 = compute_status(&result2.segments[0], segs2[0].source_hash, ctx0, &glossary);
    assert_eq!(status0, SegmentStatus::Pending);

    // Second segment: unchanged, should be Translated (or ContextChanged since prev changed)
    let ctx1 = context_hash(Some(hashes2[0]), None);
    let status1 = compute_status(&result2.segments[1], segs2[1].source_hash, ctx1, &glossary);
    // Context changed because the previous segment changed
    assert_eq!(status1, SegmentStatus::ContextChanged);
    // Needs translation but at low priority
    assert!(status1.needs_translation());
    assert!(!status1.is_high_priority());
}

#[test]
fn full_pipeline_glossary_stale() {
    // 1. Translate with empty glossary
    let doc = make_document(&["The repository is ready."]);
    let state = StateFile::new(0);
    let result = reconcile(&doc, &state);

    let segs = doc.translatable_segments();
    let ctx = context_hash(None, None);
    let mut state2 = StateFile::new(0);
    state2.segments.push(SegmentState {
        id: result.segments[0].id.clone(),
        source: "The repository is ready.".to_string(),
        source_hash: result.segments[0].source_hash,
        context_hash: ctx,
        translation: Some("레포지토리가 준비되었습니다.".to_string()),
        glossary_snapshot: HashMap::new(), // no glossary terms recorded
        translated_at: Some(Utc::now()),
        issues: Vec::new(),
    });

    // 2. Now add a glossary term
    let glossary = Glossary::from_toml(r#"
[terms.repository]
translation = "저장소"
"#).unwrap();

    let result2 = reconcile(&doc, &state2);
    let status = compute_status(&result2.segments[0], segs[0].source_hash, ctx, &glossary);
    assert_eq!(status, SegmentStatus::GlossaryStale);
    assert!(status.needs_translation());
}
