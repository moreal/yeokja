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
    #[serde(default)]
    pub translation: Option<TranslationConfig>,
    /// Rules narrowing what gets translated. Everything is translated by
    /// default; each rule only ever removes content from that set.
    #[serde(default)]
    pub tables: Vec<TableRule>,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranslationConfig {
    /// Maximum block translations in flight at once, across all files. Each
    /// permit is held for a block's whole translate-evaluate-retry chain, so
    /// this caps concurrent provider requests rather than CPU use.
    #[serde(default = "default_concurrency")]
    pub concurrency: usize,
}

pub fn default_concurrency() -> usize { 4 }

/// A rule selecting which columns of which tables are translated.
///
/// Tables are matched by the text of their header row, not by position, so a
/// rule keeps working when a table moves or gains rows, and one rule covers
/// every table sharing that schema.
///
/// ```toml
/// [[tables]]
/// files = "chapters/*.asciidoc"
/// headers = ["Instruction", "Arguments", "Explanation"]
/// translate = ["Explanation"]
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TableRule {
    /// Glob restricting the rule to matching source paths. Absent means every
    /// file.
    #[serde(default)]
    pub files: Option<String>,
    /// Header cells identifying the table. A table matches when its first row
    /// contains all of these, in order; extra columns are allowed.
    pub headers: Vec<String>,
    /// Columns to translate, named by header text or by 0-based index. Columns
    /// left out are kept verbatim. Mutually exclusive with `skip`.
    #[serde(default)]
    pub translate: Vec<ColumnRef>,
    /// Columns to keep verbatim; everything else is translated. Mutually
    /// exclusive with `translate`.
    #[serde(default)]
    pub skip: Vec<ColumnRef>,
}

/// A column named either by its header text or by 0-based index.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ColumnRef {
    Index(usize),
    Header(String),
}

impl ColumnRef {
    /// Whether this reference designates the column at `index` headed `header`.
    pub fn matches(&self, index: usize, header: Option<&str>) -> bool {
        match self {
            ColumnRef::Index(i) => *i == index,
            ColumnRef::Header(h) => header.is_some_and(|actual| actual.eq_ignore_ascii_case(h)),
        }
    }
}

impl TableRule {
    /// Whether `headers` (a table's first row) satisfies this rule's matcher.
    pub fn matches_headers(&self, headers: &[String]) -> bool {
        if self.headers.is_empty() {
            return false;
        }
        // Subsequence match in order, so extra columns do not break the rule.
        let mut wanted = self.headers.iter();
        let mut current = wanted.next();
        for actual in headers {
            if let Some(w) = current
                && actual.eq_ignore_ascii_case(w)
            {
                current = wanted.next();
            }
        }
        current.is_none()
    }

    /// Whether the column at `index` headed `header` should be translated.
    pub fn translates(&self, index: usize, header: Option<&str>) -> bool {
        if !self.translate.is_empty() {
            return self.translate.iter().any(|c| c.matches(index, header));
        }
        !self.skip.iter().any(|c| c.matches(index, header))
    }
}

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

[translation]
concurrency = 12
"#;
        let config = ProjectConfig::from_toml(toml).unwrap();
        assert_eq!(config.translation.as_ref().unwrap().concurrency, 12);
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
        assert!(config.translation.is_none());
    }

    #[test]
    fn translation_section_defaults_concurrency() {
        let toml = r#"
[project]
source_lang = "en"
target_lang = "ko"

[provider]
type = "anthropic"
model = "claude-sonnet-5"

[translation]
"#;
        let config = ProjectConfig::from_toml(toml).unwrap();
        assert_eq!(
            config.translation.unwrap().concurrency,
            default_concurrency()
        );
    }
}
