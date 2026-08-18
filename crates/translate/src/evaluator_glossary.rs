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

        let source = if context.markup == Markup::Verso {
            without_verso_role_headers(&context.source)
        } else {
            context.source.clone()
        };
        let matching_terms = find_terms_in_text(&context.glossary, &source);

        // The source is read without regard to case, so the translation is too.
        // Demanding the glossary's spelling starts a retry that cannot converge:
        // theBeamBook writes "the beam file format" and "erlang:md5/1" in its
        // own prose, and a translator that keeps the author's spelling gives the
        // same answer however many times it is asked. Case is a house-style
        // question, and this evaluator is the wrong place to have it — it costs
        // a whole block's retranslation per disagreement.
        let translation = context.translation.to_lowercase();

        for (term, expected_translation) in &matching_terms {
            if !translation.contains(&expected_translation.to_lowercase()) {
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

/// Remove opaque `{role arguments}` headers while leaving visible labels and
/// surrounding prose available to glossary matching. Backticked role payloads
/// are already masked by `find_terms_in_text` itself.
fn without_verso_role_headers(text: &str) -> String {
    let mut output = String::with_capacity(text.len());
    let mut copied_through = 0;
    let mut at = 0;
    while at < text.len() {
        if text[at..].starts_with('{') {
            let name_start = at + 1;
            let starts_as_role = text[name_start..]
                .chars()
                .next()
                .is_some_and(|ch| ch.is_ascii_alphabetic());
            if starts_as_role
                && let Some(close_offset) = text[name_start..].find('}')
            {
                let end = name_start + close_offset + 1;
                output.push_str(&text[copied_through..at]);
                copied_through = end;
                at = end;
                continue;
            }
        }
        let ch = text[at..].chars().next().unwrap();
        at += ch.len_utf8();
    }
    output.push_str(&text[copied_through..]);
    output
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
            markup: Markup::Asciidoc,
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

    /// A term the source itself spells in another case. The glossary says to
    /// leave "Erlang" alone, and a translation that left `erlang:md5/1` alone
    /// did exactly that.
    #[tokio::test]
    async fn passes_when_the_case_is_the_source_author_s() {
        let ctx = make_context(
            "The key is scrambled using erlang:md5/1.",
            "키는 erlang:md5/1을 사용하여 스크램블됩니다.",
            vec![("Erlang", "Erlang")],
        );
        assert!(GlossaryEvaluator.evaluate(&ctx).await.unwrap().passed);
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

    #[tokio::test]
    async fn verso_role_names_and_code_payloads_are_not_prose_terms() {
        let mut ctx = make_context(
            "Use {tactic}`simp` and {anchorName Monad}`Monad`.",
            "{tactic}`simp`와 {anchorName Monad}`Monad`를 사용합니다.",
            vec![("tactic", "택틱"), ("monad", "모나드")],
        );
        ctx.markup = Markup::Verso;
        let result = GlossaryEvaluator.evaluate(&ctx).await.unwrap();
        assert!(result.passed, "{:?}", result.issues);
    }

    #[tokio::test]
    async fn verso_still_checks_the_same_term_in_visible_prose() {
        let mut ctx = make_context(
            "This monad uses {anchorName Monad}`Monad`.",
            "이 모나드는 {anchorName Monad}`Monad`를 사용합니다.",
            vec![("monad", "모나드")],
        );
        ctx.markup = Markup::Verso;
        assert!(GlossaryEvaluator.evaluate(&ctx).await.unwrap().passed);

        ctx.translation = "이 추상화는 {anchorName Monad}`Monad`를 사용합니다.".to_string();
        assert!(!GlossaryEvaluator.evaluate(&ctx).await.unwrap().passed);
    }
}
