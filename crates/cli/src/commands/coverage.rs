//! `yeokja coverage` — show which parts of a document the parser passed over.
//!
//! A parser that does not recognise a construct fails silently: it emits no
//! block, the text inside is never offered for translation, and progress still
//! reads as complete because those lines were never counted. This puts the
//! passed-over runs on screen so the omission has somewhere to show up.

use anyhow::Result;
use std::path::Path;
use yeokja_core::coverage::{Coverage, coverage};
use yeokja_core::project::ProjectContext;
use yeokja_translate::orchestrator::collect_files;

/// Gaps listed per file before the rest are summarised.
const GAPS_SHOWN: usize = 5;

pub fn run(path: &str, min_lines: usize) -> Result<()> {
    let ctx = ProjectContext::load()?;
    let parser_factory = super::parser_factory();
    let files = collect_files(Path::new(path), &ctx.config)?;

    let mut word_lines = 0usize;
    let mut offered_lines = 0usize;
    let mut files_with_gaps = 0usize;

    for file_path in &files {
        let parser = parser_factory(file_path, &ctx.config);
        let source = std::fs::read_to_string(file_path)?;
        // Selection rules are a deliberate choice, not a gap, so they are not
        // applied here — this measures the parse alone.
        let document = parser
            .parse_checked(&source)
            .map_err(|error| anyhow::anyhow!("{}: {error}", file_path.display()))?;
        let result = coverage(&document);

        word_lines += result.word_lines;
        offered_lines += result.offered_lines;

        let reportable: Vec<_> = result
            .gaps
            .iter()
            .filter(|gap| gap.lines >= min_lines)
            .collect();
        if reportable.is_empty() {
            continue;
        }
        files_with_gaps += 1;

        println!(
            "\n{}   {:.0}% of {} lines with text",
            file_path.display(),
            result.ratio() * 100.0,
            result.word_lines
        );
        for gap in reportable.iter().take(GAPS_SHOWN) {
            let range = format!("L{}-{}", gap.first_line, gap.last_line);
            println!("    {range:<16}{:>5} lines   {}", gap.lines, gap.preview);
        }
        if reportable.len() > GAPS_SHOWN {
            println!("    … {} more run(s) of {min_lines}+ lines", reportable.len() - GAPS_SHOWN);
        }
    }

    let total = Coverage {
        word_lines,
        offered_lines,
        gaps: Vec::new(),
    };
    println!(
        "\n{} file(s) · {} lines with text · {:.0}% offered for translation",
        files.len(),
        word_lines,
        total.ratio() * 100.0
    );
    if files_with_gaps == 0 {
        println!("No file has a run of {min_lines} or more lines the parser passed over.");
    } else {
        println!(
            "{files_with_gaps} file(s) have a run of {min_lines} or more lines the parser passed over."
        );
        println!(
            "\nCode blocks and comments are listed here too — they are not offered on purpose.\n\
             What is worth chasing is a long run whose preview reads as prose."
        );
    }
    Ok(())
}
