use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
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
    #[serde(default)]
    pub derive: Option<DeriveConfig>,
    #[serde(default)]
    pub build: Option<BuildSection>,
}

/// How the buildable tree is derived: a base layer (typically an upstream
/// submodule) with overlays stacked on top, then patch/generate steps for
/// whatever a pure overlay cannot express. The tree is disposable — it is
/// assembled from scratch every time and atomically swapped into place.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeriveConfig {
    /// Layer 0, linked in verbatim (minus `.git`).
    pub base: String,
    /// Where the assembled tree lands.
    #[serde(default = "default_derive_target")]
    pub target: String,
    /// Later overlays win over earlier ones and over the base.
    #[serde(default)]
    pub overlay: Vec<OverlayConfig>,
    /// Run in order after the overlays, inside the tree.
    #[serde(default)]
    pub step: Vec<DeriveStep>,
}

fn default_derive_target() -> String {
    "build/tree".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OverlayConfig {
    pub path: String,
    /// Only overlay files whose base counterpart exists. The safety net for
    /// translation mirrors: an output whose source upstream deleted is skipped
    /// and reported instead of haunting the tree.
    #[serde(default)]
    pub require_base: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DeriveStep {
    /// Apply a unified diff (project-root-relative path) with `git apply`.
    /// Files it touches are materialized from links into real copies first,
    /// so a patch can never write through into an overlay or the base.
    Patch { file: String },
    /// Run a shell command with the tree as its working directory and
    /// `YEOKJA_ROOT` pointing back at the project root.
    Generate { command: String },
}

/// The `[build]` section: one anonymous target, or several named ones
/// (`[build.html]`, `[build.pdf]`) selected with `yeokja build <name>`.
/// One tree serves them all — each target is just a different command run
/// inside it.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum BuildSection {
    /// `[build]` carrying `command` directly.
    Single(BuildConfig),
    /// Sub-tables keyed by target name.
    Named(BTreeMap<String, BuildConfig>),
}

impl BuildSection {
    /// The target `name` designates, or the only target when `name` is absent.
    /// Errors name the available targets so the caller can print them as-is.
    pub fn select(&self, name: Option<&str>) -> Result<&BuildConfig, String> {
        let available = |targets: &BTreeMap<String, BuildConfig>| {
            targets.keys().cloned().collect::<Vec<_>>().join(", ")
        };
        match (self, name) {
            (BuildSection::Single(build), None) => Ok(build),
            (BuildSection::Single(_), Some(name)) => Err(format!(
                "[build] defines a single unnamed target; there is no target named {name}"
            )),
            (BuildSection::Named(targets), None) => match targets.len() {
                0 => Err("[build] defines no targets".to_string()),
                1 => Ok(targets.values().next().unwrap()),
                _ => Err(format!(
                    "[build] defines several targets; pick one of: {}",
                    available(targets)
                )),
            },
            (BuildSection::Named(targets), Some(name)) => {
                targets.get(name).ok_or_else(|| {
                    format!(
                        "no build target named {name} (available: {})",
                        available(targets)
                    )
                })
            }
        }
    }
}

// `deny_unknown_fields` keeps the untagged forms unambiguous: without it,
// `[build] command = ...` next to `[build.pdf]` would match Single and
// silently drop the named target.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BuildConfig {
    /// Shell command run inside the assembled tree, `YEOKJA_ROOT` set.
    pub command: String,
    /// Tree-relative paths copied (dereferencing links) into `dist` after a
    /// successful build.
    #[serde(default)]
    pub outputs: Vec<String>,
    #[serde(default = "default_dist")]
    pub dist: String,
}

fn default_dist() -> String {
    "dist".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectSettings {
    pub source_lang: String,
    pub target_lang: String,
    #[serde(default = "default_glossary_path")]
    pub glossary: String,
    /// Directory collecting `.yeokja.json` state files, mirroring each source's
    /// project-relative path. Absent keeps the sidecar-next-to-source layout.
    /// Lets the source tree (e.g. a git submodule) stay pristine.
    #[serde(default)]
    pub state_dir: Option<String>,
}

fn default_glossary_path() -> String {
    "glossary.toml".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceConfig {
    pub path: String,
    pub pattern: String,
    pub parser: String,
    /// Parser-owned generated syntax data. Required by parsers such as Verso
    /// whose authoritative grammar runs in an external toolchain.
    #[serde(default)]
    pub parser_manifest: Option<String>,
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
    /// Run the optional LLM-as-judge style evaluator in addition to the
    /// always-local glossary, link, format, and sentence-ending checks.
    #[serde(default = "default_style_evaluate")]
    pub style_evaluate: bool,
}

fn default_max_retries() -> u32 { 3 }
fn default_auto_evaluate() -> bool { true }
fn default_style_evaluate() -> bool { true }

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
    pub fn state_dir(&self) -> Option<&Path> {
        self.project.state_dir.as_deref().map(Path::new)
    }

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
parser_manifest = "syntax.json"
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
style_evaluate = false

[translation]
concurrency = 12
"#;
        let config = ProjectConfig::from_toml(toml).unwrap();
        assert_eq!(config.translation.as_ref().unwrap().concurrency, 12);
        assert_eq!(config.project.source_lang, "en");
        assert_eq!(config.project.target_lang, "ko");
        assert_eq!(config.sources.len(), 1);
        assert_eq!(config.sources[0].parser, "markdown");
        assert_eq!(
            config.sources[0].parser_manifest.as_deref(),
            Some("syntax.json")
        );
        assert_eq!(config.provider.provider_type, "anthropic");
        assert_eq!(config.server.unwrap().port, 8080);
        let evaluation = config.evaluation.unwrap();
        assert_eq!(evaluation.max_retries, 5);
        assert!(!evaluation.auto_evaluate);
        assert!(!evaluation.style_evaluate);
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
        assert!(config.state_dir().is_none());
    }

    #[test]
    fn parse_derive_and_build() {
        let toml = r#"
[project]
source_lang = "en"
target_lang = "ko"

[provider]
type = "claude_code"
model = "claude-sonnet-5"

[derive]
base = "upstream"

[[derive.overlay]]
path = "ko"
require_base = true

[[derive.overlay]]
path = "assets"

[[derive.step]]
kind = "patch"
file = "patches/fix.patch"

[[derive.step]]
kind = "generate"
command = "echo hi > marker.txt"

[build]
command = "make html"
outputs = ["site"]
"#;
        let config = ProjectConfig::from_toml(toml).unwrap();
        let derive = config.derive.unwrap();
        assert_eq!(derive.base, "upstream");
        assert_eq!(derive.target, "build/tree");
        assert_eq!(derive.overlay.len(), 2);
        assert!(derive.overlay[0].require_base);
        assert!(!derive.overlay[1].require_base);
        assert!(matches!(&derive.step[0], DeriveStep::Patch { file } if file == "patches/fix.patch"));
        assert!(matches!(&derive.step[1], DeriveStep::Generate { .. }));
        let build = config.build.unwrap();
        let target = build.select(None).unwrap();
        assert_eq!(target.outputs, vec!["site"]);
        assert_eq!(target.dist, "dist");
        assert!(build.select(Some("pdf")).is_err());
    }

    #[test]
    fn parse_named_build_targets() {
        let toml = r#"
[project]
source_lang = "en"
target_lang = "ko"

[provider]
type = "claude_code"
model = "claude-sonnet-5"

[build.html]
command = "make html"
outputs = ["site"]

[build.pdf]
command = "make pdf"
outputs = ["book.pdf"]
"#;
        let config = ProjectConfig::from_toml(toml).unwrap();
        let build = config.build.unwrap();
        assert_eq!(build.select(Some("html")).unwrap().outputs, vec!["site"]);
        assert_eq!(build.select(Some("pdf")).unwrap().outputs, vec!["book.pdf"]);
        // Several targets: the caller must name one, and a wrong name lists them.
        assert!(build.select(None).unwrap_err().contains("html, pdf"));
        assert!(build.select(Some("epub")).unwrap_err().contains("html, pdf"));
    }

    #[test]
    fn lone_named_build_target_needs_no_name() {
        let toml = r#"
[project]
source_lang = "en"
target_lang = "ko"

[provider]
type = "anthropic"
model = "claude-sonnet-5"

[build.html]
command = "make html"
"#;
        let config = ProjectConfig::from_toml(toml).unwrap();
        let build = config.build.unwrap();
        assert_eq!(build.select(None).unwrap().command, "make html");
    }

    #[test]
    fn mixed_build_forms_do_not_parse() {
        // An unnamed command next to a named target must be rejected, not
        // silently collapsed into the unnamed form.
        let toml = r#"
[project]
source_lang = "en"
target_lang = "ko"

[provider]
type = "anthropic"
model = "claude-sonnet-5"

[build]
command = "make html"

[build.pdf]
command = "make pdf"
"#;
        assert!(ProjectConfig::from_toml(toml).is_err());
    }

    #[test]
    fn parse_state_dir() {
        let toml = r#"
[project]
source_lang = "en"
target_lang = "ko"
state_dir = "state"

[provider]
type = "anthropic"
model = "claude-sonnet-5"
"#;
        let config = ProjectConfig::from_toml(toml).unwrap();
        assert_eq!(config.state_dir(), Some(Path::new("state")));
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
