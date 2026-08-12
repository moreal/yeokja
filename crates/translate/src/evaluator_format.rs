use crate::evaluator::*;
use async_trait::async_trait;

pub struct FormatEvaluator;

#[async_trait]
impl TranslationEvaluator for FormatEvaluator {
    async fn evaluate(
        &self,
        context: &EvaluationContext,
    ) -> Result<EvaluationResult, EvaluationError> {
        let mut issues = Vec::new();

        // Check bold markers
        let source_bold = count_pattern(&context.source, "**");
        let trans_bold = count_pattern(&context.translation, "**");
        if source_bold != trans_bold {
            issues.push(EvaluationIssue {
                severity: IssueSeverity::Error,
                kind: IssueKind::FormatLost,
                message: format!(
                    "Bold markers (**) count mismatch: source has {}, translation has {}",
                    source_bold, trans_bold
                ),
            });
        }

        // Check italic markers (single *)
        let source_italic = count_single_asterisks(&context.source);
        let trans_italic = count_single_asterisks(&context.translation);
        if source_italic != trans_italic {
            issues.push(EvaluationIssue {
                severity: IssueSeverity::Warning,
                kind: IssueKind::FormatLost,
                message: format!(
                    "Italic markers (*) count mismatch: source has {}, translation has {}",
                    source_italic, trans_italic
                ),
            });
        }

        // Check inline code
        let source_code = count_pattern(&context.source, "`");
        let trans_code = count_pattern(&context.translation, "`");
        if source_code != trans_code {
            issues.push(EvaluationIssue {
                severity: IssueSeverity::Error,
                kind: IssueKind::FormatLost,
                message: format!(
                    "Inline code markers (`) count mismatch: source has {}, translation has {}",
                    source_code, trans_code
                ),
            });
        }

        let has_errors = issues.iter().any(|i| i.severity == IssueSeverity::Error);
        Ok(EvaluationResult {
            passed: !has_errors,
            issues,
        })
    }

    fn triggers_retranslation(&self) -> bool {
        true
    }

    fn name(&self) -> &'static str {
        "Format"
    }
}

fn count_pattern(text: &str, pattern: &str) -> usize {
    text.matches(pattern).count()
}

/// Count single asterisks (not part of **) for italic detection.
fn count_single_asterisks(text: &str) -> usize {
    let total = text.matches('*').count();
    let double = count_pattern(text, "**") * 2;
    total.saturating_sub(double)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn make_context(source: &str, translation: &str) -> EvaluationContext {
        EvaluationContext {
            source: source.to_string(),
            translation: translation.to_string(),
            glossary: HashMap::new(),
            source_lang: "en".to_string(),
            target_lang: "ko".to_string(),
        }
    }

    #[tokio::test]
    async fn passes_when_formatting_preserved() {
        let ctx = make_context(
            "This is **bold** and `code`.",
            "이것은 **굵게** 그리고 `코드`.",
        );
        let result = FormatEvaluator.evaluate(&ctx).await.unwrap();
        assert!(result.passed);
    }

    #[tokio::test]
    async fn fails_when_bold_lost() {
        let ctx = make_context("This is **bold**.", "이것은 굵게.");
        let result = FormatEvaluator.evaluate(&ctx).await.unwrap();
        assert!(!result.passed);
    }

    #[tokio::test]
    async fn fails_when_code_lost() {
        let ctx = make_context("Use `func()` here.", "여기서 func()를 사용하세요.");
        let result = FormatEvaluator.evaluate(&ctx).await.unwrap();
        assert!(!result.passed);
    }
}
