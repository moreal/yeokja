use axum::{
    extract::State,
    http::StatusCode,
    response::sse::{Event, KeepAlive, Sse},
    routing::{delete, get, post, put},
    Json, Router,
};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::convert::Infallible;
use std::path::Path;
use std::sync::Arc;
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::{Stream, StreamExt};
use tower_http::cors::CorsLayer;
use yeokja_core::change::SegmentStatus;
use yeokja_core::glossary::{remove_term_in_file, upsert_term_in_file};
use yeokja_core::reconcile::reconcile;
use yeokja_core::state::StateFile;
use yeokja_translate::evaluator::EvaluationContext;
use yeokja_translate::factory::{create_evaluator_provider, create_provider};
use yeokja_translate::orchestrator::{
    collect_files, evaluate_translation, scan_file, standard_evaluators, CancelToken,
    Orchestrator, ParserFactory, ProgressEvent, TranslateOptions,
};

use crate::state::{ActiveBlock, AppState, BlockPhase, TranslationJob};

pub fn create_router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/api/status", get(get_status))
        .route("/api/segments", get(get_segments))
        .route("/api/segments/{file}/{segment_id}", put(update_segment))
        .route("/api/segments/{file}/{segment_id}/evaluate", post(evaluate_segment))
        .route("/api/glossary", get(get_glossary))
        .route("/api/glossary", post(add_glossary_term))
        .route("/api/glossary/{term}", delete(delete_glossary_term))
        .route("/api/translate/start", post(start_translation))
        .route("/api/translate/cancel", post(cancel_translation))
        .route("/api/translate/status", get(get_translation_status))
        .route("/api/translate/events", get(translate_events))
        .layer(CorsLayer::permissive())
        .with_state(state)
}

fn parser_factory() -> ParserFactory {
    Arc::new(yeokja_parsers::select_parser)
}

#[derive(Serialize)]
struct StatusResponse {
    files: usize,
    total_segments: usize,
    translated: usize,
    pending: usize,
    stale: usize,
    glossary_stale: usize,
    context_changed: usize,
}

#[derive(Serialize)]
struct SegmentResponse {
    file: String,
    id: String,
    source: String,
    translation: Option<String>,
    status: String,
    issues: Vec<String>,
}

#[derive(Serialize)]
struct GlossaryTermResponse {
    term: String,
    translation: String,
}

#[derive(Deserialize)]
pub struct AddGlossaryRequest {
    pub term: String,
    pub translation: String,
}

#[derive(Deserialize)]
pub struct UpdateSegmentRequest {
    pub translation: String,
}

#[derive(Serialize)]
struct ErrorResponse {
    error: String,
}

fn internal_error(message: impl Into<String>) -> (StatusCode, Json<ErrorResponse>) {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(ErrorResponse {
            error: message.into(),
        }),
    )
}

async fn get_status(
    State(state): State<Arc<AppState>>,
) -> Result<Json<StatusResponse>, (StatusCode, Json<ErrorResponse>)> {
    let glossary = state.glossary.read().await;
    let factory = parser_factory();

    let files = collect_files(Path::new("."), &state.config)
        .map_err(|e| internal_error(e.to_string()))?;

    let mut response = StatusResponse {
        files: files.len(),
        total_segments: 0,
        translated: 0,
        pending: 0,
        stale: 0,
        glossary_stale: 0,
        context_changed: 0,
    };

    for entry in &files {
        let (_, reconciled) = scan_file(entry, &state.config, &glossary, &factory)
            .map_err(|e| internal_error(e.to_string()))?;

        for rs in &reconciled {
            response.total_segments += 1;
            match rs.status {
                SegmentStatus::Translated => response.translated += 1,
                SegmentStatus::Pending => response.pending += 1,
                SegmentStatus::Stale => response.stale += 1,
                SegmentStatus::GlossaryStale => response.glossary_stale += 1,
                SegmentStatus::ContextChanged => response.context_changed += 1,
            }
        }
    }

    Ok(Json(response))
}

async fn get_segments(
    State(state): State<Arc<AppState>>,
) -> Result<Json<Vec<SegmentResponse>>, (StatusCode, Json<ErrorResponse>)> {
    let glossary = state.glossary.read().await;
    let factory = parser_factory();

    let files = collect_files(Path::new("."), &state.config)
        .map_err(|e| internal_error(e.to_string()))?;

    let mut responses = Vec::new();
    for entry in &files {
        let (_, reconciled) = scan_file(entry, &state.config, &glossary, &factory)
            .map_err(|e| internal_error(e.to_string()))?;

        for rs in &reconciled {
            responses.push(SegmentResponse {
                file: entry.display().to_string(),
                id: rs.state.id.to_string(),
                source: rs.state.source.clone(),
                translation: rs.state.translation.clone(),
                status: format!("{:?}", rs.status),
                issues: rs.state.issues.clone(),
            });
        }
    }

    Ok(Json(responses))
}

async fn update_segment(
    State(state): State<Arc<AppState>>,
    axum::extract::Path((file, segment_id)): axum::extract::Path<(String, String)>,
    Json(body): Json<UpdateSegmentRequest>,
) -> Result<StatusCode, StatusCode> {
    let source_path = Path::new(&file);
    if !source_path.exists() {
        return Err(StatusCode::NOT_FOUND);
    }

    let state_path = StateFile::state_file_path(source_path);

    // If no state file exists yet, create one by parsing and reconciling
    let mut state_file = if state_path.exists() {
        StateFile::load(&state_path).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    } else {
        let source_text = tokio::fs::read_to_string(source_path)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        let parser = yeokja_parsers::select_parser(source_path, &state.config);
        let doc = parser.parse(&source_text);
        let empty_state = StateFile::new(0);
        let result = reconcile(&doc, &empty_state);
        let mut sf = StateFile::new(yeokja_core::hash::content_hash(&source_text));
        sf.segments = result.segments;
        sf
    };

    // Find segment by ID and update translation
    let segment = state_file
        .segments
        .iter_mut()
        .find(|s| s.id.0 == segment_id);

    match segment {
        Some(seg) => {
            seg.translation = Some(body.translation);
            seg.translated_at = Some(Utc::now());
            state_file
                .save(&state_path)
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

            // Refresh the output file so manual edits show up immediately.
            let parser = yeokja_parsers::select_parser(source_path, &state.config);
            if let Ok(source_text) = std::fs::read_to_string(source_path) {
                let doc = parser.parse(&source_text);
                let mut translations = yeokja_core::parser::TranslationMap::new();
                for seg in &state_file.segments {
                    if let Some(t) = &seg.translation {
                        translations.insert(seg.id.clone(), t.clone());
                    }
                }
                let output_text = parser.reconstruct(&doc, &translations);
                let output_path =
                    yeokja_translate::orchestrator::resolve_output_path(source_path, &state.config);
                if let Some(parent) = output_path.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                let _ = std::fs::write(&output_path, output_text);
            }

            Ok(StatusCode::OK)
        }
        None => Err(StatusCode::NOT_FOUND),
    }
}

async fn get_glossary(
    State(state): State<Arc<AppState>>,
) -> Json<Vec<GlossaryTermResponse>> {
    let glossary = state.glossary.read().await;
    let mut terms: Vec<GlossaryTermResponse> = glossary
        .terms()
        .iter()
        .map(|(term, translation)| GlossaryTermResponse {
            term: term.clone(),
            translation: translation.clone(),
        })
        .collect();
    terms.sort_by(|a, b| a.term.cmp(&b.term));
    Json(terms)
}

async fn add_glossary_term(
    State(state): State<Arc<AppState>>,
    Json(body): Json<AddGlossaryRequest>,
) -> Result<StatusCode, (StatusCode, Json<ErrorResponse>)> {
    let term = body.term.trim();
    let translation = body.translation.trim();
    if term.is_empty() || translation.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "term and translation must not be empty".to_string(),
            }),
        ));
    }

    upsert_term_in_file(&state.glossary_path, term, translation)
        .map_err(|e| internal_error(e.to_string()))?;
    state
        .reload_glossary()
        .await
        .map_err(|e| internal_error(e.to_string()))?;
    Ok(StatusCode::CREATED)
}

async fn delete_glossary_term(
    State(state): State<Arc<AppState>>,
    axum::extract::Path(term): axum::extract::Path<String>,
) -> Result<StatusCode, (StatusCode, Json<ErrorResponse>)> {
    let removed = remove_term_in_file(&state.glossary_path, &term)
        .map_err(|e| internal_error(e.to_string()))?;
    if !removed {
        return Ok(StatusCode::NOT_FOUND);
    }
    state
        .reload_glossary()
        .await
        .map_err(|e| internal_error(e.to_string()))?;
    Ok(StatusCode::NO_CONTENT)
}

async fn get_translation_status(State(state): State<Arc<AppState>>) -> Json<TranslationJob> {
    Json(state.job.lock().await.clone())
}

/// Stream translation progress events as SSE. Events are the JSON-serialized
/// `ProgressEvent`s of the currently running (or next) translation.
async fn translate_events(
    State(state): State<Arc<AppState>>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let rx = state.events.subscribe();
    let stream = BroadcastStream::new(rx).filter_map(|msg| match msg {
        Ok(json) => Some(Ok(Event::default().data(json))),
        // A lagged subscriber just skips missed events.
        Err(_) => None,
    });
    Sse::new(stream).keep_alive(KeepAlive::default())
}

#[derive(Serialize)]
struct EvaluateSegmentResponse {
    passed: bool,
    issues: Vec<yeokja_translate::evaluator::EvaluationIssue>,
}

/// Re-run the evaluators on one translated segment and persist the issues.
async fn evaluate_segment(
    State(state): State<Arc<AppState>>,
    axum::extract::Path((file, segment_id)): axum::extract::Path<(String, String)>,
) -> Result<Json<EvaluateSegmentResponse>, (StatusCode, Json<ErrorResponse>)> {
    let source_path = Path::new(&file);
    let state_path = StateFile::state_file_path(source_path);
    if !state_path.exists() {
        return Err((
            StatusCode::NOT_FOUND,
            Json(ErrorResponse {
                error: "no translation state for this file".to_string(),
            }),
        ));
    }

    let mut state_file = StateFile::load(&state_path).map_err(|e| internal_error(e.to_string()))?;
    let segment = state_file
        .segments
        .iter_mut()
        .find(|s| s.id.0 == segment_id)
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(ErrorResponse {
                    error: "segment not found".to_string(),
                }),
            )
        })?;

    let translation = segment.translation.clone().ok_or_else(|| {
        (
            StatusCode::CONFLICT,
            Json(ErrorResponse {
                error: "segment has no translation to evaluate".to_string(),
            }),
        )
    })?;

    // Style evaluation needs a provider; fall back to mechanical checks when
    // the configured provider cannot act as a judge (e.g. missing API key).
    let eval_provider = create_evaluator_provider(&state.config.provider).unwrap_or_else(|e| {
        tracing::warn!(error = %e, "Evaluator provider unavailable; running mechanical checks only");
        None
    });
    let evaluators = standard_evaluators(eval_provider, &state.config.project.target_lang);

    let glossary = state.glossary.read().await.terms().clone();
    let context = EvaluationContext {
        source: segment.source.clone(),
        translation,
        glossary,
        source_lang: state.config.project.source_lang.clone(),
        target_lang: state.config.project.target_lang.clone(),
    };

    let result = evaluate_translation(&evaluators, &context).await;

    segment.issues = result.issues.iter().map(|i| i.message.clone()).collect();
    state_file
        .save(&state_path)
        .map_err(|e| internal_error(e.to_string()))?;

    Ok(Json(EvaluateSegmentResponse {
        passed: result.passed,
        issues: result.issues,
    }))
}

async fn start_translation(
    State(state): State<Arc<AppState>>,
) -> Result<StatusCode, (StatusCode, Json<ErrorResponse>)> {
    {
        let mut job = state.job.lock().await;
        if job.running {
            return Err((
                StatusCode::CONFLICT,
                Json(ErrorResponse {
                    error: "translation already running".to_string(),
                }),
            ));
        }
        *job = TranslationJob {
            running: true,
            started_at: Some(Utc::now()),
            ..TranslationJob::default()
        };
    }

    let options = TranslateOptions::from_config(&state.config);
    let provider = match create_provider(&state.config.provider) {
        Ok(p) => p,
        Err(e) => {
            state.job.lock().await.running = false;
            return Err(internal_error(e.to_string()));
        }
    };
    let eval_provider = if options.auto_evaluate {
        match create_evaluator_provider(&state.config.provider) {
            Ok(p) => p,
            Err(e) => {
                state.job.lock().await.running = false;
                return Err(internal_error(e.to_string()));
            }
        }
    } else {
        None
    };

    let cancel = CancelToken::default();
    *state.cancel.lock().await = Some(cancel.clone());

    let orchestrator = Orchestrator {
        config: state.config.clone(),
        glossary: Arc::new(state.glossary.read().await.clone()),
        provider,
        eval_provider,
        parser_factory: parser_factory(),
        options,
        cancel,
    };

    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<ProgressEvent>();

    // Progress consumer keeps the job snapshot up to date for
    // /api/translate/status and fans events out to SSE subscribers.
    let job_for_events = state.job.clone();
    let sse_events = state.events.clone();
    tokio::spawn(async move {
        while let Some(event) = rx.recv().await {
            if let Ok(json) = serde_json::to_string(&event) {
                let _ = sse_events.send(json);
            }
            let mut job = job_for_events.lock().await;
            match event {
                ProgressEvent::RunStarted { concurrency } => {
                    job.concurrency = concurrency;
                }
                ProgressEvent::FilesDiscovered { files } => {
                    job.files_total = files.len();
                    job.segments_total = files.iter().map(|(_, pending)| pending).sum();
                }
                ProgressEvent::BlockQueued { .. } => {
                    job.queued += 1;
                }
                ProgressEvent::BlockStarted {
                    id,
                    file,
                    segments,
                    source,
                } => {
                    job.queued = job.queued.saturating_sub(1);
                    job.active.insert(
                        0,
                        ActiveBlock {
                            id,
                            file,
                            segments,
                            source,
                            attempt: 1,
                            phase: BlockPhase::Translating,
                            started_at: Utc::now(),
                        },
                    );
                }
                ProgressEvent::BlockAttempt { id, attempt } => {
                    let mut is_retry = false;
                    if let Some(block) = job.active.iter_mut().find(|b| b.id == id) {
                        is_retry = attempt > block.attempt;
                        block.attempt = attempt;
                        block.phase = BlockPhase::Translating;
                    }
                    if is_retry {
                        job.retried += 1;
                    }
                }
                ProgressEvent::BlockTranslating { id, .. } => {
                    if let Some(block) = job.active.iter_mut().find(|b| b.id == id) {
                        block.phase = BlockPhase::Evaluating;
                    }
                }
                ProgressEvent::BlockEvaluated { .. } => {}
                ProgressEvent::BlockTranslated { id, segments, .. } => {
                    job.segments_done += segments;
                    if let Some(id) = id {
                        job.active.retain(|b| b.id != id);
                    }
                }
                ProgressEvent::BlockFailed { id, file, error } => {
                    job.active.retain(|b| b.id != id);
                    job.errors.push(format!("{}: {error}", file.display()));
                }
                ProgressEvent::FileCompleted { .. } => {
                    job.files_done += 1;
                }
                ProgressEvent::FileFailed { file, error } => {
                    job.files_done += 1;
                    job.errors.push(format!("{}: {error}", file.display()));
                }
                ProgressEvent::FileStarted { .. } => {}
                ProgressEvent::Cancelled => {}
                ProgressEvent::Finished { .. } => {
                    job.queued = 0;
                    job.active.clear();
                }
            }
        }
    });

    let job_for_run = state.job.clone();
    tokio::spawn(async move {
        let result = orchestrator.translate_path(Path::new("."), Some(tx)).await;
        let mut job = job_for_run.lock().await;
        job.running = false;
        job.finished_at = Some(Utc::now());
        if let Err(e) = result {
            job.errors.push(e.to_string());
        }
    });

    Ok(StatusCode::ACCEPTED)
}

/// Cancel the running translation. Blocks already in flight finish and are
/// saved; the rest are skipped, so a later start resumes from there.
async fn cancel_translation(
    State(state): State<Arc<AppState>>,
) -> Result<StatusCode, (StatusCode, Json<ErrorResponse>)> {
    let mut job = state.job.lock().await;
    if !job.running {
        return Err((
            StatusCode::CONFLICT,
            Json(ErrorResponse {
                error: "no translation running".to_string(),
            }),
        ));
    }
    if let Some(token) = state.cancel.lock().await.as_ref() {
        token.cancel();
    }
    job.cancelled = true;
    Ok(StatusCode::ACCEPTED)
}
