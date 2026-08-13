use crate::evaluator::*;
use async_trait::async_trait;

pub struct LinkEvaluator;

#[async_trait]
impl TranslationEvaluator for LinkEvaluator {
    async fn evaluate(
        &self,
        context: &EvaluationContext,
    ) -> Result<EvaluationResult, EvaluationError> {
        let source_urls = extract_urls(&context.source);
        let translation_urls = extract_urls(&context.translation);
        let mut issues = Vec::new();

        for url in &source_urls {
            if !translation_urls.contains(url) {
                issues.push(EvaluationIssue {
                    severity: IssueSeverity::Error,
                    kind: IssueKind::LinkBroken,
                    message: format!(
                        "URL '{}' from source is missing in translation",
                        url
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
        "Link"
    }
}

/// Simple URL extraction — finds http:// and https:// URLs
fn extract_urls(text: &str) -> Vec<String> {
    let mut urls = Vec::new();
    for word in text.split_whitespace() {
        // Find the start of a URL within the token (handles cases like "[text](https://...")
        let start = if let Some(pos) = word.find("https://").or_else(|| word.find("http://")) {
            pos
        } else {
            continue;
        };
        let candidate = &word[start..];
        // Trim trailing punctuation and closing brackets
        let url = candidate.trim_end_matches(['.', ',', ';', ')', '>']);
        if !url.is_empty() {
            urls.push(url.to_string());
        }
    }
    urls
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
            markup: Markup::Asciidoc,
        }
    }

    #[tokio::test]
    async fn passes_when_urls_preserved() {
        let ctx = make_context(
            "Visit https://example.com for details.",
            "자세한 내용은 https://example.com 을 방문하세요.",
        );
        let result = LinkEvaluator.evaluate(&ctx).await.unwrap();
        assert!(result.passed);
    }

    #[tokio::test]
    async fn fails_when_url_missing() {
        let ctx = make_context(
            "Visit https://example.com for details.",
            "자세한 내용은 예시 사이트를 방문하세요.",
        );
        let result = LinkEvaluator.evaluate(&ctx).await.unwrap();
        assert!(!result.passed);
        assert_eq!(result.issues[0].kind, IssueKind::LinkBroken);
    }

    #[tokio::test]
    async fn passes_when_no_urls() {
        let ctx = make_context("Hello world.", "안녕 세계.");
        let result = LinkEvaluator.evaluate(&ctx).await.unwrap();
        assert!(result.passed);
    }

    #[test]
    fn extract_urls_from_markdown() {
        let urls = extract_urls("See [link](https://example.com) and https://other.com.");
        assert!(urls.contains(&"https://example.com".to_string()));
        assert!(urls.contains(&"https://other.com".to_string()));
    }
}
