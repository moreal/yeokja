use crate::provider::{CompletionRequest, CompletionResponse, LlmProvider, TokenUsage, TranslateError};
use async_trait::async_trait;
use serde::Deserialize;
use std::process::Stdio;
use tokio::process::Command;

/// Provider that uses the `pi` CLI (`pi -p`) as a translation backend.
pub struct PiProvider {
    model: Option<String>,
    provider: Option<String>,
    system_prompt: Option<String>,
}

impl PiProvider {
    pub fn new(model: Option<String>, provider: Option<String>, system_prompt: Option<String>) -> Self {
        Self { model, provider, system_prompt }
    }
}

/// Represents the `agent_end` event from pi's NDJSON output.
#[derive(Deserialize)]
struct PiEvent {
    #[serde(rename = "type")]
    event_type: String,
    messages: Option<Vec<PiMessage>>,
}

#[derive(Deserialize)]
struct PiMessage {
    role: Option<String>,
    content: Option<Vec<PiContent>>,
    usage: Option<PiUsage>,
}

#[derive(Deserialize)]
struct PiContent {
    #[serde(rename = "type")]
    content_type: String,
    text: Option<String>,
}

#[derive(Deserialize)]
struct PiUsage {
    input: Option<u64>,
    output: Option<u64>,
}

#[async_trait]
impl LlmProvider for PiProvider {
    async fn complete(&self, request: CompletionRequest) -> Result<CompletionResponse, TranslateError> {
        tracing::debug!(model = ?self.model, provider = ?self.provider, prompt_len = request.prompt.len(), "Calling pi CLI");

        let mut cmd = Command::new("pi");
        cmd.arg("-p")
            .arg("--mode")
            .arg("json")
            .arg("--no-tools")
            .arg("--no-session")
            .arg("--no-extensions")
            .arg("--no-skills")
            .arg("--no-prompt-templates")
            .arg("--thinking")
            .arg("off")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        if let Some(model) = &self.model {
            cmd.arg("--model").arg(model);
        }

        if let Some(provider) = &self.provider {
            cmd.arg("--provider").arg(provider);
        }

        if let Some(system_prompt) = &self.system_prompt {
            cmd.arg("--system-prompt").arg(system_prompt);
        }

        cmd.arg("-");

        let mut child = cmd.spawn().map_err(|e| {
            TranslateError::Api {
                status: 0,
                message: format!("Failed to execute pi CLI: {e}"),
            }
        })?;

        if let Some(mut stdin) = child.stdin.take() {
            use tokio::io::AsyncWriteExt;
            stdin.write_all(request.prompt.as_bytes()).await.map_err(|e| {
                TranslateError::Api {
                    status: 0,
                    message: format!("Failed to write to pi stdin: {e}"),
                }
            })?;
            drop(stdin);
        }

        let output = child.wait_with_output().await.map_err(|e| {
            TranslateError::Api {
                status: 0,
                message: format!("Failed to wait for pi CLI: {e}"),
            }
        })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let stdout = String::from_utf8_lossy(&output.stdout);
            let detail = if stderr.trim().is_empty() { &stdout } else { &stderr };
            tracing::warn!(exit_code = output.status.code(), "pi CLI exited with error");
            return Err(TranslateError::Api {
                status: output.status.code().unwrap_or(1) as u16,
                message: format!("pi CLI failed: {detail}"),
            });
        }

        let stdout = String::from_utf8_lossy(&output.stdout);

        // Parse NDJSON: find the last agent_end event
        let mut text = String::new();
        let mut usage = None;

        for line in stdout.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }

            let event: PiEvent = match serde_json::from_str(line) {
                Ok(e) => e,
                Err(_) => continue,
            };

            if event.event_type == "agent_end" && let Some(messages) = &event.messages {
                // Find the last assistant message
                if let Some(assistant_msg) = messages.iter().rev().find(|m| {
                    m.role.as_deref() == Some("assistant")
                }) {
                    // Extract text content
                    if let Some(content) = &assistant_msg.content {
                        text = content
                            .iter()
                            .filter(|c| c.content_type == "text")
                            .filter_map(|c| c.text.as_deref())
                            .collect::<Vec<_>>()
                            .join("");
                    }

                    // Extract usage
                    if let Some(u) = &assistant_msg.usage {
                        usage = Some(TokenUsage {
                            input_tokens: u.input.unwrap_or(0),
                            output_tokens: u.output.unwrap_or(0),
                        });
                    }
                }
            }
        }

        if text.is_empty() {
            return Err(TranslateError::Parse("No text content in pi response".to_string()));
        }

        tracing::debug!(text_len = text.len(), "pi CLI response received");

        Ok(CompletionResponse { text, usage })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::prompt::parse_response;

    #[test]
    fn provider_construction() {
        let provider = PiProvider::new(
            Some("sonnet".to_string()),
            Some("anthropic".to_string()),
            Some("You are a translator.".to_string()),
        );
        assert_eq!(provider.model.as_deref(), Some("sonnet"));
        assert_eq!(provider.provider.as_deref(), Some("anthropic"));
        assert_eq!(provider.system_prompt.as_deref(), Some("You are a translator."));
    }

    #[test]
    fn provider_construction_minimal() {
        let provider = PiProvider::new(None, None, None);
        assert!(provider.model.is_none());
        assert!(provider.provider.is_none());
        assert!(provider.system_prompt.is_none());
    }

    #[test]
    fn parse_agent_end_event() {
        let ndjson = r#"{"type":"session","version":3}
{"type":"agent_start"}
{"type":"agent_end","messages":[{"role":"user","content":[{"type":"text","text":"test"}]},{"role":"assistant","content":[{"type":"text","text":"[1] 안녕하세요.\n[2] 세계."}],"usage":{"input":100,"output":50}}]}"#;

        let mut text = String::new();
        let mut usage = None;

        for line in ndjson.lines() {
            if let Ok(event) = serde_json::from_str::<PiEvent>(line) {
                if event.event_type == "agent_end" {
                    if let Some(messages) = &event.messages {
                        if let Some(msg) = messages.iter().rev().find(|m| m.role.as_deref() == Some("assistant")) {
                            if let Some(content) = &msg.content {
                                text = content.iter()
                                    .filter(|c| c.content_type == "text")
                                    .filter_map(|c| c.text.as_deref())
                                    .collect::<Vec<_>>()
                                    .join("");
                            }
                            if let Some(u) = &msg.usage {
                                usage = Some(TokenUsage {
                                    input_tokens: u.input.unwrap_or(0),
                                    output_tokens: u.output.unwrap_or(0),
                                });
                            }
                        }
                    }
                }
            }
        }

        let translations = parse_response(&text).unwrap();
        assert_eq!(translations[&1], "안녕하세요.");
        assert_eq!(translations[&2], "세계.");
        assert_eq!(usage.unwrap().input_tokens, 100);
    }

    #[test]
    fn parse_event_with_thinking() {
        let line = r#"{"type":"agent_end","messages":[{"role":"assistant","content":[{"type":"thinking","text":"Let me think..."},{"type":"text","text":"[1] 번역 결과."}],"usage":{"input":50,"output":20}}]}"#;
        let event: PiEvent = serde_json::from_str(line).unwrap();
        assert_eq!(event.event_type, "agent_end");

        let msg = event.messages.unwrap();
        let assistant = &msg[0];
        let content = assistant.content.as_ref().unwrap();
        let text: String = content.iter()
            .filter(|c| c.content_type == "text")
            .filter_map(|c| c.text.as_deref())
            .collect::<Vec<_>>()
            .join("");
        assert_eq!(text, "[1] 번역 결과.");
    }
}
