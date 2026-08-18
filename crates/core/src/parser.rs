use crate::model::{Document, SegmentId};
use std::collections::HashMap;

pub type TranslationMap = HashMap<SegmentId, String>;

#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct DocumentParseError(pub String);

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
    Rst,
    /// Verso manual syntax embedded in a Lean `#doc` command.
    Verso,
}

pub trait DocumentParser: Send + Sync {
    fn parse(&self, source: &str) -> Document;

    /// Parse with recoverable diagnostics.
    ///
    /// Existing in-process parsers are infallible and inherit this default.
    /// Parsers backed by generated syntax data override it so stale or missing
    /// inputs fail the operation instead of silently reducing coverage.
    fn parse_checked(&self, source: &str) -> Result<Document, DocumentParseError> {
        Ok(self.parse(source))
    }

    fn reconstruct(&self, document: &Document, translations: &TranslationMap) -> String;

    /// The markup this parser reads.
    fn markup(&self) -> Markup;
}
