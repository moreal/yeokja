use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SegmentId(pub String);

impl SegmentId {
    pub fn new(section: usize, block: usize, seg: usize) -> Self {
        Self(format!("section:{section}/block:{block}/seg:{seg}"))
    }

    pub fn position(&self) -> Option<(usize, usize, usize)> {
        let parts: Vec<&str> = self.0.split('/').collect();
        if parts.len() != 3 {
            return None;
        }
        let section = parts[0].strip_prefix("section:")?.parse().ok()?;
        let block = parts[1].strip_prefix("block:")?.parse().ok()?;
        let seg = parts[2].strip_prefix("seg:")?.parse().ok()?;
        Some((section, block, seg))
    }

    /// Compute a flat index for distance comparison during reconciliation.
    /// Invariant: section < 1000, block < 1000, seg < 1000.
    pub fn flat_index(&self) -> usize {
        self.position()
            .map(|(s, b, seg)| s * 1_000_000 + b * 1_000 + seg)
            .unwrap_or(0) // Safe: only used for distance comparison in reconciliation
    }
}

impl fmt::Display for SegmentId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BlockType {
    Heading,
    Paragraph,
    ListItem,
    CodeBlock,
    BlockQuote,
    ThematicBreak,
    Table,
    HtmlBlock,
}

impl BlockType {
    pub fn is_translatable(&self) -> bool {
        !matches!(self, BlockType::CodeBlock | BlockType::ThematicBreak | BlockType::HtmlBlock)
    }
}

#[derive(Debug, Clone)]
pub struct Segment {
    pub id: SegmentId,
    pub source: String,
    pub source_hash: u64,
    pub block_type: BlockType,
}

/// Where a block sits in the document's structure, beyond its type.
///
/// Parsers record this; they do not decide what it means. Selection rules are
/// applied later against the role, keeping parsing independent of config.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum BlockRole {
    #[default]
    None,
    /// A section title written on two lines, with the byte range of its
    /// underline. The underline's length identifies the title, so it has to
    /// follow the translation instead of staying verbatim.
    SetextTitle { underline: std::ops::Range<usize> },
    /// One cell of a `|===` table.
    TableCell {
        /// Index of the table within its document, so cells of two adjacent
        /// tables are never read as one.
        table: usize,
        /// 0-based column, accounting for cell spans.
        column: usize,
        /// This cell is in the first row, which names the columns. Such cells
        /// are never excluded by a selection rule.
        label_row: bool,
        /// Text of this cell's column in the first row. Absent on the label row
        /// itself, and on columns the first row left unnamed.
        header: Option<String>,
    },
}

#[derive(Debug, Clone)]
pub struct Block {
    pub block_type: BlockType,
    pub segments: Vec<Segment>,
    pub raw_content: String,
    pub heading_level: Option<u8>,
    /// Byte range of this block's translatable content within `Document::source`.
    /// Parsers that support span-based reconstruction set this; `None` means the
    /// block is reconstructed by the parser's own rendering logic.
    pub span: Option<std::ops::Range<usize>>,
    /// Structural position, for rules to select against.
    pub role: BlockRole,
    /// Whether this block's segments are offered for translation. Parsers seed
    /// it from `block_type`; selection rules may clear it. Cleared blocks keep
    /// their source text verbatim in the output.
    pub translatable: bool,
}

#[derive(Debug, Clone)]
pub struct Section {
    pub blocks: Vec<Block>,
}

#[derive(Debug, Clone)]
pub struct Document {
    pub sections: Vec<Section>,
    /// Original source text this document was parsed from.
    /// Used by span-based reconstruction to preserve untranslated regions verbatim.
    pub source: String,
}

impl Document {
    pub fn all_segments(&self) -> Vec<&Segment> {
        self.sections
            .iter()
            .flat_map(|s| s.blocks.iter())
            .flat_map(|b| b.segments.iter())
            .collect()
    }

    pub fn translatable_segments(&self) -> Vec<&Segment> {
        self.sections
            .iter()
            .flat_map(|s| s.blocks.iter())
            .filter(|b| b.translatable)
            .flat_map(|b| b.segments.iter())
            .collect()
    }
}
