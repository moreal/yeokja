use chrono::{DateTime, Utc};
use serde::Serialize;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::{broadcast, Mutex, RwLock};
use yeokja_core::config::ProjectConfig;
use yeokja_core::glossary::Glossary;
use yeokja_translate::orchestrator::CancelToken;

pub struct AppState {
    pub config: Arc<ProjectConfig>,
    pub glossary: Arc<RwLock<Glossary>>,
    pub glossary_path: PathBuf,
    pub job: Arc<Mutex<TranslationJob>>,
    /// Cancel token of the currently running translation, if any.
    pub cancel: Mutex<Option<CancelToken>>,
    /// Pre-serialized progress events fanned out to SSE subscribers.
    pub events: broadcast::Sender<String>,
}

/// What a worker is currently doing with a block.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BlockPhase {
    /// Waiting on the provider to answer.
    Translating,
    /// Provider answered; evaluators are running.
    Evaluating,
}

/// A block currently held by a worker. Mirrors what the SSE stream reports, so
/// a client that connects mid-run can render the live view without replaying
/// the whole event log.
#[derive(Debug, Clone, Serialize)]
pub struct ActiveBlock {
    pub id: u64,
    pub file: PathBuf,
    pub segments: usize,
    pub source: String,
    pub attempt: u32,
    pub phase: BlockPhase,
    pub started_at: DateTime<Utc>,
}

/// Progress of the current (or last) server-triggered translation run.
#[derive(Debug, Clone, Default, Serialize)]
pub struct TranslationJob {
    pub running: bool,
    pub cancelled: bool,
    pub started_at: Option<DateTime<Utc>>,
    pub finished_at: Option<DateTime<Utc>>,
    pub files_total: usize,
    pub files_done: usize,
    pub segments_total: usize,
    pub segments_done: usize,
    pub errors: Vec<String>,
    /// Number of worker slots (semaphore permits) for this run.
    pub concurrency: usize,
    /// Blocks spawned but still waiting for a permit.
    pub queued: usize,
    /// Blocks currently held by a worker, newest first.
    pub active: Vec<ActiveBlock>,
    /// Blocks that needed at least one retry after a failed evaluation.
    pub retried: usize,
}

impl AppState {
    pub fn new(config: ProjectConfig, glossary: Glossary) -> Self {
        let glossary_path = PathBuf::from(&config.project.glossary);
        let (events, _) = broadcast::channel(256);
        Self {
            config: Arc::new(config),
            glossary: Arc::new(RwLock::new(glossary)),
            glossary_path,
            job: Arc::new(Mutex::new(TranslationJob::default())),
            cancel: Mutex::new(None),
            events,
        }
    }

    /// Reload the glossary from disk into the shared state.
    pub async fn reload_glossary(&self) -> Result<(), yeokja_core::glossary::GlossaryError> {
        let glossary = if self.glossary_path.exists() {
            Glossary::load(&self.glossary_path)?
        } else {
            Glossary::empty()
        };
        *self.glossary.write().await = glossary;
        Ok(())
    }
}
