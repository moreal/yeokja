pub mod translate;
pub mod status;
pub mod glossary;
pub mod evaluate;

use std::path::Path;
use yeokja_core::config::ProjectConfig;
use yeokja_core::parser::DocumentParser;

/// Select the appropriate parser based on the source config or file extension.
pub fn get_parser(file_path: &Path, config: &ProjectConfig) -> Box<dyn DocumentParser> {
    // Check source config parser field
    for source in &config.sources {
        let source_dir = Path::new(&source.path);
        if file_path.starts_with(source_dir) {
            return match source.parser.as_str() {
                "asciidoc" => Box::new(yeokja_parser_asciidoc::AsciidocParser),
                _ => Box::new(yeokja_parser_markdown::MarkdownParser),
            };
        }
    }
    // Default: detect by extension
    match file_path.extension().and_then(|e| e.to_str()) {
        Some("adoc" | "asciidoc" | "asc") => Box::new(yeokja_parser_asciidoc::AsciidocParser),
        _ => Box::new(yeokja_parser_markdown::MarkdownParser),
    }
}
