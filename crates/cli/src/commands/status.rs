use anyhow::Result;
use std::path::Path;
use yeokja_core::change::SegmentStatus;
use yeokja_core::config::ProjectConfig;
use yeokja_core::project::ProjectContext;
use yeokja_core::reconcile::reconcile_with_status;
use yeokja_core::state::StateFile;

use super::get_parser;

pub fn run(path: &str) -> Result<()> {
    let ctx = ProjectContext::load()?;

    let source_path = Path::new(path);

    let mut total = 0usize;
    let mut translated = 0usize;
    let mut pending = 0usize;
    let mut stale = 0usize;
    let mut glossary_stale = 0usize;
    let mut context_changed = 0usize;

    let files = collect_files(source_path, &ctx.config)?;

    for file_path in &files {
        tracing::debug!(file = %file_path.display(), "Processing file");
        let parser = get_parser(file_path, &ctx.config);
        let source = std::fs::read_to_string(file_path)?;
        let doc = parser.parse(&source);
        let state_path = StateFile::state_file_path(file_path);

        let existing = if state_path.exists() {
            StateFile::load(&state_path)?
        } else {
            StateFile::new(0)
        };

        let reconciled = reconcile_with_status(&doc, &existing, &ctx.glossary);

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

    Ok(())
}

fn percent(part: usize, total: usize) -> f64 {
    if total == 0 { 0.0 } else { (part as f64 / total as f64) * 100.0 }
}

pub fn collect_files(path: &Path, config: &ProjectConfig) -> Result<Vec<std::path::PathBuf>> {
    let mut files = Vec::new();

    if path.is_file() {
        files.push(path.to_path_buf());
    } else if path.is_dir() {
        for source in &config.sources {
            let pattern = format!("{}/{}", source.path, source.pattern);
            for entry in glob::glob(&pattern)? {
                files.push(entry?);
            }
        }
        // If no sources configured, default to **/*.md
        if config.sources.is_empty() {
            let pattern = format!("{}/**/*.md", path.display());
            for entry in glob::glob(&pattern)? {
                files.push(entry?);
            }
        }
    }

    Ok(files)
}
