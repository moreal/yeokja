use crate::evaluator::*;
use crate::provider::{CompletionRequest, LlmProvider};
use async_trait::async_trait;
use std::sync::Arc;

pub struct StyleEvaluator {
    provider: Arc<dyn LlmProvider>,
}

impl StyleEvaluator {
    pub fn new(provider: Arc<dyn LlmProvider>, _target_lang: String) -> Self {
        Self { provider }
    }
}

#[async_trait]
impl TranslationEvaluator for StyleEvaluator {
    async fn evaluate(&self, context: &EvaluationContext) -> Result<EvaluationResult, EvaluationError> {
        let prompt = format!(
            r#"You are a translation quality evaluator. Evaluate the following translation from {} to {}.

Source: {}
Translation: {}

Rate the translation on these criteria:
1. Naturalness - Does it read naturally in the target language?
2. Accuracy - Does it convey the same meaning as the source?
3. Style consistency - Is the writing style appropriate?

Respond with ONLY one of these ratings on the first line:
GOOD - The translation is natural and accurate
ACCEPTABLE - Minor issues but understandable
POOR - Significant issues with naturalness or accuracy

Then on the next line, briefly explain why (one sentence)."#,
            context.source_lang, context.target_lang, context.source, context.translation
        );

        let response = self.provider.complete(CompletionRequest { prompt }).await.map_err(|e| {
            EvaluationError::Failed(format!("LLM evaluation failed: {e}"))
        })?;

        let reply = response.text;

        let (passed, issues) = parse_style_response(&reply);

        Ok(EvaluationResult { passed, issues })
    }

    /// LLM-as-judge results only produce warnings; per the design spec they
    /// never trigger re-translation (only mechanical checks do).
    fn triggers_retranslation(&self) -> bool {
        false
    }

    fn name(&self) -> &'static str {
        "Style"
    }
}

fn parse_style_response(response: &str) -> (bool, Vec<EvaluationIssue>) {
    let lines: Vec<&str> = response.lines().collect();
    let first_line = lines.first().map(|l| l.trim().to_uppercase()).unwrap_or_default();
    let explanation = lines.get(1).map(|l| l.trim().to_string()).unwrap_or_default();

    if first_line.starts_with("GOOD") {
        (true, Vec::new())
    } else if first_line.starts_with("ACCEPTABLE") {
        (
            true,
            vec![EvaluationIssue {
                severity: IssueSeverity::Warning,
                kind: IssueKind::StyleIssue,
                message: if explanation.is_empty() {
                    "Translation is acceptable but could be improved".to_string()
                } else {
                    explanation
                },
            }],
        )
    } else {
        // POOR or unrecognized
        (
            false,
            vec![EvaluationIssue {
                severity: IssueSeverity::Error,
                kind: IssueKind::StyleIssue,
                message: if explanation.is_empty() {
                    "Translation has style or naturalness issues".to_string()
                } else {
                    explanation
                },
            }],
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_good_response() {
        let (passed, issues) = parse_style_response("GOOD\nTranslation is natural and accurate.");
        assert!(passed);
        assert!(issues.is_empty());
    }

    #[test]
    fn parse_acceptable_response() {
        let (passed, issues) = parse_style_response("ACCEPTABLE\nMinor word choice issue.");
        assert!(passed);
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].severity, IssueSeverity::Warning);
        assert_eq!(issues[0].kind, IssueKind::StyleIssue);
        assert_eq!(issues[0].message, "Minor word choice issue.");
    }

    #[test]
    fn parse_poor_response() {
        let (passed, issues) = parse_style_response("POOR\nTranslation sounds unnatural.");
        assert!(!passed);
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].severity, IssueSeverity::Error);
    }

    #[test]
    fn parse_empty_response() {
        let (passed, issues) = parse_style_response("");
        assert!(!passed);
        assert_eq!(issues.len(), 1);
    }

    #[test]
    fn never_triggers_retranslation() {
        // Per the design spec, StyleEvaluator (LLM-as-judge) only records
        // warnings; re-translation is triggered by mechanical checks only.
        struct Never;
        #[async_trait]
        impl crate::provider::LlmProvider for Never {
            async fn complete(
                &self,
                _request: crate::provider::CompletionRequest,
            ) -> Result<crate::provider::CompletionResponse, crate::provider::TranslateError>
            {
                unreachable!()
            }
        }
        let evaluator = StyleEvaluator::new(Arc::new(Never), "ko".to_string());
        assert!(!evaluator.triggers_retranslation());
    }
}
