use anyhow::Result;
use std::path::Path;
use yeokja_core::glossary::{remove_term_in_file, upsert_term_in_file};
use yeokja_core::project::ProjectContext;

pub fn list() -> Result<()> {
    let ctx = ProjectContext::load()?;

    let terms = ctx.glossary.terms();
    if terms.is_empty() {
        println!("Glossary is empty.");
        return Ok(());
    }

    println!("Glossary ({} terms):", terms.len());
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");

    let mut sorted: Vec<_> = terms.iter().collect();
    sorted.sort_by_key(|(k, _)| k.as_str());

    for (term, translation) in sorted {
        println!("  {} → {}", term, translation);
    }

    Ok(())
}

pub fn set(term: &str, translation: &str) -> Result<()> {
    let ctx = ProjectContext::load()?;
    let glossary_path = Path::new(&ctx.config.project.glossary);
    upsert_term_in_file(glossary_path, term, translation)?;
    println!("Set: {} → {}", term, translation);
    Ok(())
}

pub fn remove(term: &str) -> Result<()> {
    let ctx = ProjectContext::load()?;
    let glossary_path = Path::new(&ctx.config.project.glossary);
    if remove_term_in_file(glossary_path, term)? {
        println!("Removed: {}", term);
    } else {
        println!("Term not found: {}", term);
    }
    Ok(())
}
