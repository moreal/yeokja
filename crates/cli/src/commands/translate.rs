use anyhow::Result;
use std::path::Path;
use std::sync::Arc;
use tokio::sync::mpsc;
use yeokja_core::hash::content_hash;
use yeokja_core::project::ProjectContext;
use yeokja_core::reconcile::{reconcile_with_status, ReconciledSegment};
use yeokja_core::state::{SegmentState, StateFile};
use yeokja_core::config::ProjectConfig;
use yeokja_core::model::Document;
use yeokja_translate::evaluator::TranslationEvaluator;
use yeokja_translate::evaluator_format::FormatEvaluator;
use yeokja_translate::evaluator_glossary::GlossaryEvaluator;
use yeokja_translate::evaluator_link::LinkEvaluator;
use yeokja_translate::evaluator_style::StyleEvaluator;
use yeokja_translate::pipeline::translate_with_evaluation;
use yeokja_translate::provider::TranslateRequest;
use chrono::Utc;

use crate::provider_factory::{create_provider, create_evaluator_provider};



/// A completed block translation to be saved by the state writer.
struct BlockResult {
    file_path: std::path::PathBuf,
    segment_updates: Vec<SegmentUpdate>,
}

struct SegmentUpdate {
    index: usize,
    translation: String,
    glossary_snapshot: std::collections::HashMap<String, String>,
    issues: Vec<String>,
}

/// Group segments needing translation by their block.
/// Returns Vec<(block_raw_content, Vec<(segment_index_in_reconciled, segment_state)>)>
fn group_by_block(
    doc: &Document,
    reconciled: &[ReconciledSegment],
) -> Vec<(String, Vec<(usize, SegmentState)>)> {
    let mut needs_translation: std::collections::HashSet<usize> = std::collections::HashSet::new();
    for (i, rs) in reconciled.iter().enumerate() {
        if rs.status.needs_translation() {
            needs_translation.insert(i);
        }
    }

    if needs_translation.is_empty() {
        return Vec::new();
    }

    // Walk blocks and group segments
    let mut groups = Vec::new();
    let mut flat_idx = 0usize;

    for section in &doc.sections {
        for block in &section.blocks {
            if !block.block_type.is_translatable() {
                continue;
            }

            let mut block_segments = Vec::new();
            for _seg in &block.segments {
                if needs_translation.contains(&flat_idx) {
                    block_segments.push((flat_idx, reconciled[flat_idx].state.clone()));
                }
                flat_idx += 1;
            }

            if !block_segments.is_empty() {
                groups.push((block.raw_content.clone(), block_segments));
            }
        }
    }

    groups
}

pub async fn run(path: &str) -> Result<()> {
    let ctx = ProjectContext::load()?;

    let provider = create_provider(&ctx.config.provider)?;
    let eval_provider = create_evaluator_provider(&ctx.config.provider)?;
    let source_path = Path::new(path);
    let files = super::status::collect_files(source_path, &ctx.config)?;

    let auto_evaluate = ctx.config.evaluation.as_ref().map(|e| e.auto_evaluate).unwrap_or(true);
    let max_retries = ctx.config.evaluation.as_ref().map(|e| e.max_retries).unwrap_or(3);

    tracing::info!(files = files.len(), "Found files to process");

    // Move Arc creation outside the file loop (#12)
    let glossary = Arc::new(ctx.glossary);
    let config = Arc::new(ctx.config);

    // Global semaphore shared across all files and blocks
    let semaphore = Arc::new(tokio::sync::Semaphore::new(4));

    let mut file_handles = Vec::new();
    for file_path in files {
        let glossary = glossary.clone();
        let config = config.clone();
        let provider = provider.clone();
        let eval_provider = eval_provider.clone();
        let semaphore = semaphore.clone();

        file_handles.push(tokio::spawn(async move {
            translate_file(
                &file_path,
                &glossary,
                &config,
                &provider,
                &eval_provider,
                auto_evaluate,
                max_retries,
                &semaphore,
            ).await
            .map_err(|e| (file_path, e))
        }));
    }

    let mut error_count = 0usize;
    for handle in file_handles {
        match handle.await {
            Ok(Ok(())) => {}
            Ok(Err((file_path, e))) => {
                tracing::error!(file = %file_path.display(), error = %e, "Failed to process file");
                error_count += 1;
            }
            Err(e) => {
                tracing::error!(error = %e, "File task panicked");
                error_count += 1;
            }
        }
    }

    if error_count > 0 {
        tracing::warn!(errors = error_count, "Translation finished with errors");
    }
    tracing::info!("Translation done");
    Ok(())
}

async fn translate_file(
    file_path: &std::path::PathBuf,
    glossary: &Arc<yeokja_core::glossary::Glossary>,
    config: &Arc<ProjectConfig>,
    provider: &Arc<dyn yeokja_translate::provider::TranslationProvider>,
    eval_provider: &Arc<dyn yeokja_translate::provider::LlmProvider>,
    auto_evaluate: bool,
    max_retries: u32,
    semaphore: &Arc<tokio::sync::Semaphore>,
) -> Result<()> {
    let parser = super::get_parser(file_path, config);
    let source = std::fs::read_to_string(file_path)?;
    let doc = parser.parse(&source);
    let state_path = StateFile::state_file_path(file_path);

    let existing = if state_path.exists() {
        StateFile::load(&state_path)?
    } else {
        StateFile::new(0)
    };

    let reconciled = reconcile_with_status(&doc, &existing, glossary);
    let block_groups = group_by_block(&doc, &reconciled);

    if block_groups.is_empty() {
        tracing::info!(file = %file_path.display(), "Up to date");
        return Ok(());
    }

    let total_segments: usize = block_groups.iter().map(|(_, segs)| segs.len()).sum();
    tracing::info!(
        file = %file_path.display(),
        blocks = block_groups.len(),
        segments = total_segments,
        "Translating"
    );

    // Collect initial segment states from reconciled, updating context_hash to current
    let initial_segments: Vec<SegmentState> = reconciled.iter().map(|rs| {
        let mut state = rs.state.clone();
        state.context_hash = rs.context_hash;
        state
    }).collect();

    // State writer: receives block results via mpsc and saves state
    let (tx, mut rx) = mpsc::channel::<BlockResult>(64);
    let updated_segments = initial_segments.clone();
    let state_path_clone = state_path.clone();
    let source_hash = content_hash(&source);
    let writer_segments = Arc::new(tokio::sync::Mutex::new(updated_segments.clone()));
    let writer_segments_for_task = writer_segments.clone();

    let state_writer = tokio::spawn(async move {
        while let Some(block_result) = rx.recv().await {
            let mut segs = writer_segments_for_task.lock().await;
            for update in &block_result.segment_updates {
                let seg = &mut segs[update.index];
                seg.translation = Some(update.translation.clone());
                seg.translated_at = Some(Utc::now());
                seg.glossary_snapshot = update.glossary_snapshot.clone();
                seg.issues = update.issues.clone();
            }

            // Save state after each block
            let mut state = StateFile::new(source_hash);
            state.segments = segs.clone();
            if let Err(e) = state.save(&state_path_clone) {
                tracing::error!(error = %e, "Failed to save state");
            }

            tracing::debug!(
                file = %block_result.file_path.display(),
                updates = block_result.segment_updates.len(),
                "State saved"
            );
        }
    });

    // Translate blocks in parallel
    let glossary = glossary.clone();
    let config = config.clone();
    let eval_provider = eval_provider.clone();
    let file_path_arc = Arc::new(file_path.clone());

    let mut handles = Vec::new();

    for (block_context, block_segments) in block_groups {
        let provider = provider.clone();
        let eval_provider = eval_provider.clone();
        let glossary = glossary.clone();
        let config = config.clone();
        let tx = tx.clone();
        let file_path = file_path_arc.clone();
        let semaphore = semaphore.clone();

        let handle = tokio::spawn(async move {
            let _permit = semaphore.acquire().await.unwrap();

            let request_segments: Vec<(usize, String)> = block_segments
                .iter()
                .enumerate()
                .map(|(req_idx, (_, seg))| (req_idx + 1, seg.source.clone()))
                .collect();

            let glossary_terms = glossary.terms().clone();

            let request = TranslateRequest {
                segments: request_segments,
                block_context: block_context.clone(),
                glossary: glossary_terms.clone(),
                source_lang: config.project.source_lang.clone(),
                target_lang: config.project.target_lang.clone(),
                feedback: None,
            };

            tracing::debug!(
                block_segments = block_segments.len(),
                block_context_len = block_context.len(),
                "Sending block translation request"
            );

            let translation_result = if auto_evaluate {
                let glossary_evaluator = GlossaryEvaluator;
                let link_evaluator = LinkEvaluator;
                let format_evaluator = FormatEvaluator;
                let style_evaluator = StyleEvaluator::new(
                    eval_provider.clone(),
                    config.project.target_lang.clone(),
                );
                let evaluators: Vec<&dyn TranslationEvaluator> = vec![
                    &glossary_evaluator,
                    &link_evaluator,
                    &format_evaluator,
                    &style_evaluator,
                ];

                translate_with_evaluation(
                    provider.as_ref(),
                    &evaluators,
                    request,
                    &glossary_terms,
                    &config.project.source_lang,
                    &config.project.target_lang,
                    max_retries,
                )
                .await
                .map(|results| {
                    results
                        .into_iter()
                        .map(|(idx, pr)| (idx, pr.translation, pr.evaluation.map(|e| {
                            e.issues.iter().map(|i| i.message.clone()).collect::<Vec<_>>()
                        }).unwrap_or_default()))
                        .collect::<Vec<_>>()
                })
            } else {
                provider
                    .translate(request)
                    .await
                    .map(|response| {
                        response
                            .translations
                            .into_iter()
                            .map(|(idx, text)| (idx, text, Vec::new()))
                            .collect::<Vec<_>>()
                    })
            };

            match translation_result {
                Ok(translations) => {
                    let mut updates = Vec::new();
                    for (req_idx, (seg_idx, seg)) in block_segments.iter().enumerate() {
                        if let Some((_, text, issues)) = translations.iter().find(|(idx, _, _)| *idx == req_idx + 1) {
                            let matching_terms = glossary.find_matching_terms(&seg.source);
                            updates.push(SegmentUpdate {
                                index: *seg_idx,
                                translation: text.clone(),
                                glossary_snapshot: matching_terms,
                                issues: issues.clone(),
                            });
                        }
                    }

                    tracing::debug!(
                        updates = updates.len(),
                        "Block translation complete"
                    );

                    let _ = tx.send(BlockResult {
                        file_path: file_path.as_ref().clone(),
                        segment_updates: updates,
                    }).await;
                }
                Err(e) => {
                    tracing::error!(error = %e, "Block translation failed");
                }
            }
        });

        handles.push(handle);
    }

    // Wait for all translation tasks
    for handle in handles {
        if let Err(e) = handle.await {
            tracing::error!(error = %e, "Translation task panicked");
        }
    }

    // Drop sender to close channel, then wait for writer to finish
    drop(tx);
    state_writer.await?;

    // Read final state from writer and reconstruct
    let final_segments = Arc::try_unwrap(writer_segments)
        .expect("writer segments still referenced")
        .into_inner();

    // Reconstruct translated document
    let mut translations = yeokja_core::parser::TranslationMap::new();
    for seg in &final_segments {
        if let Some(t) = &seg.translation {
            translations.insert(seg.id.clone(), t.clone());
        }
    }

    let output_text = parser.reconstruct(&doc, &translations);

    let output_path = resolve_output_path(file_path, &config);
    if let Some(parent) = output_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&output_path, &output_text)?;

    let translated_count = final_segments.iter().filter(|s| s.translation.is_some()).count();
    let total_count = final_segments.len();
    tracing::info!(
        file = %file_path.display(),
        translated = translated_count,
        total = total_count,
        output = %output_path.display(),
        "Translation complete"
    );

    Ok(())
}

fn resolve_output_path(source_path: &Path, config: &ProjectConfig) -> std::path::PathBuf {
    for source_config in &config.sources {
        let source_dir = Path::new(&source_config.path);
        if let Ok(rel) = source_path.strip_prefix(source_dir) {
            let stem = source_path.file_stem().unwrap_or_default().to_string_lossy();
            let ext = source_path
                .extension()
                .map(|e| format!(".{}", e.to_string_lossy()))
                .unwrap_or_default();
            let dir = source_path
                .parent()
                .unwrap_or(Path::new("."))
                .to_string_lossy();
            let rel_path = rel.to_string_lossy();

            let output = source_config
                .output
                .replace("{stem}", &stem)
                .replace("{ext}", &ext)
                .replace("{dir}", &dir)
                .replace("{path}", &rel_path);

            return Path::new(&output).to_path_buf();
        }
    }

    let stem = source_path
        .file_stem()
        .unwrap_or_default()
        .to_string_lossy();
    let ext = source_path
        .extension()
        .map(|e| format!(".{}", e.to_string_lossy()))
        .unwrap_or_default();
    source_path.with_file_name(format!("{}.ko{}", stem, ext))
}
