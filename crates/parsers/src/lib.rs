//! Registry of available document parsers.
//!
//! Central place where a file is mapped to its parser, shared by the CLI and
//! the server so both always agree on parser selection.

use std::path::Path;
use yeokja_core::config::ProjectConfig;
use yeokja_core::parser::DocumentParser;

/// Select the parser for a file, preferring the source config whose `path` and
/// `pattern` both match, and falling back to file-extension detection.
pub fn select_parser(file_path: &Path, config: &ProjectConfig) -> Box<dyn DocumentParser> {
    let file_path = file_path.strip_prefix(".").unwrap_or(file_path);
    for source in &config.sources {
        let source_dir = Path::new(&source.path);
        let source_dir = source_dir.strip_prefix(".").unwrap_or(source_dir);
        let Ok(rel) = file_path.strip_prefix(source_dir) else {
            continue;
        };
        let pattern_matches = glob::Pattern::new(&source.pattern)
            .map(|pattern| {
                pattern.matches_path_with(
                    rel,
                    glob::MatchOptions {
                        require_literal_separator: true,
                        ..glob::MatchOptions::new()
                    },
                )
            })
            .unwrap_or(false);
        if pattern_matches {
            return parser_by_name(&source.parser, file_path, source.parser_manifest.as_deref());
        }
    }
    match file_path.extension().and_then(|e| e.to_str()) {
        Some("adoc" | "asciidoc" | "asc") => Box::new(yeokja_parser_asciidoc::AsciidocParser),
        Some("rst" | "rest") => Box::new(yeokja_parser_rst::RstParser),
        Some("tex" | "latex") => Box::new(yeokja_parser_latex::LatexParser),
        // A Lean manual must never be interpreted as Markdown just because its
        // source rule is missing. With no manifest this parser reports a hard,
        // actionable error from `parse_checked`.
        Some("lean") => Box::new(yeokja_parser_verso::VersoParser::new(file_path, "")),
        _ => Box::new(yeokja_parser_markdown::MarkdownParser),
    }
}

fn parser_by_name(
    name: &str,
    file_path: &Path,
    parser_manifest: Option<&str>,
) -> Box<dyn DocumentParser> {
    match name {
        "asciidoc" => Box::new(yeokja_parser_asciidoc::AsciidocParser),
        "rst" => Box::new(yeokja_parser_rst::RstParser),
        "pep" => Box::new(yeokja_parser_rst::PepParser),
        "pep_plaintext" => Box::new(yeokja_parser_rst::PepPlaintextParser),
        "pep_text_block" => Box::new(yeokja_parser_rst::PepTextBlockParser),
        "mil" | "mathematics_in_lean" => Box::new(yeokja_parser_rst::MilParser),
        "latex" | "tex" => Box::new(yeokja_parser_latex::LatexParser),
        "verso" => Box::new(yeokja_parser_verso::VersoParser::new(
            file_path,
            parser_manifest.unwrap_or_default(),
        )),
        _ => Box::new(yeokja_parser_markdown::MarkdownParser),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use yeokja_core::config::ProjectConfig;

    fn config_with_source(path: &str, parser: &str) -> ProjectConfig {
        ProjectConfig::from_toml(&format!(
            r#"
[project]
source_lang = "en"
target_lang = "ko"

[[sources]]
path = "{path}"
pattern = "**/*.md"
parser = "{parser}"
output = "{{dir}}/{{stem}}.ko{{ext}}"

[provider]
type = "openai_compatible"
model = "gpt-4o"
"#
        ))
        .unwrap()
    }

    #[test]
    fn source_config_parser_wins() {
        let config = config_with_source("book/", "asciidoc");
        // .md extension, but the source config says asciidoc
        let parser = select_parser(Path::new("book/ch1.md"), &config);
        let doc = parser.parse("= Title");
        // Asciidoc parses "= Title" as a level-1 heading
        assert_eq!(doc.sections[0].blocks[0].heading_level, Some(1));
    }

    #[test]
    fn pattern_disambiguates_sources_sharing_a_path() {
        let config = ProjectConfig::from_toml(
            r#"
[project]
source_lang = "en"
target_lang = "ko"

[[sources]]
path = "docs"
pattern = "**/*.adoc"
parser = "asciidoc"
output = "{dir}/{stem}.ko{ext}"

[[sources]]
path = "docs"
pattern = "**/*.md"
parser = "markdown"
output = "{dir}/{stem}.ko{ext}"

[provider]
type = "openai_compatible"
model = "gpt-4o"
"#,
        )
        .unwrap();

        // Both sources share `docs`; the pattern must pick the right parser.
        let parser = select_parser(Path::new("docs/notes.md"), &config);
        let doc = parser.parse("# Title");
        assert_eq!(doc.sections[0].blocks[0].heading_level, Some(1));

        let parser = select_parser(Path::new("docs/notes.adoc"), &config);
        let doc = parser.parse("= Title");
        assert_eq!(doc.sections[0].blocks[0].heading_level, Some(1));
    }

    #[test]
    fn a_single_star_does_not_match_across_directories() {
        let config = ProjectConfig::from_toml(
            r#"
[project]
source_lang = "en"
target_lang = "ko"

[[sources]]
path = "peps"
pattern = "pep-*.rst"
parser = "pep"
output = "ko/{path}"

[[sources]]
path = "peps"
pattern = "pep-*/appendix-*.rst"
parser = "rst"
output = "ko/{path}"

[provider]
type = "openai_compatible"
model = "test"
"#,
        )
        .unwrap();

        let parser = select_parser(Path::new("peps/pep-0639/appendix-examples.rst"), &config);
        let document = parser.parse("Appendix\n========\n\nBody.\n");
        assert_eq!(document.translatable_segments()[0].source, "Appendix");
    }

    #[test]
    fn source_config_selects_verso_for_lean_files() {
        let config = ProjectConfig::from_toml(
            r#"
[project]
source_lang = "en"
target_lang = "ko"

[[sources]]
path = "book"
pattern = "**/*.lean"
parser = "verso"
output = "ko/{path}"

[provider]
type = "openai_compatible"
model = "gpt-4o"
"#,
        )
        .unwrap();

        let parser = select_parser(Path::new("book/Chapter.lean"), &config);
        assert_eq!(parser.markup(), yeokja_core::parser::Markup::Verso);
    }

    #[test]
    fn source_config_selects_mil_parser_for_lean_files() {
        let config = ProjectConfig::from_toml(
            r#"
[project]
source_lang = "en"
target_lang = "ko"

[[sources]]
path = "MIL"
pattern = "**/*.lean"
parser = "mil"
output = "ko/{path}"

[provider]
type = "openai_compatible"
model = "gpt-4o"
"#,
        )
        .unwrap();

        let parser = select_parser(Path::new("MIL/C01/S01.lean"), &config);
        assert_eq!(parser.markup(), yeokja_core::parser::Markup::Rst);
        assert_eq!(
            parser
                .parse("/- TEXT:\nHello.\nTEXT. -/")
                .translatable_segments()[0]
                .source,
            "Hello."
        );
    }

    #[test]
    fn extension_fallback_selects_asciidoc() {
        let config = config_with_source("book/", "markdown");
        let parser = select_parser(Path::new("docs/guide.adoc"), &config);
        let doc = parser.parse("= Title");
        assert_eq!(doc.sections[0].blocks[0].heading_level, Some(1));
    }

    #[test]
    fn extension_fallback_selects_markdown() {
        let config = config_with_source("book/", "markdown");
        let parser = select_parser(Path::new("docs/guide.md"), &config);
        let doc = parser.parse("# Title");
        assert_eq!(doc.sections[0].blocks[0].heading_level, Some(1));
    }

    #[test]
    fn extension_fallback_selects_latex() {
        let config = config_with_source("book/", "markdown");
        let parser = select_parser(Path::new("docs/chapter.tex"), &config);
        assert_eq!(parser.markup(), yeokja_core::parser::Markup::Latex);
    }

    #[test]
    fn lean_extension_never_falls_back_to_markdown() {
        let config = config_with_source("book/", "markdown");
        let parser = select_parser(Path::new("docs/Chapter.lean"), &config);
        assert_eq!(parser.markup(), yeokja_core::parser::Markup::Verso);
        assert!(
            parser
                .parse_checked("#doc (Manual) \"Title\" =>")
                .unwrap_err()
                .to_string()
                .contains("official Verso span manifest")
        );
    }
}
