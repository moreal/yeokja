use crate::glossary::Glossary;
use crate::state::SegmentState;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SegmentStatus {
    Translated,
    Pending,
    Stale,
    ContextChanged,
    GlossaryStale,
}

pub fn compute_status(
    segment: &SegmentState,
    current_source_hash: u64,
    current_context_hash: u64,
    glossary: &Glossary,
) -> SegmentStatus {
    if segment.translation.is_none() {
        return SegmentStatus::Pending;
    }
    if segment.source_hash != current_source_hash {
        return SegmentStatus::Stale;
    }
    if glossary.is_snapshot_stale(&segment.glossary_snapshot, &segment.source) {
        return SegmentStatus::GlossaryStale;
    }
    if segment.context_hash != current_context_hash {
        return SegmentStatus::ContextChanged;
    }
    SegmentStatus::Translated
}

impl SegmentStatus {
    pub fn needs_translation(&self) -> bool {
        matches!(
            self,
            SegmentStatus::Pending
                | SegmentStatus::Stale
                | SegmentStatus::GlossaryStale
                | SegmentStatus::ContextChanged
        )
    }

    pub fn is_high_priority(&self) -> bool {
        matches!(
            self,
            SegmentStatus::Pending | SegmentStatus::Stale | SegmentStatus::GlossaryStale
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::SegmentId;
    use std::collections::HashMap;

    fn make_segment_state(source_hash: u64, context_hash: u64, translated: bool) -> SegmentState {
        SegmentState {
            id: SegmentId::new(0, 0, 0),
            source: "test".to_string(),
            source_hash,
            context_hash,
            translation: if translated { Some("번역".to_string()) } else { None },
            glossary_snapshot: HashMap::new(),
            translated_at: None,
            issues: Vec::new(),
        }
    }

    #[test]
    fn pending_when_no_translation() {
        let seg = make_segment_state(100, 200, false);
        let status = compute_status(&seg, 100, 200, &Glossary::empty());
        assert_eq!(status, SegmentStatus::Pending);
    }

    #[test]
    fn translated_when_all_match() {
        let seg = make_segment_state(100, 200, true);
        let status = compute_status(&seg, 100, 200, &Glossary::empty());
        assert_eq!(status, SegmentStatus::Translated);
    }

    #[test]
    fn stale_when_source_hash_differs() {
        let seg = make_segment_state(100, 200, true);
        let status = compute_status(&seg, 999, 200, &Glossary::empty());
        assert_eq!(status, SegmentStatus::Stale);
    }

    #[test]
    fn context_changed_when_context_hash_differs() {
        let seg = make_segment_state(100, 200, true);
        let status = compute_status(&seg, 100, 999, &Glossary::empty());
        assert_eq!(status, SegmentStatus::ContextChanged);
    }

    #[test]
    fn glossary_stale_when_snapshot_outdated() {
        let glossary = Glossary::from_toml(r#"
[terms.test]
translation = "테스트"
"#).unwrap();
        let mut seg = make_segment_state(100, 200, true);
        seg.source = "test something".to_string();
        seg.glossary_snapshot.insert("test".to_string(), "이전번역".to_string());
        let status = compute_status(&seg, 100, 200, &glossary);
        assert_eq!(status, SegmentStatus::GlossaryStale);
    }

    #[test]
    fn needs_translation_for_actionable_statuses() {
        assert!(SegmentStatus::Pending.needs_translation());
        assert!(SegmentStatus::Stale.needs_translation());
        assert!(SegmentStatus::GlossaryStale.needs_translation());
        assert!(SegmentStatus::ContextChanged.needs_translation());
        assert!(!SegmentStatus::Translated.needs_translation());
    }

    #[test]
    fn high_priority_excludes_context_changed() {
        assert!(SegmentStatus::Pending.is_high_priority());
        assert!(SegmentStatus::Stale.is_high_priority());
        assert!(SegmentStatus::GlossaryStale.is_high_priority());
        assert!(!SegmentStatus::ContextChanged.is_high_priority());
        assert!(!SegmentStatus::Translated.is_high_priority());
    }
}
