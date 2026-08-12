//! `yeokja inspect` — show the tables in a document, how the current rules
//! treat them, and a rule you can paste into `yeokja.toml`.
//!
//! Selection rules are written by hand on purpose, so the tool's job is to make
//! the targets discoverable rather than to guess them.

use anyhow::Result;
use std::path::Path;
use yeokja_core::model::Block;
use yeokja_core::project::ProjectContext;
use yeokja_core::select::{apply_table_rules, rules_for, table_groups};
use yeokja_translate::orchestrator::collect_files;

/// Longest sample cell text shown per column.
const SAMPLE_WIDTH: usize = 44;
/// How many sample cells to show per column.
const SAMPLES: usize = 3;

pub fn run(path: &str) -> Result<()> {
    let ctx = ProjectContext::load()?;
    let parser_factory = super::parser_factory();
    let files = collect_files(Path::new(path), &ctx.config)?;

    let mut tables_seen = 0usize;
    for file_path in &files {
        let parser = parser_factory(file_path, &ctx.config);
        let source = std::fs::read_to_string(file_path)?;
        // Report the document as the translator will see it, rules included.
        let mut doc = parser.parse(&source);
        apply_table_rules(&mut doc, &ctx.config.tables, file_path);
        let doc = doc;
        let applicable = rules_for(&ctx.config.tables, file_path);

        let mut printed_file = false;
        let mut index_in_file = 0usize;

        for section in &doc.sections {
            for group in table_groups(&section.blocks) {
                if group.body.is_empty() {
                    continue;
                }
                index_in_file += 1;
                tables_seen += 1;
                if !printed_file {
                    println!("\n{}", file_path.display());
                    printed_file = true;
                }
                print_table(
                    index_in_file,
                    &group.headers,
                    &group.body,
                    &section.blocks,
                    applicable
                        .iter()
                        .find(|r| r.matches_headers(&group.headers))
                        .is_some(),
                    file_path,
                );
            }
        }
    }

    if tables_seen == 0 {
        println!("No tables found in {path}.");
    } else {
        println!(
            "\n{tables_seen} table(s). Paste a rule into yeokja.toml to choose which columns are translated."
        );
    }
    Ok(())
}

fn print_table(
    index: usize,
    headers: &[String],
    body: &[(usize, usize)],
    blocks: &[Block],
    matched: bool,
    file: &Path,
) {
    let width = headers
        .len()
        .max(body.iter().map(|(_, c)| c + 1).max().unwrap_or(0));

    println!(
        "\n  Table {index}{}",
        if matched {
            "  (a rule matches this table)"
        } else {
            ""
        }
    );

    for column in 0..width {
        let cells: Vec<&Block> = body
            .iter()
            .filter(|(_, c)| *c == column)
            .map(|(i, _)| &blocks[*i])
            .collect();
        if cells.is_empty() {
            continue;
        }
        let header = headers.get(column).map(String::as_str).unwrap_or("");
        let excluded = cells.iter().filter(|b| !b.translatable).count();
        let state = if excluded == cells.len() {
            "kept as-is"
        } else if excluded > 0 {
            "partly translated"
        } else {
            "translated"
        };

        let samples: Vec<String> = cells
            .iter()
            .take(SAMPLES)
            .map(|b| truncate(b.raw_content.trim()))
            .collect();

        println!(
            "    [{column}] {:<16} {:>4} cells  {:<18} {}",
            if header.is_empty() { "(no header)" } else { header },
            cells.len(),
            state,
            samples.join(" · ")
        );
    }

    if !matched && !headers.is_empty() {
        print_suggestion(headers, file);
    }
}

/// A ready-to-paste rule translating only the widest-looking column, which is
/// the usual shape for a reference table.
fn print_suggestion(headers: &[String], file: &Path) {
    let named: Vec<&String> = headers.iter().filter(|h| !h.is_empty()).collect();
    if named.len() < 2 {
        return;
    }
    let quoted: Vec<String> = named.iter().map(|h| format!("{h:?}")).collect();
    let last = named.last().unwrap();
    println!();
    println!("    [[tables]]");
    println!("    files = {:?}", file.display().to_string());
    println!("    headers = [{}]", quoted.join(", "));
    println!("    translate = [{last:?}]");
}

fn truncate(text: &str) -> String {
    let one_line = text.replace('\n', " ");
    match one_line.char_indices().nth(SAMPLE_WIDTH) {
        Some((cut, _)) => format!("{}…", &one_line[..cut]),
        None => one_line,
    }
}
