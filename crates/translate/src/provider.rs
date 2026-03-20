use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct TranslateRequest {
    /// Segments to translate, keyed by index (1-based, matching prompt format).
    pub segments: Vec<(usize, String)>,
    /// Full text of the containing block for context.
    pub block_context: String,
    /// Glossary terms relevant to these segments.
    pub glossary: HashMap<String, String>,
    pub source_lang: String,
    pub target_lang: String,
    /// Optional feedback from previous evaluation failures (for retry loop).
    pub feedback: Option<String>,
}

#[derive(Debug, Clone)]
pub struct TranslateResponse {
    /// Segment index → translated text.
    pub translations: HashMap<usize, String>,
    pub usage: Option<TokenUsage>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
}

#[derive(Debug, thiserror::Error)]
pub enum TranslateError {
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("API error: {status} {message}")]
    Api { status: u16, message: String },
    #[error("Rate limited")]
    RateLimited { retry_after: Option<u64> },
    #[error("Parse error: {0}")]
    Parse(String),
}

#[async_trait]
pub trait TranslationProvider: Send + Sync {
    async fn translate(&self, request: TranslateRequest) -> Result<TranslateResponse, TranslateError>;
}
