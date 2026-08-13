use anyhow::Result;
use std::path::Path;
use yeokja_core::change::SegmentStatus;
use yeokja_core::project::ProjectContext;
use yeokja_translate::evaluator::{EvaluationContext, IssueSeverity};
use yeokja_translate::factory::create_evaluator_provider;
use yeokja_translate::orchestrator::{collect_files, scan_file, standard_evaluators};

pub async fn run(path: &str) -> Result<()> {
    let ctx = ProjectContext::load()?;
    let parser_factory = super::parser_factory();

    let eval_provider = create_evaluator_provider(&ctx.config.provider)?;
    let files = collect_files(Path::new(path), &ctx.config)?;

    let evaluators = standard_evaluators(eval_provider, &ctx.config.project.target_lang);

    let mut total_segments = 0usize;
    let mut total_issues = 0usize;

    tracing::info!(files = files.len(), "Evaluating translations");

    for file_path in &files {
        let state_path = yeokja_core::state::StateFile::state_file_path(file_path);
        if !state_path.exists() {
            tracing::debug!(file = %file_path.display(), "No state file, skipping");
            continue;
        }

        let (_, reconciled) = scan_file(file_path, &ctx.config, &ctx.glossary, &parser_factory)?;
        let markup = parser_factory(file_path, &ctx.config).markup();

        let mut file_issues = 0usize;

        for rs in &reconciled {
            // Only evaluate translated segments
            if !matches!(rs.status, SegmentStatus::Translated) {
                continue;
            }

            let translation = match &rs.state.translation {
                Some(t) => t,
                None => continue,
            };

            total_segments += 1;

            let eval_ctx = EvaluationContext {
                source: rs.state.source.clone(),
                translation: translation.clone(),
                glossary: ctx.glossary.terms().clone(),
                source_lang: ctx.config.project.source_lang.clone(),
                target_lang: ctx.config.project.target_lang.clone(),
                markup,
            };

            for evaluator in &evaluators {
                match evaluator.evaluate(&eval_ctx).await {
                    Ok(result) => {
                        for issue in &result.issues {
                            file_issues += 1;
                            let level = match issue.severity {
                                IssueSeverity::Error => "ERROR",
                                IssueSeverity::Warning => "WARN",
                            };
                            tracing::info!(
                                file = %file_path.display(),
                                segment = %rs.state.id,
                                evaluator = evaluator.name(),
                                level,
                                "{}", issue.message
                            );
                        }
                    }
                    Err(e) => {
                        tracing::warn!(
                            evaluator = evaluator.name(),
                            error = %e,
                            "Evaluator failed"
                        );
                    }
                }
            }
        }

        if file_issues > 0 {
            tracing::info!(file = %file_path.display(), issues = file_issues, "Issues found");
        } else {
            tracing::info!(file = %file_path.display(), "No issues");
        }

        total_issues += file_issues;
    }

    println!("\nEvaluation complete: {} segments checked, {} issues found", total_segments, total_issues);
    Ok(())
}
