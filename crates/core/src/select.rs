//! Applying selection rules to a parsed document.
//!
//! Parsers record structure ([`BlockRole`]); this module decides what that
//! structure means for a given project config. Keeping the two apart means a
//! parser never reads config, and a rule change never requires reparsing logic.

use crate::config::TableRule;
use crate::model::{BlockRole, Document};
use std::path::Path;

/// Clear the `translatable` flag on blocks excluded by `rules`.
///
/// Only ever narrows: a block already excluded by its type stays excluded, and
/// a document with no matching rules is left untouched. Returns how many blocks
/// were excluded, for reporting.
pub fn apply_table_rules(document: &mut Document, rules: &[TableRule], file: &Path) -> usize {
    let applicable: Vec<&TableRule> = rules
        .iter()
        .filter(|r| r.files.as_deref().is_none_or(|g| path_matches(g, file)))
        .collect();
    if applicable.is_empty() {
        return 0;
    }

    // A cell's header text identifies its column, but matching a *table*
    // needs the whole header row. Cells are emitted in document order, so the
    // headers of the table currently being walked are the most recent run of
    // header-row cells (those whose own `header` is None).
    let mut excluded = 0;
    for section in &mut document.sections {
        let mut headers: Vec<String> = Vec::new();
        let mut in_header_row = false;

        for block in &mut section.blocks {
            let BlockRole::TableCell { column, header } = &block.role else {
                // Any non-cell block ends the table we were walking.
                headers.clear();
                in_header_row = false;
                continue;
            };

            match header {
                // Header row: collect the column names.
                None => {
                    if !in_header_row {
                        headers.clear();
                        in_header_row = true;
                    }
                    headers.push(block.raw_content.trim().to_string());
                }
                // Body cell: decide using the header row collected above.
                Some(h) => {
                    in_header_row = false;
                    let Some(rule) = applicable.iter().find(|r| r.matches_headers(&headers)) else {
                        continue;
                    };
                    if !rule.translates(*column, Some(h.as_str())) && block.translatable {
                        block.translatable = false;
                        excluded += 1;
                    }
                }
            }
        }
    }
    excluded
}

/// Glob match against the path as written, and against its file name, so both
/// `chapters/*.asciidoc` and `*.asciidoc` behave as expected.
fn path_matches(pattern: &str, file: &Path) -> bool {
    let Ok(glob) = glob::Pattern::new(pattern) else {
        tracing::warn!(pattern, "Invalid files glob in table rule; rule skipped");
        return false;
    };
    let normalized = file.strip_prefix("./").unwrap_or(file);
    glob.matches_path(normalized)
        || file
            .file_name()
            .is_some_and(|n| glob.matches_path(Path::new(n)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ColumnRef;
    use crate::model::{Block, BlockType, Section};

    fn cell(column: usize, header: Option<&str>, text: &str) -> Block {
        Block {
            block_type: BlockType::Table,
            segments: Vec::new(),
            raw_content: text.to_string(),
            heading_level: None,
            span: Some(0..text.len()),
            translatable: true,
            role: BlockRole::TableCell {
                column,
                header: header.map(str::to_string),
            },
        }
    }

    /// Header row followed by one body row, matching the BEAM instruction table.
    fn instruction_table() -> Document {
        Document {
            sections: vec![Section {
                blocks: vec![
                    cell(0, None, "Instruction"),
                    cell(1, None, "Arguments"),
                    cell(2, None, "Explanation"),
                    cell(0, Some("Instruction"), "allocate"),
                    cell(1, Some("Arguments"), "t t"),
                    cell(2, Some("Explanation"), "Allocate stack words"),
                ],
            }],
            source: String::new(),
        }
    }

    fn rule(translate: Vec<ColumnRef>, skip: Vec<ColumnRef>) -> TableRule {
        TableRule {
            files: None,
            headers: vec![
                "Instruction".to_string(),
                "Arguments".to_string(),
                "Explanation".to_string(),
            ],
            translate,
            skip,
        }
    }

    /// Body cells that survived, by their text.
    fn kept(doc: &Document) -> Vec<&str> {
        doc.sections
            .iter()
            .flat_map(|s| s.blocks.iter())
            .filter(|b| b.translatable)
            .filter(|b| matches!(&b.role, BlockRole::TableCell { header, .. } if header.is_some()))
            .map(|b| b.raw_content.as_str())
            .collect()
    }

    #[test]
    fn translate_list_keeps_only_named_columns() {
        let mut doc = instruction_table();
        let rules = vec![rule(vec![ColumnRef::Header("Explanation".into())], vec![])];
        assert_eq!(apply_table_rules(&mut doc, &rules, Path::new("a.adoc")), 2);
        assert_eq!(kept(&doc), vec!["Allocate stack words"]);
    }

    #[test]
    fn skip_list_removes_only_named_columns() {
        let mut doc = instruction_table();
        let rules = vec![rule(
            vec![],
            vec![ColumnRef::Header("Arguments".into()), ColumnRef::Index(0)],
        )];
        assert_eq!(apply_table_rules(&mut doc, &rules, Path::new("a.adoc")), 2);
        assert_eq!(kept(&doc), vec!["Allocate stack words"]);
    }

    #[test]
    fn header_row_is_never_excluded() {
        let mut doc = instruction_table();
        let rules = vec![rule(vec![ColumnRef::Header("Explanation".into())], vec![])];
        apply_table_rules(&mut doc, &rules, Path::new("a.adoc"));
        let headers: Vec<&str> = doc.sections[0]
            .blocks
            .iter()
            .filter(|b| matches!(&b.role, BlockRole::TableCell { header: None, .. }))
            .filter(|b| b.translatable)
            .map(|b| b.raw_content.as_str())
            .collect();
        assert_eq!(headers, vec!["Instruction", "Arguments", "Explanation"]);
    }

    #[test]
    fn non_matching_headers_leave_table_alone() {
        let mut doc = instruction_table();
        let mut r = rule(vec![ColumnRef::Header("Explanation".into())], vec![]);
        r.headers = vec!["Term".to_string(), "Definition".to_string()];
        assert_eq!(apply_table_rules(&mut doc, &[r], Path::new("a.adoc")), 0);
        assert_eq!(kept(&doc).len(), 3);
    }

    #[test]
    fn files_glob_scopes_the_rule() {
        let rules = vec![TableRule {
            files: Some("chapters/ap-*.asciidoc".to_string()),
            ..rule(vec![ColumnRef::Header("Explanation".into())], vec![])
        }];

        let mut matching = instruction_table();
        apply_table_rules(
            &mut matching,
            &rules,
            Path::new("chapters/ap-beam_instructions.asciidoc"),
        );
        assert_eq!(kept(&matching), vec!["Allocate stack words"]);

        let mut other = instruction_table();
        apply_table_rules(&mut other, &rules, Path::new("chapters/gc.asciidoc"));
        assert_eq!(kept(&other).len(), 3);
    }

    #[test]
    fn extra_columns_do_not_break_the_match() {
        let mut doc = instruction_table();
        doc.sections[0]
            .blocks
            .insert(3, cell(3, None, "Since"));
        let rules = vec![rule(vec![ColumnRef::Header("Explanation".into())], vec![])];
        assert_eq!(apply_table_rules(&mut doc, &rules, Path::new("a.adoc")), 2);
    }
}
