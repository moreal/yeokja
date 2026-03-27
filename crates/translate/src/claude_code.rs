use crate::provider::{CompletionRequest, CompletionResponse, LlmProvider, TokenUsage, TranslateError};
use async_trait::async_trait;
use serde::Deserialize;
use std::process::Stdio;
use tokio::process::Command;

/// Provider that uses the `claude` CLI (`claude -p`) as a translation backend.
pub struct ClaudeCodeProvider {
    model: Option<String>,
    system_prompt: Option<String>,
}

#[derive(Deserialize)]
struct ClaudeCodeResponseFull {
    result: Option<String>,
    is_error: bool,
    #[serde(default, rename = "modelUsage")]
    model_usage: Option<serde_json::Value>,
}

impl ClaudeCodeProvider {
    pub fn new(model: Option<String>, system_prompt: Option<String>) -> Self {
        Self { model, system_prompt }
    }
}

#[async_trait]
impl LlmProvider for ClaudeCodeProvider {
    async fn complete(&self, request: CompletionRequest) -> Result<CompletionResponse, TranslateError> {
        tracing::debug!(model = ?self.model, prompt_len = request.prompt.len(), "Calling claude CLI");

        let mut cmd = Command::new("claude");
        cmd.arg("-p")
            .arg("--output-format")
            .arg("json")
            .arg("--no-session-persistence")
            .arg("--disable-slash-commands")
            .arg("--tools")
            .arg("")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        if let Some(model) = &self.model {
            cmd.arg("--model").arg(model);
        }

        if let Some(system_prompt) = &self.system_prompt {
            cmd.arg("--system-prompt").arg(system_prompt);
        }

        // Pass prompt via stdin to avoid shell argument size limits
        cmd.arg("-");

        let mut child = cmd.spawn().map_err(|e| {
            TranslateError::Api {
                status: 0,
                message: format!("Failed to execute claude CLI: {e}"),
            }
        })?;

        if let Some(mut stdin) = child.stdin.take() {
            use tokio::io::AsyncWriteExt;
            stdin.write_all(request.prompt.as_bytes()).await.map_err(|e| {
                TranslateError::Api {
                    status: 0,
                    message: format!("Failed to write to claude stdin: {e}"),
                }
            })?;
            drop(stdin);
        }

        let output = child.wait_with_output().await.map_err(|e| {
            TranslateError::Api {
                status: 0,
                message: format!("Failed to wait for claude CLI: {e}"),
            }
        })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let stdout = String::from_utf8_lossy(&output.stdout);
            let detail = if stderr.trim().is_empty() { &stdout } else { &stderr };
            tracing::warn!(exit_code = output.status.code(), "claude CLI exited with error");
            return Err(TranslateError::Api {
                status: output.status.code().unwrap_or(1) as u16,
                message: format!("claude CLI failed: {detail}"),
            });
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let resp: ClaudeCodeResponseFull = serde_json::from_str(&stdout).map_err(|e| {
            TranslateError::Parse(format!("Failed to parse claude CLI response: {e}"))
        })?;

        if resp.is_error {
            return Err(TranslateError::Api {
                status: 0,
                message: resp.result.unwrap_or_else(|| "Unknown error".to_string()),
            });
        }

        let text = resp.result.unwrap_or_default();
        tracing::debug!(text_len = text.len(), "Claude CLI response received");

        // Extract usage from modelUsage if available
        let usage = resp.model_usage.and_then(|mu| {
            // modelUsage is { "model-name": { "inputTokens": N, "outputTokens": N, ... } }
            mu.as_object().and_then(|obj| {
                obj.values().next().map(|v| {
                    let input = v.get("inputTokens").and_then(|v| v.as_u64()).unwrap_or(0);
                    let output = v.get("outputTokens").and_then(|v| v.as_u64()).unwrap_or(0);
                    TokenUsage {
                        input_tokens: input,
                        output_tokens: output,
                    }
                })
            })
        });

        Ok(CompletionResponse { text, usage })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::prompt::parse_response;

    #[test]
    fn provider_construction() {
        let provider = ClaudeCodeProvider::new(Some("sonnet".to_string()), Some("You are a translator.".to_string()));
        assert_eq!(provider.model.as_deref(), Some("sonnet"));
        assert_eq!(provider.system_prompt.as_deref(), Some("You are a translator."));
    }

    #[test]
    fn provider_construction_no_model() {
        let provider = ClaudeCodeProvider::new(None, None);
        assert!(provider.model.is_none());
        assert!(provider.system_prompt.is_none());
    }

    #[test]
    fn parse_claude_code_response() {
        let json = r#"{
            "type": "result",
            "subtype": "success",
            "is_error": false,
            "result": "[1] 안녕하세요.\n[2] 세계.",
            "modelUsage": {
                "claude-sonnet-4-6": {
                    "inputTokens": 100,
                    "outputTokens": 50
                }
            }
        }"#;
        let resp: ClaudeCodeResponseFull = serde_json::from_str(json).unwrap();
        assert!(!resp.is_error);
        assert_eq!(resp.result.as_deref(), Some("[1] 안녕하세요.\n[2] 세계."));

        let translations = parse_response(resp.result.as_deref().unwrap()).unwrap();
        assert_eq!(translations[&1], "안녕하세요.");
        assert_eq!(translations[&2], "세계.");

        let usage = resp.model_usage.and_then(|mu| {
            mu.as_object().and_then(|obj| {
                obj.values().next().and_then(|v| {
                    Some(TokenUsage {
                        input_tokens: v.get("inputTokens")?.as_u64()?,
                        output_tokens: v.get("outputTokens")?.as_u64()?,
                    })
                })
            })
        }).unwrap();
        assert_eq!(usage.input_tokens, 100);
        assert_eq!(usage.output_tokens, 50);
    }

    #[test]
    fn parse_error_response() {
        let json = r#"{
            "type": "result",
            "subtype": "error",
            "is_error": true,
            "result": "Something went wrong"
        }"#;
        let resp: ClaudeCodeResponseFull = serde_json::from_str(json).unwrap();
        assert!(resp.is_error);
    }
}
