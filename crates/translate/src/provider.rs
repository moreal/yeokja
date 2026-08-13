use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// --- Raw LLM completion ---

/// Raw LLM completion request.
#[derive(Debug, Clone)]
pub struct CompletionRequest {
    pub prompt: String,
}

/// Raw LLM completion response.
#[derive(Debug, Clone)]
pub struct CompletionResponse {
    pub text: String,
    pub usage: Option<TokenUsage>,
}

/// Low-level LLM provider that sends a prompt and returns raw text.
/// All providers implement this trait. Higher-level concerns (prompt formatting,
/// response parsing) are handled by `TranslationProvider`.
#[async_trait]
pub trait LlmProvider: Send + Sync {
    async fn complete(&self, request: CompletionRequest) -> Result<CompletionResponse, TranslateError>;
}

// --- Translation-specific types ---

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
    /// The markup the segments are written in, so the prompt can state the
    /// rules that markup actually has.
    pub markup: yeokja_core::parser::Markup,
    /// Optional feedback from previous evaluation failures (for retry loop).
    pub feedback: Option<String>,
    /// Optional custom prompt template (see `prompt::build_prompt`).
    pub prompt_template: Option<String>,
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

/// Translate by building a numbered prompt, sending it to a raw LLM provider,
/// and parsing the `[N]` format response.
pub async fn translate_via_prompt(
    llm: &dyn LlmProvider,
    request: TranslateRequest,
) -> Result<TranslateResponse, TranslateError> {
    let prompt = crate::prompt::build_prompt(&request);
    let response = llm.complete(CompletionRequest { prompt }).await?;
    let translations = crate::prompt::parse_response_for(&response.text, &request.segments)
        .map_err(TranslateError::Parse)?;
    Ok(TranslateResponse {
        translations,
        usage: response.usage,
    })
}

/// Blanket implementation: any `LlmProvider` is automatically a `TranslationProvider`
/// by building a translation prompt and parsing the [N] format response.
#[async_trait]
impl<T: LlmProvider> TranslationProvider for T {
    async fn translate(&self, request: TranslateRequest) -> Result<TranslateResponse, TranslateError> {
        translate_via_prompt(self, request).await
    }
}
