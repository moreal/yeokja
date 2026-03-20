use crate::prompt::{build_prompt, parse_response};
use crate::provider::{TokenUsage, TranslateError, TranslateRequest, TranslateResponse, TranslationProvider};
use crate::rate_limit::RateLimiter;
use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};

pub struct OpenAICompatibleProvider {
    client: Client,
    base_url: String,
    api_key: String,
    model: String,
    rate_limiter: RateLimiter,
}

#[derive(Serialize)]
struct ChatRequest {
    model: String,
    messages: Vec<ChatMessage>,
}

#[derive(Serialize, Deserialize)]
struct ChatMessage {
    role: String,
    content: String,
}

#[derive(Deserialize)]
struct ChatResponse {
    choices: Vec<ChatChoice>,
    usage: Option<ChatUsage>,
}

#[derive(Deserialize)]
struct ChatChoice {
    message: ChatMessage,
}

#[derive(Deserialize)]
struct ChatUsage {
    prompt_tokens: u64,
    completion_tokens: u64,
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
impl TranslationProvider for OpenAICompatibleProvider {
    async fn translate(&self, request: TranslateRequest) -> Result<TranslateResponse, TranslateError> {
        self.rate_limiter.acquire().await;

        let prompt = build_prompt(&request);
        let chat_request = ChatRequest {
            model: self.model.clone(),
            messages: vec![ChatMessage {
                role: "user".to_string(),
                content: prompt,
            }],
        };

        let url = format!("{}/chat/completions", self.base_url.trim_end_matches('/'));
        let response = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .json(&chat_request)
            .send()
            .await?;

        // Check for rate limit headers
        if let Some(remaining) = response
            .headers()
            .get("x-ratelimit-remaining")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse::<u64>().ok())
        {
            self.rate_limiter.update_from_remaining(remaining).await;
        }

        let status = response.status().as_u16();
        if status == 429 {
            let retry_after = response
                .headers()
                .get("retry-after")
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.parse::<u64>().ok());
            self.rate_limiter.report_rate_limited(retry_after).await;
            return Err(TranslateError::RateLimited { retry_after });
        }

        if !response.status().is_success() {
            let message = response.text().await.unwrap_or_default();
            return Err(TranslateError::Api { status, message });
        }

        self.rate_limiter.report_success().await;

        let chat_response: ChatResponse = response.json().await?;
        let reply = chat_response
            .choices
            .first()
            .map(|c| c.message.content.as_str())
            .unwrap_or("");

        let translations = parse_response(reply)
            .map_err(TranslateError::Parse)?;

        Ok(TranslateResponse {
            translations,
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
