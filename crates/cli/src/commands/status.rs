use anyhow::Result;
use std::path::Path;
use yeokja_core::change::SegmentStatus;
use yeokja_core::project::ProjectContext;
use yeokja_translate::orchestrator::{collect_files, scan_file};

pub fn run(path: &str, check: bool) -> Result<()> {
    let ctx = ProjectContext::load()?;
    let parser_factory = super::parser_factory();

    let files = collect_files(Path::new(path), &ctx.config)?;

    let mut total = 0usize;
    let mut translated = 0usize;
    let mut pending = 0usize;
    let mut stale = 0usize;
    let mut glossary_stale = 0usize;
    let mut context_changed = 0usize;

    for file_path in &files {
        tracing::debug!(file = %file_path.display(), "Processing file");
        let (_, reconciled) = scan_file(file_path, &ctx.config, &ctx.glossary, &parser_factory)?;

        for rs in &reconciled {
            total += 1;
            match rs.status {
                SegmentStatus::Translated => translated += 1,
                SegmentStatus::Pending => pending += 1,
                SegmentStatus::Stale => stale += 1,
                SegmentStatus::GlossaryStale => glossary_stale += 1,
                SegmentStatus::ContextChanged => context_changed += 1,
            }
        }
    }

    println!("Translation Status for: {}", path);
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("Files:            {}", files.len());
    println!("Total segments:   {}", total);
    println!("Translated:       {} ({:.1}%)", translated, percent(translated, total));
    println!("Pending:          {}", pending);
    println!("Stale:            {}", stale);
    println!("Glossary stale:   {}", glossary_stale);
    println!("Context changed:  {}", context_changed);

    // The CI gate: publishing a build with untranslated segments would
    // silently ship English prose in the translated edition. Orphans are
    // deliberately not counted — a kept orphan (a chapter upstream may
    // revive) is not untranslated work.
    if check && pending + stale + glossary_stale + context_changed > 0 {
        anyhow::bail!(
            "{} segment(s) still need translation",
            pending + stale + glossary_stale + context_changed
        );
    }

    // Orphans are project-wide (they have no source to fall under `path`),
    // so they are reported regardless of the filter.
    let orphans = yeokja_translate::orchestrator::match_orphans(&files, &ctx.config, &parser_factory);
    if !orphans.is_empty() {
        println!("Orphaned state:   {}", orphans.len());
        for report in &orphans {
            match &report.renamed_to {
                Some(to) => println!(
                    "  {} → looks renamed to {}; `yeokja translate` will adopt it",
                    report.orphan.expected_source.display(),
                    to.display()
                ),
                None => println!(
                    "  {} (state kept at {})",
                    report.orphan.expected_source.display(),
                    report.orphan.state_path.display()
                ),
            }
        }
    }

    Ok(())
}

fn percent(part: usize, total: usize) -> f64 {
    if total == 0 { 0.0 } else { (part as f64 / total as f64) * 100.0 }
}
