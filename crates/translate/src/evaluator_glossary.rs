use crate::evaluator::*;
use async_trait::async_trait;
use yeokja_core::glossary::find_terms_in_text;

pub struct GlossaryEvaluator;

#[async_trait]
impl TranslationEvaluator for GlossaryEvaluator {
    async fn evaluate(
        &self,
        context: &EvaluationContext,
    ) -> Result<EvaluationResult, EvaluationError> {
        let mut issues = Vec::new();

        let matching_terms = find_terms_in_text(&context.glossary, &context.source);

        for (term, expected_translation) in &matching_terms {
            // Check if translation contains the expected translation
            if !context.translation.contains(expected_translation.as_str()) {
                issues.push(EvaluationIssue {
                    severity: IssueSeverity::Error,
                    kind: IssueKind::GlossaryMismatch,
                    message: format!(
                        "Term '{}' should be translated as '{}' but was not found in translation",
                        term, expected_translation
                    ),
                });
            }
        }

        Ok(EvaluationResult {
            passed: issues.is_empty(),
            issues,
        })
    }

    fn triggers_retranslation(&self) -> bool {
        true
    }

    fn name(&self) -> &'static str {
        "Glossary"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_context(
        source: &str,
        translation: &str,
        glossary: Vec<(&str, &str)>,
    ) -> EvaluationContext {
        EvaluationContext {
            source: source.to_string(),
            translation: translation.to_string(),
            glossary: glossary
                .into_iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
            source_lang: "en".to_string(),
            target_lang: "ko".to_string(),
        }
    }

    #[tokio::test]
    async fn passes_when_glossary_terms_used_correctly() {
        let ctx = make_context(
            "The repository is ready.",
            "저장소가 준비되었습니다.",
            vec![("repository", "저장소")],
        );
        let result = GlossaryEvaluator.evaluate(&ctx).await.unwrap();
        assert!(result.passed);
        assert!(result.issues.is_empty());
    }

    #[tokio::test]
    async fn fails_when_glossary_term_not_used() {
        let ctx = make_context(
            "The repository is ready.",
            "레포지토리가 준비되었습니다.",
            vec![("repository", "저장소")],
        );
        let result = GlossaryEvaluator.evaluate(&ctx).await.unwrap();
        assert!(!result.passed);
        assert_eq!(result.issues.len(), 1);
        assert_eq!(result.issues[0].kind, IssueKind::GlossaryMismatch);
    }

    #[tokio::test]
    async fn passes_when_term_not_in_source() {
        let ctx = make_context(
            "Hello world.",
            "안녕 세계.",
            vec![("repository", "저장소")],
        );
        let result = GlossaryEvaluator.evaluate(&ctx).await.unwrap();
        assert!(result.passed);
    }
}
