use axum::{
    extract::State,
    http::StatusCode,
    routing::{get, post, put},
    Json, Router,
};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::Arc;
use tower_http::cors::CorsLayer;
use yeokja_core::change::SegmentStatus;
use yeokja_core::config::ProjectConfig;
use yeokja_core::parser::DocumentParser;
use yeokja_core::reconcile::{reconcile, reconcile_with_status};
use yeokja_core::state::StateFile;

use crate::state::AppState;

pub fn create_router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/api/status", get(get_status))
        .route("/api/segments", get(get_segments))
        .route("/api/segments/{file}/{segment_id}", put(update_segment))
        .route("/api/glossary", get(get_glossary))
        .route("/api/glossary", post(add_glossary_term))
        .route("/api/translate/start", post(start_translation))
        .layer(CorsLayer::permissive())
        .with_state(state)
}

/// Select the appropriate parser based on the source config or file extension.
fn get_parser(file_path: &Path, config: &ProjectConfig) -> Box<dyn DocumentParser + Send + Sync> {
    for source in &config.sources {
        let source_dir = Path::new(&source.path);
        if file_path.starts_with(source_dir) {
            return match source.parser.as_str() {
                "asciidoc" => Box::new(yeokja_parser_asciidoc::AsciidocParser),
                _ => Box::new(yeokja_parser_markdown::MarkdownParser),
            };
        }
    }
    match file_path.extension().and_then(|e| e.to_str()) {
        Some("adoc" | "asciidoc" | "asc") => Box::new(yeokja_parser_asciidoc::AsciidocParser),
        _ => Box::new(yeokja_parser_markdown::MarkdownParser),
    }
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

async fn get_status(
    State(state): State<Arc<AppState>>,
) -> Result<Json<StatusResponse>, StatusCode> {
    let glossary = state.glossary.read().await;

    let mut total = 0usize;
    let mut translated = 0usize;
    let mut pending = 0usize;
    let mut stale = 0usize;
    let mut glossary_stale_count = 0usize;
    let mut context_changed_count = 0usize;
    let mut file_count = 0usize;

    for source_config in &state.config.sources {
        let pattern = format!("{}/{}", source_config.path, source_config.pattern);
        let entries = glob::glob(&pattern).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

        for entry in entries.flatten() {
            file_count += 1;
            let parser = get_parser(&entry, &state.config);
            let source = tokio::fs::read_to_string(&entry)
                .await
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
            let doc = parser.parse(&source);
            let state_path = StateFile::state_file_path(&entry);

            let existing = if state_path.exists() {
                StateFile::load(&state_path).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
            } else {
                StateFile::new(0)
            };

            let reconciled = reconcile_with_status(&doc, &existing, &glossary);

            for rs in &reconciled {
                total += 1;
                match rs.status {
                    SegmentStatus::Translated => translated += 1,
                    SegmentStatus::Pending => pending += 1,
                    SegmentStatus::Stale => stale += 1,
                    SegmentStatus::GlossaryStale => glossary_stale_count += 1,
                    SegmentStatus::ContextChanged => context_changed_count += 1,
                }
            }
        }
    }

    Ok(Json(StatusResponse {
        files: file_count,
        total_segments: total,
        translated,
        pending,
        stale,
        glossary_stale: glossary_stale_count,
        context_changed: context_changed_count,
    }))
}

async fn get_segments(
    State(state): State<Arc<AppState>>,
) -> Result<Json<Vec<SegmentResponse>>, StatusCode> {
    let glossary = state.glossary.read().await;
    let mut responses = Vec::new();

    for source_config in &state.config.sources {
        let pattern = format!("{}/{}", source_config.path, source_config.pattern);
        let entries = glob::glob(&pattern).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

        for entry in entries.flatten() {
            let parser = get_parser(&entry, &state.config);
            let source = tokio::fs::read_to_string(&entry)
                .await
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
            let doc = parser.parse(&source);
            let state_path = StateFile::state_file_path(&entry);

            let existing = if state_path.exists() {
                StateFile::load(&state_path).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
            } else {
                StateFile::new(0)
            };

            let reconciled = reconcile_with_status(&doc, &existing, &glossary);

            for rs in &reconciled {
                responses.push(SegmentResponse {
                    file: entry.display().to_string(),
                    id: rs.state.id.to_string(),
                    source: rs.state.source.clone(),
                    translation: rs.state.translation.clone(),
                    status: format!("{:?}", rs.status),
                });
            }
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
        let parser = get_parser(source_path, &state.config);
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
            Ok(StatusCode::OK)
        }
        None => Err(StatusCode::NOT_FOUND),
    }
}

async fn get_glossary(
    State(state): State<Arc<AppState>>,
) -> Json<Vec<GlossaryTermResponse>> {
    let glossary = state.glossary.read().await;
    let terms: Vec<GlossaryTermResponse> = glossary
        .terms()
        .iter()
        .map(|(term, translation)| GlossaryTermResponse {
            term: term.clone(),
            translation: translation.clone(),
        })
        .collect();
    Json(terms)
}

async fn add_glossary_term(
    State(_state): State<Arc<AppState>>,
    Json(_body): Json<AddGlossaryRequest>,
) -> StatusCode {
    // Not yet implemented: requires persisting to glossary.toml and rebuilding Glossary
    StatusCode::NOT_IMPLEMENTED
}

async fn start_translation(
    State(_state): State<Arc<AppState>>,
) -> StatusCode {
    // TODO: Spawn translation task
    StatusCode::NOT_IMPLEMENTED
}
