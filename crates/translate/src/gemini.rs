use crate::provider::{
    CompletionRequest, CompletionResponse, LlmProvider, TokenUsage, TranslateError,
};
use crate::rate_limit::RateLimiter;
use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};

pub struct GeminiProvider {
    client: Client,
    api_key: String,
    model: String,
    rate_limiter: RateLimiter,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct GeminiRequest {
    contents: Vec<GeminiContent>,
    generation_config: Option<GeminiGenerationConfig>,
}

#[derive(Serialize, Deserialize)]
struct GeminiContent {
    role: String,
    parts: Vec<GeminiPart>,
}

#[derive(Serialize, Deserialize)]
struct GeminiPart {
    text: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct GeminiGenerationConfig {
    max_output_tokens: u32,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GeminiResponse {
    candidates: Option<Vec<GeminiCandidate>>,
    usage_metadata: Option<GeminiUsageMetadata>,
}

#[derive(Deserialize)]
struct GeminiCandidate {
    content: Option<GeminiContent>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GeminiUsageMetadata {
    prompt_token_count: Option<u64>,
    candidates_token_count: Option<u64>,
}

impl GeminiProvider {
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
impl LlmProvider for GeminiProvider {
    async fn complete(
        &self,
        request: CompletionRequest,
    ) -> Result<CompletionResponse, TranslateError> {
        self.rate_limiter.acquire().await;

        let api_request = GeminiRequest {
            contents: vec![GeminiContent {
                role: "user".to_string(),
                parts: vec![GeminiPart { text: request.prompt }],
            }],
            generation_config: Some(GeminiGenerationConfig {
                max_output_tokens: 4096,
            }),
        };

        let url = format!(
            "https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent",
            self.model
        );

        tracing::debug!(model = %self.model, url = %url, "Sending Gemini HTTP request");
        let response = self.rate_limiter.process_response(
            self.client
                .post(&url)
                .header("x-goog-api-key", &self.api_key)
                .header("content-type", "application/json")
                .json(&api_request)
                .send()
                .await?,
        ).await?;

        let api_response: GeminiResponse = response.json().await?;

        let text = api_response
            .candidates
            .as_ref()
            .and_then(|c| c.first())
            .and_then(|c| c.content.as_ref())
            .map(|content| {
                content
                    .parts
                    .iter()
                    .map(|p| p.text.as_str())
                    .collect::<Vec<_>>()
                    .join("")
            })
            .unwrap_or_default();
        tracing::debug!(text_len = text.len(), "Gemini response received");

        Ok(CompletionResponse {
            text,
            usage: api_response.usage_metadata.map(|u| TokenUsage {
                input_tokens: u.prompt_token_count.unwrap_or(0),
                output_tokens: u.candidates_token_count.unwrap_or(0),
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
        let provider = GeminiProvider::new("test-key".to_string(), "gemini-2.5-flash".to_string());
        assert_eq!(provider.model, "gemini-2.5-flash");
    }

    #[test]
    fn gemini_request_serialization() {
        let req = GeminiRequest {
            contents: vec![GeminiContent {
                role: "user".to_string(),
                parts: vec![GeminiPart {
                    text: "Hello".to_string(),
                }],
            }],
            generation_config: Some(GeminiGenerationConfig {
                max_output_tokens: 4096,
            }),
        };
        let json = serde_json::to_value(&req).unwrap();
        assert_eq!(json["contents"][0]["role"], "user");
        assert_eq!(json["contents"][0]["parts"][0]["text"], "Hello");
        assert_eq!(json["generationConfig"]["maxOutputTokens"], 4096);
    }

    #[test]
    fn gemini_response_deserialization() {
        let json = r#"{
            "candidates": [{
                "content": {
                    "role": "model",
                    "parts": [{"text": "[1] 안녕하세요."}]
                },
                "finishReason": "STOP"
            }],
            "usageMetadata": {
                "promptTokenCount": 100,
                "candidatesTokenCount": 50,
                "totalTokenCount": 150
            }
        }"#;
        let resp: GeminiResponse = serde_json::from_str(json).unwrap();
        let text = resp.candidates.as_ref().unwrap()[0]
            .content
            .as_ref()
            .unwrap()
            .parts[0]
            .text
            .as_str();
        assert_eq!(text, "[1] 안녕하세요.");
        let usage = resp.usage_metadata.as_ref().unwrap();
        assert_eq!(usage.prompt_token_count, Some(100));
        assert_eq!(usage.candidates_token_count, Some(50));
    }

    #[test]
    fn gemini_response_text_extraction() {
        let json = r#"{
            "candidates": [{
                "content": {
                    "role": "model",
                    "parts": [
                        {"text": "[1] First."},
                        {"text": "\n[2] Second."}
                    ]
                }
            }],
            "usageMetadata": {"promptTokenCount": 10, "candidatesTokenCount": 20}
        }"#;
        let resp: GeminiResponse = serde_json::from_str(json).unwrap();
        let reply: String = resp.candidates.as_ref().unwrap()[0]
            .content
            .as_ref()
            .unwrap()
            .parts
            .iter()
            .map(|p| p.text.as_str())
            .collect::<Vec<_>>()
            .join("");
        let translations = parse_response(&reply).unwrap();
        assert_eq!(translations[&1], "First.");
        assert_eq!(translations[&2], "Second.");
    }

    #[test]
    fn gemini_url_construction() {
        let provider = GeminiProvider::new("key".to_string(), "gemini-2.5-flash".to_string());
        let url = format!(
            "https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent",
            provider.model
        );
        assert_eq!(
            url,
            "https://generativelanguage.googleapis.com/v1beta/models/gemini-2.5-flash:generateContent"
        );
    }
}
