use crate::model::{Document, SegmentId};
use std::collections::HashMap;

pub type TranslationMap = HashMap<SegmentId, String>;

pub trait DocumentParser {
    fn parse(&self, source: &str) -> Document;
    fn reconstruct(&self, document: &Document, translations: &TranslationMap) -> String;
}
