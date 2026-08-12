//! Provider construction from project configuration.
//! Shared by the CLI and the server.

use crate::anthropic::AnthropicProvider;
use crate::claude_code::ClaudeCodeProvider;
use crate::gemini::GeminiProvider;
use crate::openai_compatible::OpenAICompatibleProvider;
use crate::pi::PiProvider;
use crate::provider::{LlmProvider, TranslationProvider};
use crate::translate_gemma::TranslateGemmaProvider;
use std::sync::Arc;
use yeokja_core::config::ProviderConfig;

#[derive(Debug, thiserror::Error)]
pub enum FactoryError {
    #[error("Environment variable {0} not set")]
    MissingApiKey(String),
    #[error("Unknown provider type: {0}")]
    UnknownProvider(String),
}

const TRANSLATOR_SYSTEM_PROMPT: &str = "You are a professional translator. Translate accurately while preserving the original formatting, technical terms, and structure. Do not add explanations or commentary — output only the translation.";
const EVALUATOR_SYSTEM_PROMPT: &str = "You are a translation quality evaluator. Assess translations for naturalness, accuracy, and style consistency. Respond concisely with a rating and brief explanation.";

fn api_key(config: &ProviderConfig) -> Result<String, FactoryError> {
    match &config.api_key_env {
        Some(env_var) => {
            std::env::var(env_var).map_err(|_| FactoryError::MissingApiKey(env_var.clone()))
        }
        None => Ok(String::new()),
    }
}

fn optional_model(config: &ProviderConfig) -> Option<String> {
    if config.model.is_empty() {
        None
    } else {
        Some(config.model.clone())
    }
}

/// Build the shared low-level LLM provider described by the config.
/// `system_prompt` is only used by CLI-backed providers (claude_code, pi);
/// HTTP providers receive their instructions inside each prompt.
fn create_llm_provider(
    config: &ProviderConfig,
    system_prompt: &str,
) -> Result<Arc<dyn LlmProvider>, FactoryError> {
    let provider: Arc<dyn LlmProvider> = match config.provider_type.as_str() {
        "openai_compatible" | "openai" => {
            let base_url = config
                .base_url
                .clone()
                .unwrap_or_else(|| "https://api.openai.com/v1".to_string());
            Arc::new(OpenAICompatibleProvider::new(
                base_url,
                api_key(config)?,
                config.model.clone(),
            ))
        }
        "anthropic" => Arc::new(AnthropicProvider::new(api_key(config)?, config.model.clone())),
        "gemini" => Arc::new(GeminiProvider::new(api_key(config)?, config.model.clone())),
        "claude_code" | "claude-code" => Arc::new(ClaudeCodeProvider::new(
            optional_model(config),
            Some(system_prompt.to_string()),
        )),
        "pi" => Arc::new(PiProvider::new(
            optional_model(config),
            config.base_url.clone(), // pi reuses base_url as its --provider flag
            Some(system_prompt.to_string()),
        )),
        other => return Err(FactoryError::UnknownProvider(other.to_string())),
    };
    Ok(provider)
}

/// Create the translation provider described by the config.
pub fn create_provider(
    config: &ProviderConfig,
) -> Result<Arc<dyn TranslationProvider>, FactoryError> {
    // TranslateGemma has its own request format and only implements
    // TranslationProvider, so it is handled outside create_llm_provider.
    if matches!(
        config.provider_type.as_str(),
        "translate_gemma" | "translategemma"
    ) {
        let base_url = config
            .base_url
            .clone()
            .unwrap_or_else(|| "http://localhost:8000/v1".to_string());
        return Ok(Arc::new(TranslateGemmaProvider::new(
            base_url,
            api_key(config)?,
            config.model.clone(),
        )));
    }

    struct AsTranslation(Arc<dyn LlmProvider>);

    #[async_trait::async_trait]
    impl TranslationProvider for AsTranslation {
        async fn translate(
            &self,
            request: crate::provider::TranslateRequest,
        ) -> Result<crate::provider::TranslateResponse, crate::provider::TranslateError> {
            crate::provider::translate_via_prompt(self.0.as_ref(), request).await
        }
    }

    let llm = create_llm_provider(config, TRANSLATOR_SYSTEM_PROMPT)?;
    Ok(Arc::new(AsTranslation(llm)))
}

/// Create the raw LLM provider used for LLM-based evaluation (StyleEvaluator).
/// Returns `None` when the configured provider cannot act as a generic LLM
/// judge (TranslateGemma); mechanical checks still run in that case.
pub fn create_evaluator_provider(
    config: &ProviderConfig,
) -> Result<Option<Arc<dyn LlmProvider>>, FactoryError> {
    if matches!(
        config.provider_type.as_str(),
        "translate_gemma" | "translategemma"
    ) {
        tracing::warn!("translate_gemma cannot act as a style evaluator; running mechanical checks only");
        return Ok(None);
    }
    create_llm_provider(config, EVALUATOR_SYSTEM_PROMPT).map(Some)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn provider_config(provider_type: &str) -> ProviderConfig {
        ProviderConfig {
            provider_type: provider_type.to_string(),
            model: "test-model".to_string(),
            api_key_env: None,
            base_url: None,
            prompt_template: None,
        }
    }

    #[test]
    fn creates_known_providers() {
        for ty in ["openai", "openai_compatible", "anthropic", "gemini", "translate_gemma"] {
            assert!(create_provider(&provider_config(ty)).is_ok(), "provider {ty}");
        }
    }

    #[test]
    fn unknown_provider_rejected() {
        assert!(matches!(
            create_provider(&provider_config("nope")),
            Err(FactoryError::UnknownProvider(_))
        ));
    }

    #[test]
    fn missing_api_key_env_rejected() {
        let mut config = provider_config("anthropic");
        config.api_key_env = Some("YEOKJA_TEST_SURELY_UNSET_ENV".to_string());
        assert!(matches!(
            create_provider(&config),
            Err(FactoryError::MissingApiKey(_))
        ));
    }

    #[test]
    fn translate_gemma_has_no_evaluator() {
        assert!(create_evaluator_provider(&provider_config("translate_gemma"))
            .unwrap()
            .is_none());
    }
}
