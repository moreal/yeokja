//! Applying selection rules to a parsed document.
//!
//! Parsers record structure ([`BlockRole`]); this module decides what that
//! structure means for a given project config. Keeping the two apart means a
//! parser never reads config, and a rule change never requires reparsing logic.

use crate::config::TableRule;
use crate::model::{BlockRole, Document};
use std::path::Path;

/// One table found in a document: its header row, and the block indices of its
/// body cells within the section.
#[derive(Debug, Clone)]
pub struct TableGroup {
    pub headers: Vec<String>,
    /// `(block index within the section, column index)` for each body cell.
    pub body: Vec<(usize, usize)>,
}

/// Group a section's blocks into tables.
///
/// Cells name the table they came from, so grouping is exact — two tables
/// written back to back never merge, however their rows are laid out.
pub fn table_groups(blocks: &[crate::model::Block]) -> Vec<TableGroup> {
    let mut groups: Vec<(usize, TableGroup)> = Vec::new();

    for (idx, block) in blocks.iter().enumerate() {
        let BlockRole::TableCell {
            table,
            column,
            label_row,
            ..
        } = &block.role
        else {
            continue;
        };
        let position = match groups.iter().position(|(id, _)| id == table) {
            Some(position) => position,
            None => {
                groups.push((
                    *table,
                    TableGroup {
                        headers: Vec::new(),
                        body: Vec::new(),
                    },
                ));
                groups.len() - 1
            }
        };
        let group = &mut groups[position].1;
        if *label_row {
            if group.headers.len() <= *column {
                group.headers.resize(column + 1, String::new());
            }
            group.headers[*column] = block.raw_content.trim().to_string();
        } else {
            group.body.push((idx, *column));
        }
    }
    groups.into_iter().map(|(_, group)| group).collect()
}

/// Rules that apply to `file`, in declaration order.
pub fn rules_for<'a>(rules: &'a [TableRule], file: &Path) -> Vec<&'a TableRule> {
    rules
        .iter()
        .filter(|r| r.files.as_deref().is_none_or(|g| path_matches(g, file)))
        .collect()
}

/// Clear the `translatable` flag on blocks excluded by `rules`.
///
/// Only ever narrows: a block already excluded by its type stays excluded, and
/// a document with no matching rules is left untouched. Returns how many blocks
/// were excluded, for reporting.
pub fn apply_table_rules(document: &mut Document, rules: &[TableRule], file: &Path) -> usize {
    let applicable = rules_for(rules, file);
    if applicable.is_empty() {
        return 0;
    }

    let mut excluded = 0;
    for section in &mut document.sections {
        let groups = table_groups(&section.blocks);
        for group in groups {
            let Some(rule) = applicable.iter().find(|r| r.matches_headers(&group.headers)) else {
                continue;
            };
            for (idx, column) in group.body {
                let block = &mut section.blocks[idx];
                let header = match &block.role {
                    BlockRole::TableCell { header, .. } => header.clone(),
                    _ => None,
                };
                if !rule.translates(column, header.as_deref()) && block.translatable {
                    block.translatable = false;
                    excluded += 1;
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

    /// A body cell of table `table`, sitting under the named column.
    fn cell(table: usize, column: usize, header: &str, text: &str) -> Block {
        block(
            text,
            BlockRole::TableCell {
                table,
                column,
                label_row: false,
                header: Some(header.to_string()),
            },
        )
    }

    fn unlabelled_cell(table: usize, column: usize, text: &str) -> Block {
        block(
            text,
            BlockRole::TableCell {
                table,
                column,
                label_row: false,
                header: None,
            },
        )
    }

    /// A cell of table `table`'s label row.
    fn label(table: usize, column: usize, text: &str) -> Block {
        block(
            text,
            BlockRole::TableCell {
                table,
                column,
                label_row: true,
                header: None,
            },
        )
    }

    fn block(text: &str, role: BlockRole) -> Block {
        Block {
            block_type: BlockType::Table,
            segments: Vec::new(),
            raw_content: text.to_string(),
            heading_level: None,
            span: Some(0..text.len()),
            translatable: true,
            role,
        }
    }

    /// Header row followed by one body row, matching the BEAM instruction table.
    fn instruction_table() -> Document {
        Document {
            sections: vec![Section {
                blocks: vec![
                    label(1, 0, "Instruction"),
                    label(1, 1, "Arguments"),
                    label(1, 2, "Explanation"),
                    cell(1, 0, "Instruction", "allocate"),
                    cell(1, 1, "Arguments", "t t"),
                    cell(1, 2, "Explanation", "Allocate stack words"),
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
            headerless: false,
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
            .filter(|b| matches!(&b.role, BlockRole::TableCell { label_row: false, .. }))
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
            .filter(|b| matches!(&b.role, BlockRole::TableCell { label_row: true, .. }))
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
    fn table_groups_separates_consecutive_tables() {
        let blocks = vec![
            label(1, 0, "Type"),
            label(1, 1, "Explanation"),
            cell(1, 0, "Type", "c"),
            cell(1, 1, "Explanation", "A constant"),
            // A second table follows directly, with a different schema.
            label(2, 0, "Instruction"),
            label(2, 1, "Arguments"),
            cell(2, 0, "Instruction", "allocate"),
            cell(2, 1, "Arguments", "t t"),
        ];
        let groups = table_groups(&blocks);
        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0].headers, vec!["Type", "Explanation"]);
        assert_eq!(groups[1].headers, vec!["Instruction", "Arguments"]);
        assert_eq!(groups[0].body.len(), 2);
        assert_eq!(groups[1].body.len(), 2);
    }

    #[test]
    fn a_rule_applies_only_to_the_table_it_matches() {
        // Two tables in one section: only the second matches the rule, so the
        // first must come through untouched.
        let mut doc = Document {
            sections: vec![Section {
                blocks: vec![
                    label(1, 0, "Type"),
                    label(1, 1, "Explanation"),
                    cell(1, 0, "Type", "c"),
                    cell(1, 1, "Explanation", "A constant"),
                    label(2, 0, "Instruction"),
                    label(2, 1, "Arguments"),
                    label(2, 2, "Explanation"),
                    cell(2, 0, "Instruction", "allocate"),
                    cell(2, 1, "Arguments", "t t"),
                    cell(2, 2, "Explanation", "Allocate stack words"),
                ],
            }],
            source: String::new(),
        };
        let rules = vec![rule(vec![ColumnRef::Header("Explanation".into())], vec![])];
        assert_eq!(apply_table_rules(&mut doc, &rules, Path::new("a.adoc")), 2);
        assert_eq!(kept(&doc), vec!["c", "A constant", "Allocate stack words"]);
    }

    #[test]
    fn extra_columns_do_not_break_the_match() {
        let mut doc = instruction_table();
        doc.sections[0].blocks.insert(3, label(1, 3, "Since"));
        let rules = vec![rule(vec![ColumnRef::Header("Explanation".into())], vec![])];
        assert_eq!(apply_table_rules(&mut doc, &rules, Path::new("a.adoc")), 2);
    }

    #[test]
    fn headerless_rule_does_not_match_a_labelled_table() {
        let mut doc = instruction_table();
        doc.sections[0].blocks.extend([
            unlabelled_cell(2, 0, "'B'"),
            unlabelled_cell(2, 1, "unsigned_char"),
        ]);
        let rules = vec![TableRule {
            files: Some("development-tools/clinic/howto.rst".to_string()),
            headers: vec![],
            headerless: true,
            translate: vec![],
            skip: vec![ColumnRef::Index(0), ColumnRef::Index(1)],
        }];
        assert_eq!(
            apply_table_rules(
                &mut doc,
                &rules,
                Path::new("development-tools/clinic/howto.rst"),
            ),
            2,
        );
        assert_eq!(
            kept(&doc),
            vec!["allocate", "t t", "Allocate stack words"]
        );
    }
}
