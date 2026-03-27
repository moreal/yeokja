use anyhow::Result;
use std::path::Path;
use yeokja_core::config::ProjectConfig;
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
    let config_path = Path::new("yeokja.toml");
    let config = if config_path.exists() {
        ProjectConfig::load(config_path)?
    } else {
        anyhow::bail!("yeokja.toml not found in current directory");
    };

    let glossary_path = Path::new(&config.project.glossary);
    let content = if glossary_path.exists() {
        std::fs::read_to_string(glossary_path)?
    } else {
        String::new()
    };

    // Parse existing, add/update term, write back
    let mut doc: toml::Table = if content.is_empty() {
        toml::Table::new()
    } else {
        content.parse()?
    };

    let terms = doc.entry("terms").or_insert_with(|| toml::Value::Table(toml::Table::new()));
    if let toml::Value::Table(terms_table) = terms {
        let mut entry = toml::Table::new();
        entry.insert("translation".to_string(), toml::Value::String(translation.to_string()));
        terms_table.insert(term.to_string(), toml::Value::Table(entry));
    }

    std::fs::write(glossary_path, toml::to_string_pretty(&doc)?)?;
    println!("Set: {} → {}", term, translation);

    Ok(())
}
