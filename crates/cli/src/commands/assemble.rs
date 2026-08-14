use anyhow::Result;
use std::path::Path;
use yeokja_core::project::ProjectContext;

pub fn run() -> Result<()> {
    let ctx = ProjectContext::load()?;
    let report = yeokja_assemble::assemble(Path::new("."), &ctx.config)?;

    println!("Assembled {}", report.target.display());
    println!("  base files linked: {}", report.files_linked);
    println!("  overlaid:          {}", report.files_overlaid);
    for patch in &report.patches_applied {
        println!("  patched:           {patch}");
    }
    if report.steps_run > report.patches_applied.len() {
        println!(
            "  generate steps:    {}",
            report.steps_run - report.patches_applied.len()
        );
    }
    if !report.skipped_orphans.is_empty() {
        println!(
            "  skipped (source gone upstream — see `yeokja orphans`):"
        );
        for rel in &report.skipped_orphans {
            println!("    {}", rel.display());
        }
    }
    Ok(())
}
