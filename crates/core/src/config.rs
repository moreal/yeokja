use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectConfig {
    pub project: ProjectSettings,
    #[serde(default)]
    pub sources: Vec<SourceConfig>,
    pub provider: ProviderConfig,
    #[serde(default)]
    pub server: Option<ServerConfig>,
    #[serde(default)]
    pub evaluation: Option<EvaluationConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectSettings {
    pub source_lang: String,
    pub target_lang: String,
    #[serde(default = "default_glossary_path")]
    pub glossary: String,
}

fn default_glossary_path() -> String {
    "glossary.toml".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceConfig {
    pub path: String,
    pub pattern: String,
    pub parser: String,
    pub output: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderConfig {
    #[serde(rename = "type")]
    pub provider_type: String,
    pub model: String,
    #[serde(default)]
    pub api_key_env: Option<String>,
    #[serde(default)]
    pub base_url: Option<String>,
    #[serde(default)]
    pub prompt_template: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    #[serde(default = "default_port")]
    pub port: u16,
}

fn default_port() -> u16 {
    3000
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvaluationConfig {
    #[serde(default = "default_max_retries")]
    pub max_retries: u32,
    #[serde(default = "default_auto_evaluate")]
    pub auto_evaluate: bool,
}

fn default_max_retries() -> u32 { 3 }
fn default_auto_evaluate() -> bool { true }

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Parse error: {0}")]
    Parse(#[from] toml::de::Error),
}

impl ProjectConfig {
    pub fn load(path: &Path) -> Result<Self, ConfigError> {
        let content = std::fs::read_to_string(path)?;
        Self::from_toml(&content)
    }

    pub fn from_toml(content: &str) -> Result<Self, ConfigError> {
        Ok(toml::from_str(content)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_full_config() {
        let toml = r#"
[project]
source_lang = "en"
target_lang = "ko"
glossary = "glossary.toml"

[[sources]]
path = "book/"
pattern = "**/*.md"
parser = "markdown"
output = "{dir}/{stem}.ko{ext}"

[provider]
type = "anthropic"
model = "claude-sonnet-4-20250514"
api_key_env = "ANTHROPIC_API_KEY"

[server]
port = 8080

[evaluation]
max_retries = 5
auto_evaluate = false
"#;
        let config = ProjectConfig::from_toml(toml).unwrap();
        assert_eq!(config.project.source_lang, "en");
        assert_eq!(config.project.target_lang, "ko");
        assert_eq!(config.sources.len(), 1);
        assert_eq!(config.sources[0].parser, "markdown");
        assert_eq!(config.provider.provider_type, "anthropic");
        assert_eq!(config.server.unwrap().port, 8080);
        assert_eq!(config.evaluation.unwrap().max_retries, 5);
    }

    #[test]
    fn parse_minimal_config() {
        let toml = r#"
[project]
source_lang = "en"
target_lang = "ko"

[provider]
type = "openai_compatible"
model = "gpt-4o"
"#;
        let config = ProjectConfig::from_toml(toml).unwrap();
        assert_eq!(config.project.glossary, "glossary.toml");
        assert!(config.sources.is_empty());
        assert!(config.server.is_none());
    }
}
