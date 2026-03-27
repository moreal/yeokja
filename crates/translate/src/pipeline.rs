use crate::evaluator::{EvaluationContext, EvaluationResult, TranslationEvaluator};
use crate::provider::{TranslateError, TranslateRequest, TranslationProvider};
use std::collections::HashMap;

/// Result of a single segment through the pipeline.
#[derive(Debug)]
pub struct PipelineResult {
    pub translation: String,
    pub evaluation: Option<EvaluationResult>,
    pub attempts: u32,
}

/// Run a segment through the translate-evaluate-retry pipeline.
pub async fn translate_with_evaluation(
    provider: &dyn TranslationProvider,
    evaluators: &[&dyn TranslationEvaluator],
    request: TranslateRequest,
    glossary: &HashMap<String, String>,
    source_lang: &str,
    target_lang: &str,
    max_retries: u32,
) -> Result<HashMap<usize, PipelineResult>, TranslateError> {
    let mut current_request = request;
    let mut results: HashMap<usize, PipelineResult> = HashMap::new();
    let mut attempts = 0u32;

    loop {
        attempts += 1;
        tracing::debug!(attempt = attempts, "Starting translation attempt");
        let response = provider.translate(current_request.clone()).await?;

        // Evaluate each translated segment
        let mut all_passed = true;
        let mut feedback_parts: Vec<String> = Vec::new();

        for (&idx, translation) in &response.translations {
            let source = current_request
                .segments
                .iter()
                .find(|(i, _)| *i == idx)
                .map(|(_, s)| s.as_str())
                .unwrap_or("");

            let eval_ctx = EvaluationContext {
                source: source.to_string(),
                translation: translation.clone(),
                glossary: glossary.clone(),
                source_lang: source_lang.to_string(),
                target_lang: target_lang.to_string(),
            };

            let mut combined_result = EvaluationResult {
                passed: true,
                issues: Vec::new(),
            };

            for evaluator in evaluators {
                if let Ok(result) = evaluator.evaluate(&eval_ctx).await {
                    if !result.passed && evaluator.triggers_retranslation() {
                        all_passed = false;
                        for issue in &result.issues {
                            feedback_parts.push(issue.message.clone());
                        }
                    }
                    combined_result.issues.extend(result.issues);
                    if !result.passed {
                        combined_result.passed = false;
                    }
                }
            }

            tracing::debug!(idx, passed = combined_result.passed, "Evaluation result");
            results.insert(
                idx,
                PipelineResult {
                    translation: translation.clone(),
                    evaluation: Some(combined_result),
                    attempts,
                },
            );
        }

        if all_passed || attempts > max_retries {
            if !all_passed {
                tracing::warn!(attempts, "Max retries exceeded");
            }
            break;
        }

        // Retry with feedback
        tracing::info!(attempt = attempts, "Retrying translation with feedback");
        current_request.feedback = Some(feedback_parts.join("\n"));
    }

    Ok(results)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::evaluator::*;
    use crate::provider::*;
    use async_trait::async_trait;

    struct MockProvider {
        responses: std::sync::Mutex<Vec<HashMap<usize, String>>>,
    }

    impl MockProvider {
        fn new(responses: Vec<HashMap<usize, String>>) -> Self {
            Self {
                responses: std::sync::Mutex::new(responses),
            }
        }
    }

    #[async_trait]
    impl TranslationProvider for MockProvider {
        async fn translate(
            &self,
            _request: TranslateRequest,
        ) -> Result<TranslateResponse, TranslateError> {
            let mut responses = self.responses.lock().unwrap();
            let translations = if responses.is_empty() {
                HashMap::new()
            } else {
                responses.remove(0)
            };
            Ok(TranslateResponse {
                translations,
                usage: None,
            })
        }
    }

    struct AlwaysPassEvaluator;

    #[async_trait]
    impl TranslationEvaluator for AlwaysPassEvaluator {
        async fn evaluate(
            &self,
            _ctx: &EvaluationContext,
        ) -> Result<EvaluationResult, EvaluationError> {
            Ok(EvaluationResult {
                passed: true,
                issues: Vec::new(),
            })
        }
    }

    struct FailOnceEvaluator {
        call_count: std::sync::Mutex<u32>,
    }

    impl FailOnceEvaluator {
        fn new() -> Self {
            Self {
                call_count: std::sync::Mutex::new(0),
            }
        }
    }

    #[async_trait]
    impl TranslationEvaluator for FailOnceEvaluator {
        async fn evaluate(
            &self,
            _ctx: &EvaluationContext,
        ) -> Result<EvaluationResult, EvaluationError> {
            let mut count = self.call_count.lock().unwrap();
            *count += 1;
            if *count <= 1 {
                Ok(EvaluationResult {
                    passed: false,
                    issues: vec![EvaluationIssue {
                        severity: IssueSeverity::Error,
                        kind: IssueKind::GlossaryMismatch,
                        message: "Wrong term".to_string(),
                    }],
                })
            } else {
                Ok(EvaluationResult {
                    passed: true,
                    issues: Vec::new(),
                })
            }
        }
    }

    #[tokio::test]
    async fn pipeline_passes_on_first_try() {
        let provider = MockProvider::new(vec![[(1, "안녕하세요.".to_string())].into()]);
        let evaluators: Vec<&dyn TranslationEvaluator> = vec![&AlwaysPassEvaluator];
        let request = TranslateRequest {
            segments: vec![(1, "Hello.".to_string())],
            block_context: "Hello.".to_string(),
            glossary: HashMap::new(),
            source_lang: "en".to_string(),
            target_lang: "ko".to_string(),
            feedback: None,
        };

        let results = translate_with_evaluation(
            &provider,
            &evaluators,
            request,
            &HashMap::new(),
            "en",
            "ko",
            3,
        )
        .await
        .unwrap();

        assert_eq!(results[&1].translation, "안녕하세요.");
        assert_eq!(results[&1].attempts, 1);
    }

    #[tokio::test]
    async fn pipeline_retries_on_failure() {
        let provider = MockProvider::new(vec![
            [(1, "레포지토리".to_string())].into(),
            [(1, "저장소".to_string())].into(),
        ]);
        let evaluator = FailOnceEvaluator::new();
        let evaluators: Vec<&dyn TranslationEvaluator> = vec![&evaluator];
        let request = TranslateRequest {
            segments: vec![(1, "repository".to_string())],
            block_context: "repository".to_string(),
            glossary: HashMap::new(),
            source_lang: "en".to_string(),
            target_lang: "ko".to_string(),
            feedback: None,
        };

        let results = translate_with_evaluation(
            &provider,
            &evaluators,
            request,
            &HashMap::new(),
            "en",
            "ko",
            3,
        )
        .await
        .unwrap();

        assert_eq!(results[&1].translation, "저장소");
        assert_eq!(results[&1].attempts, 2);
    }

    #[tokio::test]
    async fn pipeline_stops_after_max_retries() {
        let mut responses = Vec::new();
        for _ in 0..5 {
            responses.push([(1, "bad".to_string())].into());
        }
        let provider = MockProvider::new(responses);

        struct AlwaysFailEvaluator;
        #[async_trait]
        impl TranslationEvaluator for AlwaysFailEvaluator {
            async fn evaluate(
                &self,
                _ctx: &EvaluationContext,
            ) -> Result<EvaluationResult, EvaluationError> {
                Ok(EvaluationResult {
                    passed: false,
                    issues: vec![EvaluationIssue {
                        severity: IssueSeverity::Error,
                        kind: IssueKind::GlossaryMismatch,
                        message: "Always fails".to_string(),
                    }],
                })
            }
        }

        let evaluators: Vec<&dyn TranslationEvaluator> = vec![&AlwaysFailEvaluator];
        let request = TranslateRequest {
            segments: vec![(1, "test".to_string())],
            block_context: "test".to_string(),
            glossary: HashMap::new(),
            source_lang: "en".to_string(),
            target_lang: "ko".to_string(),
            feedback: None,
        };

        let results = translate_with_evaluation(
            &provider,
            &evaluators,
            request,
            &HashMap::new(),
            "en",
            "ko",
            3,
        )
        .await
        .unwrap();

        // Should stop after max_retries + 1 attempts (initial + 3 retries = 4)
        assert!(results[&1].attempts <= 4);
        assert_eq!(results[&1].translation, "bad");
    }
}
