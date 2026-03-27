use std::sync::Arc;
use tokio::sync::RwLock;
use yeokja_core::config::ProjectConfig;
use yeokja_core::glossary::Glossary;

pub struct AppState {
    pub config: ProjectConfig,
    pub glossary: Arc<RwLock<Glossary>>,
}
