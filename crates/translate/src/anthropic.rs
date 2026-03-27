use crate::provider::{
    CompletionRequest, CompletionResponse, LlmProvider, TokenUsage, TranslateError,
};
use crate::rate_limit::RateLimiter;
use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};

pub struct AnthropicProvider {
    client: Client,
    api_key: String,
    model: String,
    rate_limiter: RateLimiter,
}

#[derive(Serialize)]
struct AnthropicRequest {
    model: String,
    max_tokens: u32,
    messages: Vec<AnthropicMessage>,
}

#[derive(Serialize, Deserialize)]
struct AnthropicMessage {
    role: String,
    content: String,
}

#[derive(Deserialize)]
struct AnthropicResponse {
    content: Vec<AnthropicContent>,
    usage: Option<AnthropicUsage>,
}

#[derive(Deserialize)]
struct AnthropicContent {
    #[serde(rename = "type")]
    content_type: String,
    text: Option<String>,
}

#[derive(Deserialize)]
struct AnthropicUsage {
    input_tokens: u64,
    output_tokens: u64,
}

impl AnthropicProvider {
    pub fn new(api_key: String, model: String) -> Self {
        Self {
            client: Client::new(),
            api_key,
            model,
            rate_limiter: RateLimiter::new(),
        }
    }
}

#[async_trait]
impl LlmProvider for AnthropicProvider {
    async fn complete(
        &self,
        request: CompletionRequest,
    ) -> Result<CompletionResponse, TranslateError> {
        self.rate_limiter.acquire().await;

        let api_request = AnthropicRequest {
            model: self.model.clone(),
            max_tokens: 4096,
            messages: vec![AnthropicMessage {
                role: "user".to_string(),
                content: request.prompt,
            }],
        };

        tracing::debug!(model = %self.model, "Sending Anthropic HTTP request");
        let response = self.rate_limiter.process_response(
            self.client
                .post("https://api.anthropic.com/v1/messages")
                .header("x-api-key", &self.api_key)
                .header("anthropic-version", "2023-06-01")
                .header("content-type", "application/json")
                .json(&api_request)
                .send()
                .await?,
        ).await?;

        let api_response: AnthropicResponse = response.json().await?;
        let text = api_response
            .content
            .iter()
            .filter(|c| c.content_type == "text")
            .filter_map(|c| c.text.as_deref())
            .collect::<Vec<_>>()
            .join("");
        tracing::debug!(text_len = text.len(), "Anthropic response received");

        Ok(CompletionResponse {
            text,
            usage: api_response.usage.map(|u| TokenUsage {
                input_tokens: u.input_tokens,
                output_tokens: u.output_tokens,
            }),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::prompt::parse_response;

    #[test]
    fn provider_construction() {
        let provider = AnthropicProvider::new(
            "test-key".to_string(),
            "claude-sonnet-4-20250514".to_string(),
        );
        assert_eq!(provider.model, "claude-sonnet-4-20250514");
    }

    #[test]
    fn anthropic_request_serialization() {
        let req = AnthropicRequest {
            model: "claude-sonnet-4-20250514".to_string(),
            max_tokens: 4096,
            messages: vec![AnthropicMessage {
                role: "user".to_string(),
                content: "Hello".to_string(),
            }],
        };
        let json = serde_json::to_value(&req).unwrap();
        assert_eq!(json["model"], "claude-sonnet-4-20250514");
        assert_eq!(json["max_tokens"], 4096);
        assert_eq!(json["messages"][0]["role"], "user");
    }

    #[test]
    fn anthropic_response_deserialization() {
        let json = r#"{
            "id": "msg_123",
            "type": "message",
            "role": "assistant",
            "content": [{"type": "text", "text": "[1] 안녕하세요."}],
            "model": "claude-sonnet-4-20250514",
            "stop_reason": "end_turn",
            "usage": {"input_tokens": 100, "output_tokens": 50}
        }"#;
        let resp: AnthropicResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.content.len(), 1);
        assert_eq!(resp.content[0].text.as_deref(), Some("[1] 안녕하세요."));
        assert_eq!(resp.usage.as_ref().unwrap().input_tokens, 100);
        assert_eq!(resp.usage.as_ref().unwrap().output_tokens, 50);
    }

    #[test]
    fn anthropic_response_text_extraction() {
        let json = r#"{
            "content": [
                {"type": "text", "text": "[1] First.\n"},
                {"type": "text", "text": "[2] Second."}
            ],
            "usage": {"input_tokens": 10, "output_tokens": 20}
        }"#;
        let resp: AnthropicResponse = serde_json::from_str(json).unwrap();
        let reply: String = resp
            .content
            .iter()
            .filter(|c| c.content_type == "text")
            .filter_map(|c| c.text.as_deref())
            .collect::<Vec<_>>()
            .join("");
        let translations = parse_response(&reply).unwrap();
        assert_eq!(translations[&1], "First.");
        assert_eq!(translations[&2], "Second.");
    }
}
