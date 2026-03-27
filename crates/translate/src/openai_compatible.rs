use crate::openai_types::{ChatRequest, ChatMessage, ChatResponse};
use crate::provider::{CompletionRequest, CompletionResponse, LlmProvider, TokenUsage, TranslateError};
use crate::rate_limit::RateLimiter;
use async_trait::async_trait;
use reqwest::Client;

pub struct OpenAICompatibleProvider {
    client: Client,
    base_url: String,
    api_key: String,
    model: String,
    rate_limiter: RateLimiter,
}

impl OpenAICompatibleProvider {
    pub fn new(base_url: String, api_key: String, model: String) -> Self {
        Self {
            client: Client::new(),
            base_url,
            api_key,
            model,
            rate_limiter: RateLimiter::new(),
        }
    }
}

#[async_trait]
impl LlmProvider for OpenAICompatibleProvider {
    async fn complete(&self, request: CompletionRequest) -> Result<CompletionResponse, TranslateError> {
        self.rate_limiter.acquire().await;

        let chat_request = ChatRequest {
            model: self.model.clone(),
            messages: vec![ChatMessage {
                role: "user".to_string(),
                content: request.prompt,
            }],
        };

        let url = format!("{}/chat/completions", self.base_url.trim_end_matches('/'));
        tracing::debug!(model = %self.model, url = %url, "Sending OpenAI-compatible HTTP request");
        let response = self.rate_limiter.process_response(
            self.client
                .post(&url)
                .header("Authorization", format!("Bearer {}", self.api_key))
                .json(&chat_request)
                .send()
                .await?,
        ).await?;

        let chat_response: ChatResponse = response.json().await?;
        let text = chat_response
            .choices
            .first()
            .map(|c| c.message.content.clone())
            .unwrap_or_default();
        tracing::debug!(text_len = text.len(), "OpenAI-compatible response received");

        Ok(CompletionResponse {
            text,
            usage: chat_response.usage.map(|u| TokenUsage {
                input_tokens: u.prompt_tokens,
                output_tokens: u.completion_tokens,
            }),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_construction() {
        let provider = OpenAICompatibleProvider::new(
            "https://api.openai.com/v1".to_string(),
            "test-key".to_string(),
            "gpt-4o".to_string(),
        );
        assert_eq!(provider.model, "gpt-4o");
    }
}
