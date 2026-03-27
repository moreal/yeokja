use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct EvaluationContext {
    pub source: String,
    pub translation: String,
    pub glossary: HashMap<String, String>,
    pub source_lang: String,
    pub target_lang: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvaluationResult {
    pub passed: bool,
    pub issues: Vec<EvaluationIssue>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvaluationIssue {
    pub severity: IssueSeverity,
    pub kind: IssueKind,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum IssueSeverity {
    Error,
    Warning,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum IssueKind {
    GlossaryMismatch,
    LinkBroken,
    FormatLost,
    StyleIssue,
}

#[async_trait]
pub trait TranslationEvaluator: Send + Sync {
    async fn evaluate(
        &self,
        context: &EvaluationContext,
    ) -> Result<EvaluationResult, EvaluationError>;

    /// Whether failures from this evaluator should trigger re-translation.
    fn triggers_retranslation(&self) -> bool {
        true
    }
}

#[derive(Debug, thiserror::Error)]
pub enum EvaluationError {
    #[error("Evaluation failed: {0}")]
    Failed(String),
}
