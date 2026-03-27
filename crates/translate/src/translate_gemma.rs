use crate::openai_types::{ChatRequest, ChatMessage, ChatResponse};
use crate::provider::{
    TokenUsage, TranslateError, TranslateRequest, TranslateResponse, TranslationProvider,
};
use crate::rate_limit::RateLimiter;
use async_trait::async_trait;
use reqwest::Client;
use std::collections::HashMap;

pub struct TranslateGemmaProvider {
    client: Client,
    base_url: String,
    api_key: String,
    model: String,
    rate_limiter: RateLimiter,
}

impl TranslateGemmaProvider {
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

/// Build the TranslateGemma-specific prompt format.
/// TranslateGemma uses: <<<source>>>LANG<<<target>>>LANG<<<text>>>TEXT
fn build_translate_gemma_prompt(source_lang: &str, target_lang: &str, text: &str) -> String {
    format!("<<<source>>>{source_lang}<<<target>>>{target_lang}<<<text>>>{text}")
}

#[async_trait]
impl TranslationProvider for TranslateGemmaProvider {
    async fn translate(
        &self,
        request: TranslateRequest,
    ) -> Result<TranslateResponse, TranslateError> {
        // TranslateGemma translates one segment at a time (no batching support)
        // We send each segment as a separate request
        let mut translations = HashMap::new();
        let mut total_input = 0u64;
        let mut total_output = 0u64;

        for (idx, text) in &request.segments {
            self.rate_limiter.acquire().await;

            let prompt =
                build_translate_gemma_prompt(&request.source_lang, &request.target_lang, text);

            let chat_request = ChatRequest {
                model: self.model.clone(),
                messages: vec![ChatMessage {
                    role: "user".to_string(),
                    content: prompt,
                }],
            };

            let url = format!(
                "{}/chat/completions",
                self.base_url.trim_end_matches('/')
            );
            tracing::debug!(model = %self.model, segment_idx = idx, "Sending TranslateGemma HTTP request");
            let response = self.rate_limiter.process_response(
                self.client
                    .post(&url)
                    .header("Authorization", format!("Bearer {}", self.api_key))
                    .json(&chat_request)
                    .send()
                    .await?,
            ).await?;

            let chat_response: ChatResponse = response.json().await?;
            if let Some(choice) = chat_response.choices.first() {
                let translation = choice.message.content.trim().to_string();
                tracing::debug!(segment_idx = idx, text_len = translation.len(), "TranslateGemma response received");
                if !translation.is_empty() {
                    translations.insert(*idx, translation);
                }
            }

            if let Some(usage) = chat_response.usage {
                total_input += usage.prompt_tokens;
                total_output += usage.completion_tokens;
            }
        }

        if translations.is_empty() {
            return Err(TranslateError::Parse(
                "No translations received".to_string(),
            ));
        }

        Ok(TranslateResponse {
            translations,
            usage: Some(TokenUsage {
                input_tokens: total_input,
                output_tokens: total_output,
            }),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_construction() {
        let provider = TranslateGemmaProvider::new(
            "http://localhost:8000/v1".to_string(),
            "".to_string(),
            "translategemma-12b-it".to_string(),
        );
        assert_eq!(provider.model, "translategemma-12b-it");
    }

    #[test]
    fn translate_gemma_prompt_format() {
        let prompt = build_translate_gemma_prompt("en", "ko", "Hello world.");
        assert_eq!(
            prompt,
            "<<<source>>>en<<<target>>>ko<<<text>>>Hello world."
        );
    }

    #[test]
    fn translate_gemma_prompt_preserves_text() {
        let prompt =
            build_translate_gemma_prompt("en", "de", "The repository stores all history.");
        assert!(prompt.starts_with("<<<source>>>en<<<target>>>de<<<text>>>"));
        assert!(prompt.ends_with("The repository stores all history."));
    }

    #[test]
    fn chat_response_deserialization() {
        let json = r#"{
            "choices": [{
                "message": {"role": "assistant", "content": "안녕하세요."},
                "finish_reason": "stop"
            }],
            "usage": {"prompt_tokens": 50, "completion_tokens": 10}
        }"#;
        let resp: ChatResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.choices[0].message.content, "안녕하세요.");
        assert_eq!(resp.usage.as_ref().unwrap().prompt_tokens, 50);
    }
}
