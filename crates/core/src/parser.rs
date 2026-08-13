use crate::model::{Document, SegmentId};
use std::collections::HashMap;

pub type TranslationMap = HashMap<SegmentId, String>;

/// The markup language a parser reads.
///
/// Checks that run after parsing — is this text still valid markup? — need to
/// know which syntax the text will be read back as, and the two differ on
/// rules that matter for a translation. A parser is the one thing that always
/// knows, so it says.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Markup {
    Markdown,
    Asciidoc,
}

pub trait DocumentParser: Send + Sync {
    fn parse(&self, source: &str) -> Document;
    fn reconstruct(&self, document: &Document, translations: &TranslationMap) -> String;

    /// The markup this parser reads.
    fn markup(&self) -> Markup;
}
