use anyhow::Result;
use std::path::Path;
use yeokja_core::project::ProjectContext;
use yeokja_translate::orchestrator::{collect_files, match_orphans, resolve_output_path};

/// List state files whose source is gone; with `delete`, remove them and
/// their derived outputs. Deletion is the one thing that is never automatic:
/// an orphan may be a chapter upstream will revive, and its translations are
/// the only expensive thing in the project.
pub fn run(delete: bool) -> Result<()> {
    let ctx = ProjectContext::load()?;
    let parser_factory = super::parser_factory();

    // Collect from every configured source root so rename candidates
    // anywhere in the project are seen.
    let mut files = Vec::new();
    for source in &ctx.config.sources {
        files.extend(collect_files(Path::new(&source.path), &ctx.config)?);
    }
    files.sort();
    files.dedup();

    let reports = match_orphans(&files, &ctx.config, &parser_factory)?;
    if reports.is_empty() {
        println!("No orphaned state files.");
        return Ok(());
    }

    for report in &reports {
        println!(
            "{} (source gone: {})",
            report.orphan.state_path.display(),
            report.orphan.expected_source.display()
        );
        if let Some(to) = &report.renamed_to {
            println!(
                "  looks renamed to {} — `yeokja translate` will adopt it",
                to.display()
            );
        }
    }

    if !delete {
        println!(
            "\n{} orphan(s). Re-run with --delete to remove them and their outputs.",
            reports.len()
        );
        return Ok(());
    }

    for report in &reports {
        if report.renamed_to.is_some() {
            println!(
                "keeping {} (rename match — adopt it instead)",
                report.orphan.state_path.display()
            );
            continue;
        }
        std::fs::remove_file(&report.orphan.state_path)?;
        println!("deleted {}", report.orphan.state_path.display());
        let output = resolve_output_path(&report.orphan.expected_source, &ctx.config);
        if output.is_file() {
            std::fs::remove_file(&output)?;
            println!("deleted {}", output.display());
        }
    }
    Ok(())
}
