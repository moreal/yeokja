//! Translation orchestration shared by the CLI and the server.
//!
//! Owns the whole run: collect files, reconcile against saved state, batch
//! pending segments by block, translate (optionally with the evaluate-retry
//! pipeline), persist state incrementally, and reconstruct output files.
//! Progress is reported through an event channel so front-ends (CLI logs,
//! TUI, server API) can render it however they like.

use crate::evaluator::TranslationEvaluator;
use crate::evaluator_ending::EndingEvaluator;
use crate::evaluator_format::FormatEvaluator;
use crate::evaluator_glossary::GlossaryEvaluator;
use crate::evaluator_link::LinkEvaluator;
use crate::evaluator_style::StyleEvaluator;
use crate::pipeline::translate_with_evaluation_observed;
use crate::provider::{LlmProvider, TranslateRequest, TranslationProvider};
use chrono::Utc;
use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::{Mutex, Semaphore, mpsc};
use yeokja_core::config::ProjectConfig;
use yeokja_core::glossary::Glossary;
use yeokja_core::hash::content_hash;
use yeokja_core::model::Document;
use yeokja_core::parser::{DocumentParser, Markup, TranslationMap};
use yeokja_core::reconcile::{ReconciledSegment, reconcile_with_status};
use yeokja_core::select::apply_table_rules;
use yeokja_core::state::{SegmentState, StateFile};

/// Maps a file to the parser that should handle it.
pub type ParserFactory =
    Arc<dyn Fn(&Path, &ProjectConfig) -> Box<dyn DocumentParser> + Send + Sync>;

/// How many characters of a block's source are carried in an event, so the
/// live view can show what is being worked on without streaming whole blocks.
const PREVIEW_CHARS: usize = 240;

fn preview(text: &str) -> String {
    let trimmed = text.trim();
    match trimmed.char_indices().nth(PREVIEW_CHARS) {
        Some((cut, _)) => format!("{}…", &trimmed[..cut]),
        None => trimmed.to_string(),
    }
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ProgressEvent {
    /// Emitted once before any work starts, carrying run-wide constants.
    RunStarted {
        concurrency: usize,
    },
    /// Emitted once at start: every file with its pending-segment count.
    FilesDiscovered {
        files: Vec<(PathBuf, usize)>,
    },
    FileStarted {
        file: PathBuf,
    },
    /// A block task was spawned and is waiting for a concurrency permit.
    BlockQueued {
        id: u64,
        file: PathBuf,
        segments: usize,
    },
    /// A block acquired a permit; a worker is now on it.
    BlockStarted {
        id: u64,
        file: PathBuf,
        segments: usize,
        source: String,
    },
    /// A translation request was sent to the provider.
    BlockAttempt {
        id: u64,
        attempt: u32,
    },
    /// The provider answered; evaluators are about to run.
    BlockTranslating {
        id: u64,
        attempt: u32,
    },
    /// Evaluators finished. `passed == false` means a retry follows unless
    /// `max_retries` is exhausted.
    BlockEvaluated {
        id: u64,
        attempt: u32,
        passed: bool,
        issues: Vec<String>,
    },
    /// A block finished translating; `segments` segments were saved.
    BlockTranslated {
        id: Option<u64>,
        file: PathBuf,
        segments: usize,
        current: Option<String>,
    },
    /// A block errored out; its segments stay untranslated.
    BlockFailed {
        id: u64,
        file: PathBuf,
        error: String,
    },
    FileCompleted {
        file: PathBuf,
    },
    FileFailed {
        file: PathBuf,
        error: String,
    },
    /// The run was cancelled; already-translated blocks are kept.
    Cancelled,
    Finished {
        errors: usize,
    },
}

/// Cooperative cancellation for a translation run. Cancelling stops further
/// block translations from starting; in-flight requests finish and their
/// results are still saved, so a cancelled run resumes where it stopped.
#[derive(Debug, Clone, Default)]
pub struct CancelToken(Arc<std::sync::atomic::AtomicBool>);

impl CancelToken {
    pub fn cancel(&self) {
        self.0.store(true, std::sync::atomic::Ordering::Relaxed);
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.load(std::sync::atomic::Ordering::Relaxed)
    }
}

pub type ProgressSender = mpsc::UnboundedSender<ProgressEvent>;

/// Monotonic block ids, unique per process, so live-view clients can pair a
/// block's start/attempt/finish events across concurrently running files.
static NEXT_BLOCK_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

fn next_block_id() -> u64 {
    NEXT_BLOCK_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}

#[derive(Debug, Clone)]
pub struct TranslateOptions {
    pub auto_evaluate: bool,
    /// Whether automatic evaluation also calls the optional LLM style judge.
    /// Mechanical evaluators still run when this is false.
    pub style_evaluate: bool,
    pub max_retries: u32,
    /// Maximum concurrent block translation requests across all files.
    pub concurrency: usize,
}

impl TranslateOptions {
    pub fn from_config(config: &ProjectConfig) -> Self {
        let evaluation = config.evaluation.as_ref();
        Self {
            auto_evaluate: evaluation.map(|e| e.auto_evaluate).unwrap_or(true),
            style_evaluate: evaluation.map(|e| e.style_evaluate).unwrap_or(true),
            max_retries: evaluation.map(|e| e.max_retries).unwrap_or(3),
            concurrency: config
                .translation
                .as_ref()
                .map(|t| t.concurrency)
                .unwrap_or_else(yeokja_core::config::default_concurrency),
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
    #[error("Failed to parse {path}: {message}")]
    Parse { path: PathBuf, message: String },
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
            let entries =
                glob::glob(pattern).map_err(|e| OrchestratorError::Glob(e.to_string()))?;
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
                let pattern = format!("{}/{}", source.path.trim_end_matches('/'), source.pattern);
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

/// The fraction of a new file's segment hashes an orphaned state must cover
/// to count as that file's previous life under another name.
const RENAME_OVERLAP_THRESHOLD: f64 = 0.5;

/// An orphaned state file, paired with the collected source file that looks
/// like its rename destination when one exists.
#[derive(Debug)]
pub struct OrphanReport {
    pub orphan: yeokja_core::orphans::OrphanState,
    pub renamed_to: Option<PathBuf>,
}

/// Match orphaned state files against collected files that have no state yet.
/// Pure report — nothing is moved. A match means either the whole-file hash
/// is identical (pure rename) or most segment hashes carry over (rename plus
/// edits).
pub fn match_orphans(
    files: &[PathBuf],
    config: &ProjectConfig,
    parser_factory: &ParserFactory,
) -> Result<Vec<OrphanReport>, OrchestratorError> {
    let orphans = yeokja_core::orphans::find_orphan_states(config);
    if orphans.is_empty() {
        return Ok(Vec::new());
    }

    // Only files with no state of their own can be a rename destination.
    let stateless: Vec<&PathBuf> = files
        .iter()
        .filter(|f| !StateFile::state_file_path(f, config.state_dir()).exists())
        .collect();

    let mut claimed: HashSet<usize> = HashSet::new();
    let mut renamed_to: HashMap<usize, PathBuf> = HashMap::new();

    for file in stateless {
        let Ok(source) = std::fs::read_to_string(file) else {
            continue;
        };
        let file_hash = content_hash(&source);
        let mut segment_hashes: Option<HashSet<u64>> = None;

        let mut best: Option<(usize, f64)> = None;
        for (idx, orphan) in orphans.iter().enumerate() {
            if claimed.contains(&idx) {
                continue;
            }
            let Ok(state) = StateFile::load(&orphan.state_path) else {
                continue;
            };
            if state.source_hash == file_hash {
                best = Some((idx, 1.0));
                break;
            }
            let hashes = if let Some(hashes) = &segment_hashes {
                hashes
            } else {
                let parser = parser_factory(file, config);
                let mut doc =
                    parser
                        .parse_checked(&source)
                        .map_err(|error| OrchestratorError::Parse {
                            path: (*file).clone(),
                            message: error.to_string(),
                        })?;
                apply_table_rules(&mut doc, &config.tables, file);
                let hashes = doc
                    .translatable_segments()
                    .iter()
                    .map(|s| s.source_hash)
                    .collect();
                segment_hashes.insert(hashes)
            };
            if hashes.is_empty() {
                continue;
            }
            let matched = state
                .segments
                .iter()
                .filter(|s| hashes.contains(&s.source_hash))
                .map(|s| s.source_hash)
                .collect::<HashSet<u64>>()
                .len();
            let overlap = matched as f64 / hashes.len() as f64;
            if overlap >= RENAME_OVERLAP_THRESHOLD && best.is_none_or(|(_, prev)| overlap > prev) {
                best = Some((idx, overlap));
            }
        }

        if let Some((idx, _)) = best {
            claimed.insert(idx);
            renamed_to.insert(idx, file.clone());
        }
    }

    Ok(orphans
        .into_iter()
        .enumerate()
        .map(|(idx, orphan)| OrphanReport {
            renamed_to: renamed_to.get(&idx).cloned(),
            orphan,
        })
        .collect())
}

/// Move each matched orphan's state to its rename destination, so the
/// translations carry over instead of being paid for again. Returns the
/// `(from, to)` moves performed. Unmatched orphans are left alone — deleting
/// state is reserved for an explicit clean.
pub fn adopt_renamed_states(
    files: &[PathBuf],
    config: &ProjectConfig,
    parser_factory: &ParserFactory,
) -> Result<Vec<(PathBuf, PathBuf)>, OrchestratorError> {
    let mut moves = Vec::new();
    for report in match_orphans(files, config, parser_factory)? {
        let Some(new_source) = report.renamed_to else {
            continue;
        };
        let dest = StateFile::state_file_path(&new_source, config.state_dir());
        if let Some(parent) = dest.parent()
            && !parent.as_os_str().is_empty()
            && std::fs::create_dir_all(parent).is_err()
        {
            continue;
        }
        match std::fs::rename(&report.orphan.state_path, &dest) {
            Ok(()) => {
                tracing::info!(
                    from = %report.orphan.expected_source.display(),
                    to = %new_source.display(),
                    "Adopted state across a rename"
                );
                moves.push((report.orphan.state_path, dest));
            }
            Err(e) => {
                tracing::warn!(
                    state = %report.orphan.state_path.display(),
                    error = %e,
                    "Failed to adopt orphaned state"
                );
            }
        }
    }
    Ok(moves)
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
    let mut doc = parser
        .parse_checked(&source)
        .map_err(|error| OrchestratorError::Parse {
            path: file_path.to_path_buf(),
            message: error.to_string(),
        })?;
    apply_table_rules(&mut doc, &config.tables, file_path);
    let doc = doc;
    let state_path = StateFile::state_file_path(file_path, config.state_dir());
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
        Box::new(EndingEvaluator),
    ];
    if let Some(provider) = style_provider {
        evaluators.push(Box::new(StyleEvaluator::new(
            provider,
            target_lang.to_string(),
        )));
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
            // `flat_idx` addresses `reconciled`, which is indexed over
            // `Document::translatable_segments`. That filters on the block's
            // own flag, not on its type, so a cell a table rule excluded still
            // has a translatable type and would shift every index after it.
            if !block.translatable {
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

/// Merge adjacent block requests without losing their per-segment identity.
///
/// Starting a CLI-backed provider once per short LaTeX list item costs much
/// more than translating it. The joined context keeps block boundaries visible
/// while the numbered segments and evaluators remain exactly as granular as
/// before. Very large blocks stay alone and the byte cap prevents an otherwise
/// harmless run of tiny segments from producing an unwieldy prompt.
fn batch_block_groups(
    groups: Vec<(String, Vec<(usize, SegmentState)>)>,
    segment_limit: usize,
) -> Vec<(String, Vec<(usize, SegmentState)>)> {
    const CONTEXT_LIMIT: usize = 16 * 1024;
    let segment_limit = segment_limit.max(1);
    if segment_limit == 1 {
        return groups;
    }

    let mut batched = Vec::new();
    let mut context = String::new();
    let mut segments = Vec::new();

    for (next_context, mut next_segments) in groups {
        let separator = if context.is_empty() { 0 } else { 7 }; // "\n\n---\n\n"
        let exceeds_segments =
            !segments.is_empty() && segments.len() + next_segments.len() > segment_limit;
        let exceeds_context =
            !segments.is_empty() && context.len() + separator + next_context.len() > CONTEXT_LIMIT;
        if exceeds_segments || exceeds_context {
            batched.push((std::mem::take(&mut context), std::mem::take(&mut segments)));
        }

        if !context.is_empty() {
            context.push_str("\n\n---\n\n");
        }
        context.push_str(&next_context);
        segments.append(&mut next_segments);
    }

    if !segments.is_empty() {
        batched.push((context, segments));
    }
    batched
}

pub struct Orchestrator {
    pub config: Arc<ProjectConfig>,
    pub glossary: Arc<Glossary>,
    pub provider: Arc<dyn TranslationProvider>,
    /// LLM used by the StyleEvaluator. `None` runs mechanical checks only.
    pub eval_provider: Option<Arc<dyn LlmProvider>>,
    pub parser_factory: ParserFactory,
    pub options: TranslateOptions,
    pub cancel: CancelToken,
}

impl Orchestrator {
    /// Translate every pending segment under `path` and write output files.
    pub async fn translate_path(
        &self,
        path: &Path,
        progress: Option<ProgressSender>,
    ) -> Result<TranslateOutcome, OrchestratorError> {
        let files = collect_files(path, &self.config)?;

        // A renamed source whose translations exist under the old name would
        // otherwise be retranslated from zero.
        let adopted = adopt_renamed_states(&files, &self.config, &self.parser_factory)?;
        if !adopted.is_empty() {
            tracing::info!(
                count = adopted.len(),
                "Adopted orphaned state across renames"
            );
        }

        // Pre-scan for pending counts so front-ends can show totals up front.
        send(
            &progress,
            ProgressEvent::RunStarted {
                concurrency: self.options.concurrency.max(1),
            },
        );

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
                let result = this.translate_file(&file_path, &semaphore, &progress).await;
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

        if self.cancel.is_cancelled() {
            send(&progress, ProgressEvent::Cancelled);
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
            cancel: self.cancel.clone(),
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
    cancel: CancelToken,
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
        if self.cancel.is_cancelled() {
            return Ok(0);
        }

        let parser = (self.parser_factory)(file_path, &self.config);
        let source = std::fs::read_to_string(file_path).map_err(|e| OrchestratorError::Io {
            path: file_path.to_path_buf(),
            source: e,
        })?;
        let mut doc = parser
            .parse_checked(&source)
            .map_err(|error| OrchestratorError::Parse {
                path: file_path.to_path_buf(),
                message: error.to_string(),
            })?;
        let excluded = apply_table_rules(&mut doc, &self.config.tables, file_path);
        if excluded > 0 {
            tracing::debug!(file = %file_path.display(), excluded, "Table rules excluded cells");
        }
        let doc = doc;
        let state_path = StateFile::state_file_path(file_path, self.config.state_dir());

        let existing = if state_path.exists() {
            StateFile::load(&state_path)?
        } else {
            StateFile::new(0)
        };

        let reconciled = reconcile_with_status(&doc, &existing, &self.glossary);
        let block_groups = batch_block_groups(
            group_by_block(&doc, &reconciled),
            self.config
                .translation
                .as_ref()
                .map(|translation| translation.batch_segments)
                .unwrap_or_else(yeokja_core::config::default_batch_segments),
        );

        let output_path = resolve_output_path(file_path, &self.config);
        if block_groups.is_empty() {
            tracing::info!(file = %file_path.display(), "Up to date");
            // Reconstruct even so. Nothing else ever rewrites an output whose
            // translations are all done, so a parser that has since learned to
            // read a construct would leave the old rendering in place forever.
            // `write_output` leaves the file alone when the text is unchanged.
            let segments: Vec<SegmentState> = reconciled.into_iter().map(|rs| rs.state).collect();
            self.write_output(&*parser, &doc, &segments, file_path, &output_path)?;
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
        let (tx, mut rx) = mpsc::channel::<(u64, Vec<SegmentUpdate>)>(64);
        let source_hash = content_hash(&source);
        let writer_segments = Arc::new(Mutex::new(initial_segments));
        let writer_segments_for_task = writer_segments.clone();
        let state_path_clone = state_path.clone();
        let progress_clone = progress.clone();
        let file_path_buf = file_path.to_path_buf();

        let state_writer = tokio::spawn(async move {
            let mut translated_count = 0usize;
            while let Some((block_id, updates)) = rx.recv().await {
                let mut segs = writer_segments_for_task.lock().await;
                let current = updates.first().map(|u| u.translation.clone());
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
                        id: Some(block_id),
                        file: file_path_buf.clone(),
                        segments: updates.len(),
                        current,
                    },
                );
            }
            translated_count
        });

        // Translate blocks concurrently, bounded by the shared semaphore. Every
        // block task is spawned up front and then queues on `acquire()`, so the
        // permit is the moment a worker picks the block up.
        let mut handles = Vec::new();
        let markup = parser.markup();
        for (block_context, block_segments) in block_groups {
            let this = FileTranslator {
                config: self.config.clone(),
                glossary: self.glossary.clone(),
                provider: self.provider.clone(),
                eval_provider: self.eval_provider.clone(),
                parser_factory: self.parser_factory.clone(),
                options: self.options.clone(),
                cancel: self.cancel.clone(),
            };
            let tx = tx.clone();
            let semaphore = semaphore.clone();
            let progress = progress.clone();
            let block_id = next_block_id();
            let file = file_path.to_path_buf();

            send(
                &progress,
                ProgressEvent::BlockQueued {
                    id: block_id,
                    file: file.clone(),
                    segments: block_segments.len(),
                },
            );

            handles.push(tokio::spawn(async move {
                let _permit = semaphore.acquire().await.expect("semaphore closed");
                if this.cancel.is_cancelled() {
                    return;
                }
                send(
                    &progress,
                    ProgressEvent::BlockStarted {
                        id: block_id,
                        file: file.clone(),
                        segments: block_segments.len(),
                        source: preview(&block_context),
                    },
                );
                match this
                    .translate_block(block_id, &block_context, &block_segments, markup, &progress)
                    .await
                {
                    Ok(updates) => {
                        let _ = tx.send((block_id, updates)).await;
                    }
                    Err(e) => {
                        tracing::error!(error = %e, "Block translation failed");
                        send(
                            &progress,
                            ProgressEvent::BlockFailed {
                                id: block_id,
                                file,
                                error: e.to_string(),
                            },
                        );
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
        block_id: u64,
        block_context: &str,
        block_segments: &[(usize, SegmentState)],
        markup: Markup,
        progress: &Option<ProgressSender>,
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
            markup,
            feedback: None,
            prompt_template: self.config.provider.prompt_template.clone(),
        };

        let translations: Vec<(usize, String, Vec<String>)> = if self.options.auto_evaluate {
            let evaluators =
                standard_evaluators(self.eval_provider.clone(), &self.config.project.target_lang);
            let evaluator_refs: Vec<&dyn TranslationEvaluator> =
                evaluators.iter().map(|e| e.as_ref()).collect();

            let observer = |event: crate::pipeline::PipelineEvent| {
                use crate::pipeline::PipelineEvent as Pe;
                let progress_event = match event {
                    Pe::AttemptStarted { attempt } => ProgressEvent::BlockAttempt {
                        id: block_id,
                        attempt,
                    },
                    Pe::Translated { attempt } => ProgressEvent::BlockTranslating {
                        id: block_id,
                        attempt,
                    },
                    Pe::Evaluated {
                        attempt,
                        passed,
                        issues,
                    } => ProgressEvent::BlockEvaluated {
                        id: block_id,
                        attempt,
                        passed,
                        issues,
                    },
                };
                send(progress, progress_event);
            };

            translate_with_evaluation_observed(
                self.provider.as_ref(),
                &evaluator_refs,
                request,
                &glossary_terms,
                &self.config.project.source_lang,
                &self.config.project.target_lang,
                markup,
                self.options.max_retries,
                &observer,
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
        // Rewriting identical bytes would churn the mtime of every output on
        // every run, and this is called whether or not anything was translated.
        if std::fs::read_to_string(output_path).is_ok_and(|on_disk| on_disk == output_text) {
            tracing::debug!(file = %output_path.display(), "Output already current");
            return Ok(());
        }
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
    use crate::provider::{TranslateRequest, TranslateResponse};
    use async_trait::async_trait;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use yeokja_core::model::{Block, BlockRole, BlockType, Section, Segment, SegmentId};

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

    /// A block a table rule excluded still has a translatable *type*. Walking
    /// by type here while `reconciled` is indexed by the block's own flag
    /// shifts every index past the exclusion, so the blocks that actually need
    /// translating are never grouped and the file reports itself up to date.
    #[test]
    fn an_excluded_block_does_not_shift_the_segments_after_it() {
        fn block(text: &str, index: usize, translatable: bool) -> Block {
            Block {
                block_type: BlockType::Table,
                segments: vec![Segment {
                    id: SegmentId::new(0, index, 0),
                    source: text.to_string(),
                    source_hash: yeokja_core::hash::content_hash(text),
                    block_type: BlockType::Table,
                }],
                raw_content: text.to_string(),
                heading_level: None,
                span: None,
                role: BlockRole::None,
                translatable,
            }
        }

        let doc = Document {
            sections: vec![Section {
                blocks: vec![
                    block("keep as-is", 0, false),
                    block("translate me", 1, true),
                ],
            }],
            source: String::new(),
        };

        // One reconciled entry, matching the single translatable segment.
        let glossary = Glossary::from_toml("").unwrap();
        let reconciled = reconcile_with_status(&doc, &StateFile::new(0), &glossary);
        assert_eq!(reconciled.len(), 1);

        let groups = group_by_block(&doc, &reconciled);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].0, "translate me");
        assert_eq!(groups[0].1[0].0, 0, "index must address `reconciled`");
    }

    #[test]
    fn adjacent_block_groups_are_batched_without_losing_indices() {
        let doc = Document {
            sections: vec![Section {
                blocks: (0..5)
                    .map(|index| Block {
                        block_type: BlockType::Paragraph,
                        segments: vec![Segment {
                            id: SegmentId::new(0, index, 0),
                            source: format!("Sentence {index}."),
                            source_hash: index as u64,
                            block_type: BlockType::Paragraph,
                        }],
                        raw_content: format!("Context {index}."),
                        heading_level: None,
                        span: None,
                        role: BlockRole::None,
                        translatable: true,
                    })
                    .collect(),
            }],
            source: String::new(),
        };
        let glossary = Glossary::from_toml("").unwrap();
        let reconciled = reconcile_with_status(&doc, &StateFile::new(0), &glossary);
        let groups = batch_block_groups(group_by_block(&doc, &reconciled), 2);
        assert_eq!(groups.len(), 3);
        assert_eq!(
            groups[0]
                .1
                .iter()
                .map(|(index, _)| *index)
                .collect::<Vec<_>>(),
            [0, 1]
        );
        assert_eq!(
            groups[1]
                .1
                .iter()
                .map(|(index, _)| *index)
                .collect::<Vec<_>>(),
            [2, 3]
        );
        assert_eq!(
            groups[2]
                .1
                .iter()
                .map(|(index, _)| *index)
                .collect::<Vec<_>>(),
            [4]
        );
        assert!(groups[0].0.contains("Context 0.\n\n---\n\nContext 1."));
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

    /// Counts translate calls and echoes segments back.
    struct CountingProvider(Arc<AtomicUsize>);

    #[async_trait]
    impl TranslationProvider for CountingProvider {
        async fn translate(
            &self,
            request: TranslateRequest,
        ) -> Result<TranslateResponse, crate::provider::TranslateError> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Ok(TranslateResponse {
                translations: request
                    .segments
                    .iter()
                    .map(|(idx, text)| (*idx, format!("KO:{text}")))
                    .collect(),
                usage: None,
            })
        }
    }

    /// Minimal parser: the whole source is one paragraph block.
    struct OneBlockParser;

    impl DocumentParser for OneBlockParser {
        fn markup(&self) -> Markup {
            Markup::Markdown
        }

        fn parse(&self, source: &str) -> Document {
            let text = source.trim().to_string();
            let segment = Segment {
                id: SegmentId::new(0, 0, 0),
                source_hash: content_hash(&text),
                source: text,
                block_type: BlockType::Paragraph,
            };
            Document {
                sections: vec![Section {
                    blocks: vec![Block {
                        block_type: BlockType::Paragraph,
                        segments: vec![segment],
                        raw_content: source.to_string(),
                        heading_level: None,
                        span: Some(0..source.len()),
                        translatable: true,
                        role: BlockRole::None,
                    }],
                }],
                source: source.to_string(),
            }
        }

        fn reconstruct(&self, document: &Document, translations: &TranslationMap) -> String {
            let seg = &document.sections[0].blocks[0].segments[0];
            translations
                .get(&seg.id)
                .cloned()
                .unwrap_or_else(|| document.source.clone())
        }
    }

    fn test_orchestrator(dir: &Path, calls: Arc<AtomicUsize>, cancel: CancelToken) -> Orchestrator {
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
            dir.display()
        ));
        Orchestrator {
            config: Arc::new(config),
            glossary: Arc::new(Glossary::empty()),
            provider: Arc::new(CountingProvider(calls)),
            eval_provider: None,
            parser_factory: Arc::new(|_, _| Box::new(OneBlockParser)),
            options: TranslateOptions {
                auto_evaluate: false,
                style_evaluate: false,
                max_retries: 0,
                concurrency: 2,
            },
            cancel,
        }
    }

    #[tokio::test]
    async fn translate_path_translates_pending_segments() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("ch1.md"), "Hello.").unwrap();

        let calls = Arc::new(AtomicUsize::new(0));
        let orchestrator = test_orchestrator(dir.path(), calls.clone(), CancelToken::default());
        let outcome = orchestrator.translate_path(dir.path(), None).await.unwrap();

        assert_eq!(outcome.segments_translated, 1);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn a_renamed_file_reuses_its_translations_instead_of_repaying() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("ch1.md"), "Hello.").unwrap();

        let calls = Arc::new(AtomicUsize::new(0));
        let orchestrator = test_orchestrator(dir.path(), calls.clone(), CancelToken::default());
        orchestrator.translate_path(dir.path(), None).await.unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 1);

        // Upstream renames the chapter; the derived output is cleaned up.
        std::fs::rename(dir.path().join("ch1.md"), dir.path().join("ch2.md")).unwrap();
        std::fs::remove_file(dir.path().join("ch1.ko.md")).unwrap();

        let outcome = orchestrator.translate_path(dir.path(), None).await.unwrap();
        assert_eq!(outcome.segments_translated, 0);
        assert_eq!(
            calls.load(Ordering::SeqCst),
            1,
            "no retranslation after rename"
        );
        assert!(dir.path().join("ch2.md.yeokja.json").exists());
        assert!(!dir.path().join("ch1.md.yeokja.json").exists());
        assert_eq!(
            std::fs::read_to_string(dir.path().join("ch2.ko.md")).unwrap(),
            "KO:Hello."
        );
    }

    /// One segment per line, so rename detection has hashes to overlap.
    struct LineParser;

    impl DocumentParser for LineParser {
        fn markup(&self) -> Markup {
            Markup::Markdown
        }

        fn parse(&self, source: &str) -> Document {
            let blocks = source
                .lines()
                .enumerate()
                .map(|(i, line)| Block {
                    block_type: BlockType::Paragraph,
                    segments: vec![Segment {
                        id: SegmentId::new(0, i, 0),
                        source_hash: content_hash(line),
                        source: line.to_string(),
                        block_type: BlockType::Paragraph,
                    }],
                    raw_content: line.to_string(),
                    heading_level: None,
                    span: None,
                    role: BlockRole::None,
                    translatable: true,
                })
                .collect();
            Document {
                sections: vec![Section { blocks }],
                source: source.to_string(),
            }
        }

        fn reconstruct(&self, document: &Document, _translations: &TranslationMap) -> String {
            document.source.clone()
        }
    }

    #[test]
    fn a_rename_with_edits_still_matches_by_segment_overlap() {
        let dir = tempfile::tempdir().unwrap();
        let old_lines = ["One.", "Two.", "Three.", "Four."];
        // Renamed and edited: three of four lines survive.
        std::fs::write(dir.path().join("new.md"), "One.\nTwo.\nThree.\nCHANGED.").unwrap();

        let mut state = StateFile::new(content_hash("does-not-match-whole-file"));
        for (i, line) in old_lines.iter().enumerate() {
            state.segments.push(SegmentState {
                id: yeokja_core::model::SegmentId::new(0, i, 0),
                source: line.to_string(),
                source_hash: content_hash(line),
                context_hash: 0,
                translation: Some(format!("KO:{line}")),
                glossary_snapshot: HashMap::new(),
                translated_at: None,
                issues: Vec::new(),
            });
        }
        state.save(&dir.path().join("old.md.yeokja.json")).unwrap();

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
            dir.path().display()
        ));
        let factory: ParserFactory = Arc::new(|_, _| Box::new(LineParser));

        let files = vec![dir.path().join("new.md")];
        let reports = match_orphans(&files, &config, &factory).unwrap();
        assert_eq!(reports.len(), 1);
        assert_eq!(reports[0].renamed_to.as_deref(), Some(files[0].as_path()));

        let moves = adopt_renamed_states(&files, &config, &factory).unwrap();
        assert_eq!(moves.len(), 1);
        assert!(dir.path().join("new.md.yeokja.json").exists());
        assert!(!dir.path().join("old.md.yeokja.json").exists());
    }

    #[tokio::test]
    async fn cancelled_run_translates_nothing() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("ch1.md"), "Hello.").unwrap();

        let calls = Arc::new(AtomicUsize::new(0));
        let cancel = CancelToken::default();
        cancel.cancel();
        let orchestrator = test_orchestrator(dir.path(), calls.clone(), cancel);
        let outcome = orchestrator.translate_path(dir.path(), None).await.unwrap();

        assert_eq!(outcome.segments_translated, 0);
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }
}
