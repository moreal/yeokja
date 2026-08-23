use crate::evaluator::*;
use async_trait::async_trait;

/// Holds Korean translations to one register.
///
/// A document translated block by block has no memory of its own voice: the
/// model writes ~합니다 in one block and ~한다 in the next, both natural on
/// their own, and the seams only show when the document is read whole. An
/// audit of theBeamBook found one prose sentence in five off-register. The
/// register itself is a project-wide editorial choice; this codebase writes
/// its documents in 합쇼체, so translations follow it.
///
/// The check is mechanical — endings are morphology, not judgment — which is
/// what lets it trigger re-translation under the design spec, unlike the
/// LLM-judged StyleEvaluator.
pub struct EndingEvaluator;

#[async_trait]
impl TranslationEvaluator for EndingEvaluator {
    async fn evaluate(
        &self,
        context: &EvaluationContext,
    ) -> Result<EvaluationResult, EvaluationError> {
        let mut issues = Vec::new();

        // Only Korean has this shape of register, and only prose is judged:
        // a source that does not end a sentence is a title or a fragment,
        // free to end on a noun.
        if context.target_lang.starts_with("ko") && context.source.trim_end().ends_with('.') {
            let offending = off_register_endings(&context.translation);
            if !offending.is_empty() {
                issues.push(EvaluationIssue {
                    severity: IssueSeverity::Error,
                    kind: IssueKind::StyleIssue,
                    message: format!(
                        "Sentence endings leave the document's register: {}. This document \
                         is uniformly 합쇼체 — end declaratives in ~합니다/~입니다, \
                         imperatives in ~하십시오, and proposals in ~ㅂ시다/~읍시다; \
                         do not use plain ~다/~이다 or polite ~요/~세요.",
                        offending.join(", "),
                    ),
                });
            }
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
        "Ending"
    }
}

/// Every sentence ending in `text` that leaves 합쇼체, quoted with its period.
///
/// A sentence ends where a period follows Hangul, so numbers, URLs, and code
/// never enter the judgment. The trailing syllable run decides the register:
/// ~니다 and the proposal endings ~ㅂ시다/~읍시다 are 합쇼체; any other ~다
/// is the plain register; the two-syllable ~요 forms are the polite register. A bare final 요 is left alone — 필요 and
/// 중요 end nouns, not sentences — and so are noun endings generally, since a
/// fragment rendered as a noun phrase is a translator's legitimate choice.
fn off_register_endings(text: &str) -> Vec<String> {
    const POLITE: [&str; 9] = [
        "세요", "어요", "아요", "해요", "예요", "에요", "네요", "지요", "죠",
    ];
    let mut found = Vec::new();
    for piece in text.split('.') {
        let tail: String = {
            let run: Vec<char> = piece
                .chars()
                .rev()
                .take_while(|c| ('가'..='힣').contains(c))
                .collect();
            run.into_iter().rev().collect()
        };
        if tail.is_empty() || !piece.ends_with(&tail) {
            continue;
        }
        let off = (tail.ends_with('다') && !is_hapsyoche_da_ending(&tail))
            || POLITE.iter().any(|p| tail.ends_with(p));
        if off {
            let shown: String = tail
                .chars()
                .rev()
                .take(6)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect();
            found.push(format!("…{shown}."));
        }
    }
    found
}

fn is_hapsyoche_da_ending(tail: &str) -> bool {
    if tail.ends_with("니다") || tail.ends_with("읍시다") {
        return true;
    }
    let Some(stem) = tail.strip_suffix("시다") else {
        return false;
    };
    // In forms such as 합시다, 봅시다, 둡시다, the syllable before 시다 has
    // jongseong ㅂ (index 17 in the Unicode Hangul composition formula).
    stem.chars()
        .next_back()
        .is_some_and(|ch| ('가'..='힣').contains(&ch) && (ch as u32 - 0xAC00) % 28 == 17)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn context(source: &str, translation: &str) -> EvaluationContext {
        EvaluationContext {
            source: source.to_string(),
            translation: translation.to_string(),
            glossary: HashMap::new(),
            source_lang: "en".to_string(),
            target_lang: "ko".to_string(),
            markup: Markup::Rst,
        }
    }

    #[tokio::test]
    async fn formal_register_passes() {
        let ctx = context(
            "The scheduler is responsible for the guarantees.",
            "스케줄러는 보장을 담당합니다.",
        );
        assert!(EndingEvaluator.evaluate(&ctx).await.unwrap().passed);
    }

    #[tokio::test]
    async fn formal_proposals_pass() {
        for translation in [
            "이제 $G$를 유한군이라고 합시다.",
            "다음 예를 살펴봅시다.",
            "두 집합을 잡읍시다.",
        ] {
            let ctx = context("Let us continue.", translation);
            assert!(
                EndingEvaluator.evaluate(&ctx).await.unwrap().passed,
                "{translation:?} should pass"
            );
        }
    }

    /// The audit's most common find: a block translated whole in the plain
    /// register inside a 합쇼체 document.
    #[tokio::test]
    async fn plain_register_fails() {
        let ctx = context(
            "The scheduler is responsible for the guarantees.",
            "스케줄러는 시스템의 실시간 보장을 담당한다.",
        );
        let result = EndingEvaluator.evaluate(&ctx).await.unwrap();
        assert!(!result.passed);
        assert!(result.issues[0].message.contains("담당한다."));
    }

    /// 88 of theBeamBook's 어체 findings were ~하세요 imperatives.
    #[tokio::test]
    async fn polite_imperative_fails() {
        let ctx = context(
            "See the illustration for details.",
            "자세한 내용은 그림을 참고하세요.",
        );
        assert!(!EndingEvaluator.evaluate(&ctx).await.unwrap().passed);
    }

    /// Mixing inside one segment: the final sentence is the polite register
    /// even though the first is formal.
    #[tokio::test]
    async fn a_mixed_segment_names_the_offending_sentence() {
        let ctx = context(
            "This knowledge is essential. See the memory chapter.",
            "이 지식은 필수적입니다. 메모리 장을 참고하세요.",
        );
        let result = EndingEvaluator.evaluate(&ctx).await.unwrap();
        assert!(!result.passed);
        assert!(result.issues[0].message.contains("참고하세요."));
        assert!(!result.issues[0].message.contains("필수적입니다"));
    }

    #[test]
    fn nouns_numbers_and_code_are_not_sentences() {
        for text in [
            "이 작업에는 재컴파일이 필요.", // bare 요 ends a noun
            "버전은 3.14입니다.",
            "https://doc.pypy.org 를 보십시오.",
            "명령형은 ~하십시오.",
            "프로세스 생성 방법.", // noun phrase rendering
        ] {
            assert!(
                off_register_endings(text).is_empty(),
                "{text:?} should pass"
            );
        }
    }

    #[tokio::test]
    async fn titles_are_not_judged() {
        // The source carries no period, so the segment is a title or fragment.
        let ctx = context("Process Management", "프로세스를 관리한다");
        assert!(EndingEvaluator.evaluate(&ctx).await.unwrap().passed);
    }

    #[tokio::test]
    async fn other_target_languages_are_left_alone() {
        let mut ctx = context("It runs.", "それは動く。");
        ctx.target_lang = "ja".to_string();
        assert!(EndingEvaluator.evaluate(&ctx).await.unwrap().passed);
    }
}
