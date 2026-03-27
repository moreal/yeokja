use std::sync::Arc;
use yeokja_core::config::ProviderConfig;
use yeokja_translate::provider::{LlmProvider, TranslationProvider};
use yeokja_translate::openai_compatible::OpenAICompatibleProvider;
use yeokja_translate::anthropic::AnthropicProvider;
use yeokja_translate::gemini::GeminiProvider;
use yeokja_translate::translate_gemma::TranslateGemmaProvider;
use yeokja_translate::claude_code::ClaudeCodeProvider;
use yeokja_translate::pi::PiProvider;

pub fn create_provider(config: &ProviderConfig) -> anyhow::Result<Arc<dyn TranslationProvider>> {
    let api_key = if let Some(env_var) = &config.api_key_env {
        std::env::var(env_var)
            .map_err(|_| anyhow::anyhow!("Environment variable {} not set", env_var))?
    } else {
        String::new()
    };

    let provider: Arc<dyn TranslationProvider> = match config.provider_type.as_str() {
        "openai_compatible" | "openai" => {
            let base_url = config.base_url.clone()
                .unwrap_or_else(|| "https://api.openai.com/v1".to_string());
            Arc::new(OpenAICompatibleProvider::new(base_url, api_key, config.model.clone()))
        }
        "anthropic" => {
            Arc::new(AnthropicProvider::new(api_key, config.model.clone()))
        }
        "gemini" => {
            Arc::new(GeminiProvider::new(api_key, config.model.clone()))
        }
        "translate_gemma" | "translategemma" => {
            let base_url = config.base_url.clone()
                .unwrap_or_else(|| "http://localhost:8000/v1".to_string());
            Arc::new(TranslateGemmaProvider::new(base_url, api_key, config.model.clone()))
        }
        "claude_code" | "claude-code" => {
            let model = if config.model.is_empty() {
                None
            } else {
                Some(config.model.clone())
            };
            let system_prompt = Some("You are a professional translator. Translate accurately while preserving the original formatting, technical terms, and structure. Do not add explanations or commentary — output only the translation.".to_string());
            Arc::new(ClaudeCodeProvider::new(model, system_prompt))
        }
        "pi" => {
            let model = if config.model.is_empty() {
                None
            } else {
                Some(config.model.clone())
            };
            let pi_provider = config.base_url.clone(); // reuse base_url field for pi's --provider
            let system_prompt = Some("You are a professional translator. Translate accurately while preserving the original formatting, technical terms, and structure. Do not add explanations or commentary — output only the translation.".to_string());
            Arc::new(PiProvider::new(model, pi_provider, system_prompt))
        }
        other => {
            anyhow::bail!("Unknown provider type: {}", other);
        }
    };

    Ok(provider)
}

/// Create an LlmProvider for evaluation (e.g., StyleEvaluator).
pub fn create_evaluator_provider(config: &ProviderConfig) -> anyhow::Result<Arc<dyn LlmProvider>> {
    let api_key = if let Some(env_var) = &config.api_key_env {
        std::env::var(env_var)
            .map_err(|_| anyhow::anyhow!("Environment variable {} not set", env_var))?
    } else {
        String::new()
    };

    let system_prompt = "You are a translation quality evaluator. Assess translations for naturalness, accuracy, and style consistency. Respond concisely with a rating and brief explanation.".to_string();

    let provider: Arc<dyn LlmProvider> = match config.provider_type.as_str() {
        "openai_compatible" | "openai" => {
            let base_url = config.base_url.clone()
                .unwrap_or_else(|| "https://api.openai.com/v1".to_string());
            // OpenAI-compatible providers don't have system_prompt in struct,
            // StyleEvaluator handles the prompt itself
            let _ = system_prompt;
            Arc::new(OpenAICompatibleProvider::new(base_url, api_key, config.model.clone()))
        }
        "anthropic" => {
            let _ = system_prompt;
            Arc::new(AnthropicProvider::new(api_key, config.model.clone()))
        }
        "gemini" => {
            let _ = system_prompt;
            Arc::new(GeminiProvider::new(api_key, config.model.clone()))
        }
        "claude_code" | "claude-code" => {
            let model = if config.model.is_empty() {
                None
            } else {
                Some(config.model.clone())
            };
            Arc::new(ClaudeCodeProvider::new(model, Some(system_prompt)))
        }
        "pi" => {
            let model = if config.model.is_empty() {
                None
            } else {
                Some(config.model.clone())
            };
            let pi_provider = config.base_url.clone();
            Arc::new(PiProvider::new(model, pi_provider, Some(system_prompt)))
        }
        other => {
            anyhow::bail!("Unknown provider type for evaluator: {}", other);
        }
    };

    Ok(provider)
}
