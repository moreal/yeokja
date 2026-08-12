pub mod coverage;
pub mod evaluate;
pub mod glossary;
pub mod inspect;
pub mod serve;
pub mod status;
pub mod translate;

use yeokja_translate::orchestrator::ParserFactory;

/// Parser selection shared with the server through the yeokja-parsers registry.
pub fn parser_factory() -> ParserFactory {
    std::sync::Arc::new(yeokja_parsers::select_parser)
}
