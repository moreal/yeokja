//! Translation orchestration shared by the CLI and the server.
//!
//! Owns the whole run: collect files, reconcile against saved state, batch
//! pending segments by block, translate (optionally with the evaluate-retry
//! pipeline), persist state incrementally, and reconstruct output files.
//! Progress is reported through an event channel so front-ends (CLI logs,
//! TUI, server API) can render it however they like.

use crate::evaluator::TranslationEvaluator;
use crate::evaluator_format::FormatEvaluator;
use crate::evaluator_glossary::GlossaryEvaluator;
use crate::evaluator_link::LinkEvaluator;
use crate::evaluator_style::StyleEvaluator;
use crate::pipeline::translate_with_evaluation;
use crate::provider::{LlmProvider, TranslateRequest, TranslationProvider};
use chrono::Utc;
use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex, Semaphore};
use yeokja_core::config::ProjectConfig;
use yeokja_core::glossary::Glossary;
use yeokja_core::hash::content_hash;
use yeokja_core::model::Document;
use yeokja_core::parser::{DocumentParser, TranslationMap};
use yeokja_core::reconcile::{reconcile_with_status, ReconciledSegment};
use yeokja_core::state::{SegmentState, StateFile};

/// Maps a file to the parser that should handle it.
pub type ParserFactory =
    Arc<dyn Fn(&Path, &ProjectConfig) -> Box<dyn DocumentParser> + Send + Sync>;

#[derive(Debug, Clone, serde::Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ProgressEvent {
    /// Emitted once at start: every file with its pending-segment count.
    FilesDiscovered { files: Vec<(PathBuf, usize)> },
    FileStarted { file: PathBuf },
    /// A block finished translating; `segments` segments were saved.
    BlockTranslated {
        file: PathBuf,
        segments: usize,
        current: Option<String>,
    },
    FileCompleted { file: PathBuf },
    FileFailed { file: PathBuf, error: String },
    Finished { errors: usize },
}

pub type ProgressSender = mpsc::UnboundedSender<ProgressEvent>;

#[derive(Debug, Clone)]
pub struct TranslateOptions {
    pub auto_evaluate: bool,
    pub max_retries: u32,
    /// Maximum concurrent block translation requests across all files.
    pub concurrency: usize,
}

impl TranslateOptions {
    pub fn from_config(config: &ProjectConfig) -> Self {
        let evaluation = config.evaluation.as_ref();
        Self {
            auto_evaluate: evaluation.map(|e| e.auto_evaluate).unwrap_or(true),
            max_retries: evaluation.map(|e| e.max_retries).unwrap_or(3),
            concurrency: 4,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct TranslateOutcome {
    pub files_processed: usize,
    pub files_failed: usize,
    pub segments_translated: usize,
}

#[derive(Debug, thiserror::Error)]
pub enum OrchestratorError {
    #[error("IO error on {path}: {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("Glob error: {0}")]
    Glob(String),
    #[error("State error: {0}")]
    State(#[from] yeokja_core::state::StateError),
    #[error("Task join error: {0}")]
    Join(#[from] tokio::task::JoinError),
}

fn send(progress: &Option<ProgressSender>, event: ProgressEvent) {
    if let Some(tx) = progress {
        let _ = tx.send(event);
    }
}

/// Strip a leading `./` so config-relative and user-supplied paths compare equal.
fn normalize(path: &Path) -> &Path {
    path.strip_prefix(".").unwrap_or(path)
}

/// Collect source files for `path` according to the project config.
///
/// - A file path is returned as-is.
/// - A directory path collects files from every configured source whose glob
///   matches, restricted to files under `path`.
/// - Files that are the *output* of another collected file (e.g. `x.ko.md`
///   produced from `x.md`) are excluded so translations are never re-translated.
pub fn collect_files(
    path: &Path,
    config: &ProjectConfig,
) -> Result<Vec<PathBuf>, OrchestratorError> {
    let mut files: BTreeSet<PathBuf> = BTreeSet::new();

    if path.is_file() {
        files.insert(path.to_path_buf());
    } else if path.is_dir() {
        let filter = normalize(path);
        let mut push_matches = |pattern: &str| -> Result<(), OrchestratorError> {
            let entries = glob::glob(pattern).map_err(|e| OrchestratorError::Glob(e.to_string()))?;
            for entry in entries {
                let entry = entry.map_err(|e| OrchestratorError::Glob(e.to_string()))?;
                if normalize(&entry).starts_with(filter) {
                    files.insert(entry);
                }
            }
            Ok(())
        };

        if config.sources.is_empty() {
            push_matches(&format!("{}/**/*.md", path.display()))?;
        } else {
            for source in &config.sources {
                let pattern = format!(
                    "{}/{}",
                    source.path.trim_end_matches('/'),
                    source.pattern
                );
                push_matches(&pattern)?;
            }
        }
    }

    // Exclude files that are outputs of other collected files.
    let outputs: HashSet<PathBuf> = files
        .iter()
        .map(|f| resolve_output_path(f, config))
        .collect();
    Ok(files.into_iter().filter(|f| !outputs.contains(f)).collect())
}

/// Resolve the output path for a source file using the source config's
/// `output` template (`{dir}`, `{stem}`, `{ext}`, `{path}` variables).
pub fn resolve_output_path(source_path: &Path, config: &ProjectConfig) -> PathBuf {
    let stem = source_path
        .file_stem()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned();
    let ext = source_path
        .extension()
        .map(|e| format!(".{}", e.to_string_lossy()))
        .unwrap_or_default();

    for source_config in &config.sources {
        let source_dir = Path::new(&source_config.path);
        if let Ok(rel) = normalize(source_path).strip_prefix(normalize(source_dir)) {
            let dir = source_path
                .parent()
                .unwrap_or(Path::new("."))
                .to_string_lossy()
                .into_owned();
            let rel_path = rel.to_string_lossy();

            let output = source_config
                .output
                .replace("{stem}", &stem)
                .replace("{ext}", &ext)
                .replace("{dir}", &dir)
                .replace("{path}", &rel_path);

            return PathBuf::from(output);
        }
    }

    source_path.with_file_name(format!("{stem}.ko{ext}"))
}

/// Parse a file and reconcile it against its saved state.
/// Shared by the status command, the server API, and the translation run.
pub fn scan_file(
    file_path: &Path,
    config: &ProjectConfig,
    glossary: &Glossary,
    parser_factory: &ParserFactory,
) -> Result<(Document, Vec<ReconciledSegment>), OrchestratorError> {
    let parser = parser_factory(file_path, config);
    let source = std::fs::read_to_string(file_path).map_err(|e| OrchestratorError::Io {
        path: file_path.to_path_buf(),
        source: e,
    })?;
    let doc = parser.parse(&source);
    let state_path = StateFile::state_file_path(file_path);
    let existing = if state_path.exists() {
        StateFile::load(&state_path)?
    } else {
        StateFile::new(0)
    };
    let reconciled = reconcile_with_status(&doc, &existing, glossary);
    Ok((doc, reconciled))
}

/// Build the standard evaluator set: mechanical checks always, plus the
/// LLM-as-judge StyleEvaluator when a provider is available.
pub fn standard_evaluators(
    style_provider: Option<Arc<dyn LlmProvider>>,
    target_lang: &str,
) -> Vec<Box<dyn TranslationEvaluator>> {
    let mut evaluators: Vec<Box<dyn TranslationEvaluator>> = vec![
        Box::new(GlossaryEvaluator),
        Box::new(LinkEvaluator),
        Box::new(FormatEvaluator),
    ];
    if let Some(provider) = style_provider {
        evaluators.push(Box::new(StyleEvaluator::new(provider, target_lang.to_string())));
    }
    evaluators
}

/// Run every evaluator against one translation and combine the results.
/// Evaluator failures (e.g. LLM errors) are logged and skipped.
pub async fn evaluate_translation(
    evaluators: &[Box<dyn TranslationEvaluator>],
    context: &crate::evaluator::EvaluationContext,
) -> crate::evaluator::EvaluationResult {
    let mut combined = crate::evaluator::EvaluationResult {
        passed: true,
        issues: Vec::new(),
    };
    for evaluator in evaluators {
        match evaluator.evaluate(context).await {
            Ok(result) => {
                if !result.passed {
                    combined.passed = false;
                }
                combined.issues.extend(result.issues);
            }
            Err(e) => {
                tracing::warn!(evaluator = evaluator.name(), error = %e, "Evaluator failed");
            }
        }
    }
    combined
}

/// Group segments needing translation by their containing block.
/// Returns `(block_raw_content, [(flat_segment_index, segment_state)])` pairs.
fn group_by_block(
    doc: &Document,
    reconciled: &[ReconciledSegment],
) -> Vec<(String, Vec<(usize, SegmentState)>)> {
    let needs_translation: HashSet<usize> = reconciled
        .iter()
        .enumerate()
        .filter(|(_, rs)| rs.status.needs_translation())
        .map(|(i, _)| i)
        .collect();

    if needs_translation.is_empty() {
        return Vec::new();
    }

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

pub struct Orchestrator {
    pub config: Arc<ProjectConfig>,
    pub glossary: Arc<Glossary>,
    pub provider: Arc<dyn TranslationProvider>,
    /// LLM used by the StyleEvaluator. `None` runs mechanical checks only.
    pub eval_provider: Option<Arc<dyn LlmProvider>>,
    pub parser_factory: ParserFactory,
    pub options: TranslateOptions,
}

impl Orchestrator {
    /// Translate every pending segment under `path` and write output files.
    pub async fn translate_path(
        &self,
        path: &Path,
        progress: Option<ProgressSender>,
    ) -> Result<TranslateOutcome, OrchestratorError> {
        let files = collect_files(path, &self.config)?;

        // Pre-scan for pending counts so front-ends can show totals up front.
        let mut discovered = Vec::new();
        for file in &files {
            let pending = match scan_file(file, &self.config, &self.glossary, &self.parser_factory)
            {
                Ok((_, reconciled)) => reconciled
                    .iter()
                    .filter(|rs| rs.status.needs_translation())
                    .count(),
                Err(_) => 0,
            };
            discovered.push((file.clone(), pending));
        }
        send(
            &progress,
            ProgressEvent::FilesDiscovered {
                files: discovered.clone(),
            },
        );
        tracing::info!(files = files.len(), "Found files to process");

        let semaphore = Arc::new(Semaphore::new(self.options.concurrency.max(1)));
        let mut outcome = TranslateOutcome::default();
        let mut handles = Vec::new();

        for file_path in files {
            let this = self.clone_parts();
            let semaphore = semaphore.clone();
            let progress = progress.clone();
            handles.push(tokio::spawn(async move {
                let result = this
                    .translate_file(&file_path, &semaphore, &progress)
                    .await;
                (file_path, result)
            }));
        }

        for handle in handles {
            let (file_path, result) = handle.await?;
            match result {
                Ok(translated) => {
                    outcome.files_processed += 1;
                    outcome.segments_translated += translated;
                    send(&progress, ProgressEvent::FileCompleted { file: file_path });
                }
                Err(e) => {
                    tracing::error!(file = %file_path.display(), error = %e, "Failed to process file");
                    outcome.files_failed += 1;
                    send(
                        &progress,
                        ProgressEvent::FileFailed {
                            file: file_path,
                            error: e.to_string(),
                        },
                    );
                }
            }
        }

        send(
            &progress,
            ProgressEvent::Finished {
                errors: outcome.files_failed,
            },
        );
        Ok(outcome)
    }

    fn clone_parts(&self) -> FileTranslator {
        FileTranslator {
            config: self.config.clone(),
            glossary: self.glossary.clone(),
            provider: self.provider.clone(),
            eval_provider: self.eval_provider.clone(),
            parser_factory: self.parser_factory.clone(),
            options: self.options.clone(),
        }
    }
}

struct FileTranslator {
    config: Arc<ProjectConfig>,
    glossary: Arc<Glossary>,
    provider: Arc<dyn TranslationProvider>,
    eval_provider: Option<Arc<dyn LlmProvider>>,
    parser_factory: ParserFactory,
    options: TranslateOptions,
}

struct SegmentUpdate {
    index: usize,
    translation: String,
    glossary_snapshot: HashMap<String, String>,
    issues: Vec<String>,
}

impl FileTranslator {
    /// Translate one file; returns the number of segments translated.
    async fn translate_file(
        &self,
        file_path: &Path,
        semaphore: &Arc<Semaphore>,
        progress: &Option<ProgressSender>,
    ) -> Result<usize, OrchestratorError> {
        let parser = (self.parser_factory)(file_path, &self.config);
        let source = std::fs::read_to_string(file_path).map_err(|e| OrchestratorError::Io {
            path: file_path.to_path_buf(),
            source: e,
        })?;
        let doc = parser.parse(&source);
        let state_path = StateFile::state_file_path(file_path);

        let existing = if state_path.exists() {
            StateFile::load(&state_path)?
        } else {
            StateFile::new(0)
        };

        let reconciled = reconcile_with_status(&doc, &existing, &self.glossary);
        let block_groups = group_by_block(&doc, &reconciled);

        let output_path = resolve_output_path(file_path, &self.config);
        if block_groups.is_empty() {
            tracing::info!(file = %file_path.display(), "Up to date");
            // Regenerate the output file if it went missing.
            if !output_path.exists() {
                let segments: Vec<SegmentState> =
                    reconciled.into_iter().map(|rs| rs.state).collect();
                self.write_output(&*parser, &doc, &segments, file_path, &output_path)?;
            }
            return Ok(0);
        }

        send(
            progress,
            ProgressEvent::FileStarted {
                file: file_path.to_path_buf(),
            },
        );

        let total_segments: usize = block_groups.iter().map(|(_, segs)| segs.len()).sum();
        tracing::info!(
            file = %file_path.display(),
            blocks = block_groups.len(),
            segments = total_segments,
            "Translating"
        );

        // Carry current context hashes into the stored state.
        let initial_segments: Vec<SegmentState> = reconciled
            .iter()
            .map(|rs| {
                let mut state = rs.state.clone();
                state.context_hash = rs.context_hash;
                state
            })
            .collect();

        // State writer: applies block results and saves after each block so an
        // interrupted run resumes from the last completed block.
        let (tx, mut rx) = mpsc::channel::<Vec<SegmentUpdate>>(64);
        let source_hash = content_hash(&source);
        let writer_segments = Arc::new(Mutex::new(initial_segments));
        let writer_segments_for_task = writer_segments.clone();
        let state_path_clone = state_path.clone();
        let progress_clone = progress.clone();
        let file_path_buf = file_path.to_path_buf();

        let state_writer = tokio::spawn(async move {
            let mut translated_count = 0usize;
            while let Some(updates) = rx.recv().await {
                let mut segs = writer_segments_for_task.lock().await;
                let current = updates
                    .first()
                    .map(|u| u.translation.clone());
                for update in &updates {
                    let seg = &mut segs[update.index];
                    seg.translation = Some(update.translation.clone());
                    seg.translated_at = Some(Utc::now());
                    seg.glossary_snapshot = update.glossary_snapshot.clone();
                    seg.issues = update.issues.clone();
                }
                translated_count += updates.len();

                let mut state = StateFile::new(source_hash);
                state.segments = segs.clone();
                if let Err(e) = state.save(&state_path_clone) {
                    tracing::error!(error = %e, "Failed to save state");
                }

                send(
                    &progress_clone,
                    ProgressEvent::BlockTranslated {
                        file: file_path_buf.clone(),
                        segments: updates.len(),
                        current,
                    },
                );
            }
            translated_count
        });

        // Translate blocks concurrently, bounded by the shared semaphore.
        let mut handles = Vec::new();
        for (block_context, block_segments) in block_groups {
            let this = FileTranslator {
                config: self.config.clone(),
                glossary: self.glossary.clone(),
                provider: self.provider.clone(),
                eval_provider: self.eval_provider.clone(),
                parser_factory: self.parser_factory.clone(),
                options: self.options.clone(),
            };
            let tx = tx.clone();
            let semaphore = semaphore.clone();

            handles.push(tokio::spawn(async move {
                let _permit = semaphore.acquire().await.expect("semaphore closed");
                match this.translate_block(&block_context, &block_segments).await {
                    Ok(updates) => {
                        let _ = tx.send(updates).await;
                    }
                    Err(e) => {
                        tracing::error!(error = %e, "Block translation failed");
                    }
                }
            }));
        }

        for handle in handles {
            if let Err(e) = handle.await {
                tracing::error!(error = %e, "Translation task panicked");
            }
        }

        drop(tx);
        let translated_count = state_writer.await?;

        let final_segments = writer_segments.lock().await.clone();
        self.write_output(&*parser, &doc, &final_segments, file_path, &output_path)?;

        Ok(translated_count)
    }

    /// Translate one block's pending segments, optionally running the
    /// evaluate-retry pipeline. Returns the resulting segment updates.
    async fn translate_block(
        &self,
        block_context: &str,
        block_segments: &[(usize, SegmentState)],
    ) -> Result<Vec<SegmentUpdate>, crate::provider::TranslateError> {
        let request_segments: Vec<(usize, String)> = block_segments
            .iter()
            .enumerate()
            .map(|(req_idx, (_, seg))| (req_idx + 1, seg.source.clone()))
            .collect();

        let glossary_terms = self.glossary.terms().clone();

        let request = TranslateRequest {
            segments: request_segments,
            block_context: block_context.to_string(),
            glossary: glossary_terms.clone(),
            source_lang: self.config.project.source_lang.clone(),
            target_lang: self.config.project.target_lang.clone(),
            feedback: None,
            prompt_template: self.config.provider.prompt_template.clone(),
        };

        let translations: Vec<(usize, String, Vec<String>)> = if self.options.auto_evaluate {
            let evaluators = standard_evaluators(
                self.eval_provider.clone(),
                &self.config.project.target_lang,
            );
            let evaluator_refs: Vec<&dyn TranslationEvaluator> =
                evaluators.iter().map(|e| e.as_ref()).collect();

            translate_with_evaluation(
                self.provider.as_ref(),
                &evaluator_refs,
                request,
                &glossary_terms,
                &self.config.project.source_lang,
                &self.config.project.target_lang,
                self.options.max_retries,
            )
            .await?
            .into_iter()
            .map(|(idx, pr)| {
                let issues = pr
                    .evaluation
                    .map(|e| e.issues.iter().map(|i| i.message.clone()).collect())
                    .unwrap_or_default();
                (idx, pr.translation, issues)
            })
            .collect()
        } else {
            self.provider
                .translate(request)
                .await?
                .translations
                .into_iter()
                .map(|(idx, text)| (idx, text, Vec::new()))
                .collect()
        };

        let mut updates = Vec::new();
        for (req_idx, (seg_idx, seg)) in block_segments.iter().enumerate() {
            if let Some((_, text, issues)) =
                translations.iter().find(|(idx, _, _)| *idx == req_idx + 1)
            {
                updates.push(SegmentUpdate {
                    index: *seg_idx,
                    translation: text.clone(),
                    glossary_snapshot: self.glossary.find_matching_terms(&seg.source),
                    issues: issues.clone(),
                });
            }
        }
        Ok(updates)
    }

    fn write_output(
        &self,
        parser: &dyn DocumentParser,
        doc: &Document,
        segments: &[SegmentState],
        file_path: &Path,
        output_path: &Path,
    ) -> Result<(), OrchestratorError> {
        let mut translations = TranslationMap::new();
        for seg in segments {
            if let Some(t) = &seg.translation {
                translations.insert(seg.id.clone(), t.clone());
            }
        }

        let output_text = parser.reconstruct(doc, &translations);
        if let Some(parent) = output_path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| OrchestratorError::Io {
                path: parent.to_path_buf(),
                source: e,
            })?;
        }
        std::fs::write(output_path, &output_text).map_err(|e| OrchestratorError::Io {
            path: output_path.to_path_buf(),
            source: e,
        })?;

        let translated = segments.iter().filter(|s| s.translation.is_some()).count();
        tracing::info!(
            file = %file_path.display(),
            translated,
            total = segments.len(),
            output = %output_path.display(),
            "Translation complete"
        );
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config(toml: &str) -> ProjectConfig {
        ProjectConfig::from_toml(toml).unwrap()
    }

    fn book_config() -> ProjectConfig {
        test_config(
            r#"
[project]
source_lang = "en"
target_lang = "ko"

[[sources]]
path = "book/"
pattern = "**/*.md"
parser = "markdown"
output = "{dir}/{stem}.ko{ext}"

[provider]
type = "openai_compatible"
model = "test"
"#,
        )
    }

    #[test]
    fn resolve_output_path_with_template() {
        let config = book_config();
        let output = resolve_output_path(Path::new("book/ch1.md"), &config);
        assert_eq!(output, PathBuf::from("book/ch1.ko.md"));
    }

    #[test]
    fn resolve_output_path_fallback() {
        let config = book_config();
        let output = resolve_output_path(Path::new("other/ch1.md"), &config);
        assert_eq!(output, PathBuf::from("other/ch1.ko.md"));
    }

    #[test]
    fn collect_files_excludes_outputs() {
        let dir = tempfile::tempdir().unwrap();
        let book = dir.path().join("book");
        std::fs::create_dir_all(&book).unwrap();
        std::fs::write(book.join("ch1.md"), "Hello.").unwrap();
        std::fs::write(book.join("ch1.ko.md"), "안녕.").unwrap();

        let config = test_config(&format!(
            r#"
[project]
source_lang = "en"
target_lang = "ko"

[[sources]]
path = "{}"
pattern = "**/*.md"
parser = "markdown"
output = "{{dir}}/{{stem}}.ko{{ext}}"

[provider]
type = "openai_compatible"
model = "test"
"#,
            book.display()
        ));

        let files = collect_files(&book, &config).unwrap();
        assert_eq!(files, vec![book.join("ch1.md")]);
    }

    #[test]
    fn collect_files_respects_path_filter() {
        let dir = tempfile::tempdir().unwrap();
        let book = dir.path().join("book");
        let docs = dir.path().join("docs");
        std::fs::create_dir_all(&book).unwrap();
        std::fs::create_dir_all(&docs).unwrap();
        std::fs::write(book.join("ch1.md"), "Hello.").unwrap();
        std::fs::write(docs.join("guide.md"), "Guide.").unwrap();

        let config = test_config(&format!(
            r#"
[project]
source_lang = "en"
target_lang = "ko"

[[sources]]
path = "{book}"
pattern = "**/*.md"
parser = "markdown"
output = "{{dir}}/{{stem}}.ko{{ext}}"

[[sources]]
path = "{docs}"
pattern = "**/*.md"
parser = "markdown"
output = "{{dir}}/{{stem}}.ko{{ext}}"

[provider]
type = "openai_compatible"
model = "test"
"#,
            book = book.display(),
            docs = docs.display()
        ));

        // Only files under `book` should be collected when book is requested.
        let files = collect_files(&book, &config).unwrap();
        assert_eq!(files, vec![book.join("ch1.md")]);
    }

    #[test]
    fn collect_files_single_file() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("solo.md");
        std::fs::write(&file, "Hello.").unwrap();
        let config = book_config();
        let files = collect_files(&file, &config).unwrap();
        assert_eq!(files, vec![file]);
    }
}
