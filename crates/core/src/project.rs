use crate::config::ProjectConfig;
use crate::glossary::Glossary;
use std::path::Path;

pub struct ProjectContext {
    pub config: ProjectConfig,
    pub glossary: Glossary,
}

#[derive(Debug, thiserror::Error)]
pub enum ProjectError {
    #[error("yeokja.toml not found in current directory")]
    ConfigNotFound,
    #[error("Config error: {0}")]
    Config(#[from] crate::config::ConfigError),
    #[error("Glossary error: {0}")]
    Glossary(#[from] crate::glossary::GlossaryError),
}

impl ProjectContext {
    pub fn load() -> Result<Self, ProjectError> {
        let config_path = Path::new("yeokja.toml");
        if !config_path.exists() {
            return Err(ProjectError::ConfigNotFound);
        }
        let config = ProjectConfig::load(config_path)?;

        let glossary_path = Path::new(&config.project.glossary);
        let glossary = if glossary_path.exists() {
            Glossary::load(glossary_path)?
        } else {
            Glossary::empty()
        };

        Ok(Self { config, glossary })
    }
}
