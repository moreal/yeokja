use anyhow::Result;
use std::path::Path;
use std::sync::Arc;
use yeokja_core::project::ProjectContext;
use yeokja_translate::factory::{create_evaluator_provider, create_provider};
use yeokja_translate::orchestrator::{
    Orchestrator, ProgressSender, TranslateOptions, TranslateOutcome,
};

/// Run a translation over `path`, reporting progress to `progress` if given.
pub async fn run(path: &str, progress: Option<ProgressSender>) -> Result<TranslateOutcome> {
    let ctx = ProjectContext::load()?;

    let options = TranslateOptions::from_config(&ctx.config);
    let provider = create_provider(&ctx.config.provider)?;
    let eval_provider = if options.auto_evaluate {
        create_evaluator_provider(&ctx.config.provider)?
    } else {
        None
    };

    let orchestrator = Orchestrator {
        config: Arc::new(ctx.config),
        glossary: Arc::new(ctx.glossary),
        provider,
        eval_provider,
        parser_factory: super::parser_factory(),
        options,
    };

    let outcome = orchestrator.translate_path(Path::new(path), progress).await?;

    if outcome.files_failed > 0 {
        tracing::warn!(errors = outcome.files_failed, "Translation finished with errors");
    }
    tracing::info!(
        files = outcome.files_processed,
        segments = outcome.segments_translated,
        "Translation done"
    );
    Ok(outcome)
}
