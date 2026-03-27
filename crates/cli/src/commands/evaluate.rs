use anyhow::Result;
use std::path::Path;
use yeokja_core::change::SegmentStatus;
use yeokja_core::project::ProjectContext;
use yeokja_core::reconcile::reconcile_with_status;
use yeokja_core::state::StateFile;
use yeokja_translate::evaluator::TranslationEvaluator;
use yeokja_translate::evaluator::{EvaluationContext, IssueSeverity};
use yeokja_translate::evaluator_format::FormatEvaluator;
use yeokja_translate::evaluator_glossary::GlossaryEvaluator;
use yeokja_translate::evaluator_link::LinkEvaluator;
use yeokja_translate::evaluator_style::StyleEvaluator;

use crate::provider_factory::create_evaluator_provider;

use super::get_parser;

pub async fn run(path: &str) -> Result<()> {
    let ctx = ProjectContext::load()?;

    let eval_provider = create_evaluator_provider(&ctx.config.provider)?;
    let source_path = Path::new(path);
    let files = super::status::collect_files(source_path, &ctx.config)?;

    let glossary_evaluator = GlossaryEvaluator;
    let link_evaluator = LinkEvaluator;
    let format_evaluator = FormatEvaluator;
    let style_evaluator = StyleEvaluator::new(eval_provider, ctx.config.project.target_lang.clone());

    let evaluators: Vec<(&str, &dyn TranslationEvaluator)> = vec![
        ("Glossary", &glossary_evaluator),
        ("Link", &link_evaluator),
        ("Format", &format_evaluator),
        ("Style", &style_evaluator),
    ];

    let mut total_segments = 0usize;
    let mut total_issues = 0usize;

    tracing::info!(files = files.len(), "Evaluating translations");

    for file_path in &files {
        let parser = get_parser(file_path, &ctx.config);
        let source = std::fs::read_to_string(file_path)?;
        let doc = parser.parse(&source);
        let state_path = StateFile::state_file_path(file_path);

        let existing = if state_path.exists() {
            StateFile::load(&state_path)?
        } else {
            tracing::debug!(file = %file_path.display(), "No state file, skipping");
            continue;
        };

        let reconciled = reconcile_with_status(&doc, &existing, &ctx.glossary);

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
            };

            for (name, evaluator) in &evaluators {
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
                                evaluator = name,
                                level,
                                "{}", issue.message
                            );
                        }
                    }
                    Err(e) => {
                        tracing::warn!(
                            evaluator = name,
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
