use crate::evaluator::{EvaluationContext, EvaluationResult, Markup, TranslationEvaluator};
use crate::provider::{TranslateError, TranslateRequest, TranslationProvider};
use std::collections::HashMap;

/// Result of a single segment through the pipeline.
#[derive(Debug)]
pub struct PipelineResult {
    pub translation: String,
    pub evaluation: Option<EvaluationResult>,
    pub attempts: u32,
}

/// Observable milestones inside the translate-evaluate-retry loop, so callers
/// can surface what a block is doing while it waits on the LLM.
#[derive(Debug, Clone)]
pub enum PipelineEvent {
    /// A translation request is about to be sent (`attempt` is 1-based).
    AttemptStarted { attempt: u32 },
    /// The LLM answered; evaluation is about to run.
    Translated { attempt: u32 },
    /// Evaluators finished for this attempt. `passed` is false only when an
    /// evaluator that triggers retranslation rejected the result.
    Evaluated {
        attempt: u32,
        passed: bool,
        issues: Vec<String>,
    },
}

/// Callback invoked on each [`PipelineEvent`].
pub type PipelineObserver<'a> = &'a (dyn Fn(PipelineEvent) + Send + Sync);

/// Run a block through the translate-evaluate-retry pipeline.
#[allow(clippy::too_many_arguments)]
pub async fn translate_with_evaluation(
    provider: &dyn TranslationProvider,
    evaluators: &[&dyn TranslationEvaluator],
    request: TranslateRequest,
    glossary: &HashMap<String, String>,
    source_lang: &str,
    target_lang: &str,
    markup: Markup,
    max_retries: u32,
) -> Result<HashMap<usize, PipelineResult>, TranslateError> {
    translate_with_evaluation_observed(
        provider,
        evaluators,
        request,
        glossary,
        source_lang,
        target_lang,
        markup,
        max_retries,
        &|_| {},
    )
    .await
}

/// [`translate_with_evaluation`], reporting each attempt and evaluation to
/// `on_event` as it happens.
#[allow(clippy::too_many_arguments)]
pub async fn translate_with_evaluation_observed(
    provider: &dyn TranslationProvider,
    evaluators: &[&dyn TranslationEvaluator],
    request: TranslateRequest,
    glossary: &HashMap<String, String>,
    source_lang: &str,
    target_lang: &str,
    markup: Markup,
    max_retries: u32,
    on_event: PipelineObserver<'_>,
) -> Result<HashMap<usize, PipelineResult>, TranslateError> {
    let mut current_request = request;
    let mut results: HashMap<usize, PipelineResult> = HashMap::new();
    let mut attempts = 0u32;

    loop {
        attempts += 1;
        tracing::debug!(attempt = attempts, "Starting translation attempt");
        on_event(PipelineEvent::AttemptStarted { attempt: attempts });
        let response = provider.translate(current_request.clone()).await?;
        on_event(PipelineEvent::Translated { attempt: attempts });

        // Evaluate each translated segment. Passed segments leave the retry
        // set immediately: retranslating an entire multi-block batch because
        // one item lost a brace is slower and can regress translations that
        // were already correct.
        let mut retry_segments = Vec::new();
        let mut feedback_parts: Vec<String> = Vec::new();
        let mut attempt_issues: Vec<String> = Vec::new();

        for (idx, source) in &current_request.segments {
            if !response.translations.contains_key(idx) {
                retry_segments.push((*idx, source.clone()));
                let message =
                    format!("Missing translation for [{idx}]; include it in the response.");
                feedback_parts.push(message.clone());
                attempt_issues.push(message);
            }
        }

        for (&idx, translation) in &response.translations {
            let source = current_request
                .segments
                .iter()
                .find(|(i, _)| *i == idx)
                .map(|(_, s)| s.as_str())
                .unwrap_or("");
            let translation = if markup == Markup::Verso {
                crate::evaluator_format::restore_verso_code_whitespace(source, translation)
            } else {
                translation.clone()
            };

            let eval_ctx = EvaluationContext {
                source: source.to_string(),
                translation: translation.clone(),
                glossary: glossary.clone(),
                source_lang: source_lang.to_string(),
                target_lang: target_lang.to_string(),
                markup,
            };

            let mut combined_result = EvaluationResult {
                passed: true,
                issues: Vec::new(),
            };
            let mut triggers_retry = false;

            for evaluator in evaluators {
                if let Ok(result) = evaluator.evaluate(&eval_ctx).await {
                    if !result.passed && evaluator.triggers_retranslation() {
                        triggers_retry = true;
                        for issue in &result.issues {
                            feedback_parts.push(format!("[{idx}] {}", issue.message));
                        }
                    }
                    combined_result.issues.extend(result.issues);
                    if !result.passed {
                        combined_result.passed = false;
                    }
                }
            }

            tracing::debug!(idx, passed = combined_result.passed, "Evaluation result");
            attempt_issues.extend(combined_result.issues.iter().map(|i| i.message.clone()));
            results.insert(
                idx,
                PipelineResult {
                    translation,
                    evaluation: Some(combined_result),
                    attempts,
                },
            );
            if triggers_retry
                && let Some((_, source)) = current_request
                    .segments
                    .iter()
                    .find(|(request_idx, _)| *request_idx == idx)
            {
                retry_segments.push((idx, source.clone()));
            }
        }

        retry_segments.sort_by_key(|(idx, _)| *idx);
        retry_segments.dedup_by_key(|(idx, _)| *idx);
        let all_passed = retry_segments.is_empty();

        on_event(PipelineEvent::Evaluated {
            attempt: attempts,
            passed: all_passed,
            issues: attempt_issues,
        });

        if all_passed || attempts > max_retries {
            if !all_passed {
                tracing::warn!(attempts, "Max retries exceeded");
            }
            break;
        }

        // Retry with feedback
        tracing::info!(attempt = attempts, "Retrying translation with feedback");
        current_request.segments = retry_segments;
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
        requests: std::sync::Mutex<Vec<Vec<usize>>>,
    }

    impl MockProvider {
        fn new(responses: Vec<HashMap<usize, String>>) -> Self {
            Self {
                responses: std::sync::Mutex::new(responses),
                requests: std::sync::Mutex::new(Vec::new()),
            }
        }

        fn requests(&self) -> Vec<Vec<usize>> {
            self.requests.lock().unwrap().clone()
        }
    }

    #[async_trait]
    impl TranslationProvider for MockProvider {
        async fn translate(
            &self,
            request: TranslateRequest,
        ) -> Result<TranslateResponse, TranslateError> {
            self.requests
                .lock()
                .unwrap()
                .push(request.segments.iter().map(|(idx, _)| *idx).collect());
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

    struct RejectBad;

    #[async_trait]
    impl TranslationEvaluator for RejectBad {
        async fn evaluate(
            &self,
            context: &EvaluationContext,
        ) -> Result<EvaluationResult, EvaluationError> {
            let passed = context.translation != "bad";
            Ok(EvaluationResult {
                passed,
                issues: if passed {
                    Vec::new()
                } else {
                    vec![EvaluationIssue {
                        severity: IssueSeverity::Error,
                        kind: IssueKind::FormatLost,
                        message: "bad translation".to_string(),
                    }]
                },
            })
        }

        fn triggers_retranslation(&self) -> bool {
            true
        }

        fn name(&self) -> &'static str {
            "RejectBad"
        }
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
            markup: Markup::Markdown,
            feedback: None,
            prompt_template: None,
        };

        let results = translate_with_evaluation(
            &provider,
            &evaluators,
            request,
            &HashMap::new(),
            "en",
            "ko",
            Markup::Markdown,
            3,
        )
        .await
        .unwrap();

        assert_eq!(results[&1].translation, "안녕하세요.");
        assert_eq!(results[&1].attempts, 1);
    }

    #[tokio::test]
    async fn retry_contains_only_failed_and_missing_segments() {
        let provider = MockProvider::new(vec![
            [(1, "통과합니다.".to_string()), (2, "bad".to_string())].into(),
            [
                (2, "고쳤습니다.".to_string()),
                (3, "추가했습니다.".to_string()),
            ]
            .into(),
        ]);
        let evaluators: Vec<&dyn TranslationEvaluator> = vec![&RejectBad];
        let request = TranslateRequest {
            segments: vec![
                (1, "First.".to_string()),
                (2, "Second.".to_string()),
                (3, "Third.".to_string()),
            ],
            block_context: "First. Second. Third.".to_string(),
            glossary: HashMap::new(),
            source_lang: "en".to_string(),
            target_lang: "ko".to_string(),
            markup: Markup::Markdown,
            feedback: None,
            prompt_template: None,
        };

        let result = translate_with_evaluation(
            &provider,
            &evaluators,
            request,
            &HashMap::new(),
            "en",
            "ko",
            Markup::Markdown,
            3,
        )
        .await
        .unwrap();

        assert_eq!(provider.requests(), [vec![1, 2, 3], vec![2, 3]]);
        assert_eq!(result[&1].translation, "통과합니다.");
        assert_eq!(result[&2].translation, "고쳤습니다.");
        assert_eq!(result[&3].translation, "추가했습니다.");
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
            markup: Markup::Markdown,
            feedback: None,
            prompt_template: None,
        };

        let results = translate_with_evaluation(
            &provider,
            &evaluators,
            request,
            &HashMap::new(),
            "en",
            "ko",
            Markup::Markdown,
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
            markup: Markup::Markdown,
            feedback: None,
            prompt_template: None,
        };

        let results = translate_with_evaluation(
            &provider,
            &evaluators,
            request,
            &HashMap::new(),
            "en",
            "ko",
            Markup::Markdown,
            3,
        )
        .await
        .unwrap();

        // Should stop after max_retries + 1 attempts (initial + 3 retries = 4)
        assert!(results[&1].attempts <= 4);
        assert_eq!(results[&1].translation, "bad");
    }
}
