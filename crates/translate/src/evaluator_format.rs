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

        // A segment's span starts where a line does, so the first character of
        // a translation lands where markup is read.
        if let Some(opened) = line_start_construct(&context.translation)
            && Some(opened) != line_start_construct(&context.source)
        {
            issues.push(EvaluationIssue {
                severity: IssueSeverity::Error,
                kind: IssueKind::FormatLost,
                message: format!(
                    "Translation begins with {opened}, which the source does not. At \
                     the start of a line that is markup, not text — begin with a word \
                     instead, or reorder the sentence."
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

/// The block construct `text` would open if it sat at the start of a line, or
/// `None` for ordinary prose.
///
/// Translations are spliced in at the position their source occupied, which for
/// a paragraph or a list item is the first non-space character of a line. A
/// translation that opens a construct its source did not restructures the
/// document silently: the paragraph becomes a block title, a heading, a table
/// cell. Nothing downstream can tell that apart from markup an author wrote.
///
/// The names cover both parsers, since a translation is checked before anyone
/// knows which file it belongs to.
fn line_start_construct(text: &str) -> Option<&'static str> {
    let text = text.trim_start();
    let mut chars = text.chars();
    let first = chars.next()?;
    let rest = chars.as_str();
    let spaced = rest.starts_with([' ', '\t']);
    match first {
        // `.Title` names the block below it; `...` is an ellipsis.
        '.' if !rest.is_empty() && !rest.starts_with(['.', ' ', '\t']) => Some("a block title (`.`)"),
        '*' | '-' | '+' if spaced => Some("a list item"),
        '=' if spaced => Some("a section title (`=`)"),
        '#' if spaced => Some("a heading (`#`)"),
        '>' if spaced => Some("a block quote (`>`)"),
        '|' => Some("a table cell (`|`)"),
        '/' if rest.starts_with('/') => Some("a comment (`//`)"),
        '[' if text.ends_with(']') => Some("an attribute line (`[...]`)"),
        ':' => {
            let end = rest.find(':')?;
            (end > 0 && !rest[..end].contains(char::is_whitespace))
                .then_some("an attribute entry (`:name:`)")
        }
        _ => None,
    }
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

    /// The case this was written for: "The .erlang.crypt file should contain…"
    /// came back starting with the filename, and asciidoctor read the whole
    /// paragraph as the title of the block below it.
    #[tokio::test]
    async fn fails_when_the_translation_opens_a_block_title() {
        let ctx = make_context(
            "The .erlang.crypt file should contain a list of tuples.",
            ".erlang.crypt 파일은 튜플 목록을 포함해야 합니다.",
        );
        let result = FormatEvaluator.evaluate(&ctx).await.unwrap();
        assert!(!result.passed);
    }

    #[tokio::test]
    async fn allows_a_construct_the_source_already_had() {
        // A list item's span excludes its marker, so both sides start with one
        // only when the text itself does.
        let ctx = make_context("| Instruction", "| 명령어");
        assert!(FormatEvaluator.evaluate(&ctx).await.unwrap().passed);
    }

    #[tokio::test]
    async fn ordinary_prose_is_not_markup() {
        for (source, translation) in [
            ("Wait for it...", "기다려 보세요..."),
            ("It costs -5 dollars.", "-5달러입니다."),
            ("Ratio 3:1 applies.", "비율 3:1이 적용됩니다."),
        ] {
            let ctx = make_context(source, translation);
            let result = FormatEvaluator.evaluate(&ctx).await.unwrap();
            assert!(result.passed, "{translation:?} should read as prose");
        }
    }

    #[test]
    fn constructs_are_recognised_at_line_start() {
        assert_eq!(line_start_construct("= Title"), Some("a section title (`=`)"));
        assert_eq!(line_start_construct("* item"), Some("a list item"));
        assert_eq!(line_start_construct("[source,erlang]"), Some("an attribute line (`[...]`)"));
        assert_eq!(line_start_construct(":toc: left"), Some("an attribute entry (`:name:`)"));
        assert_eq!(line_start_construct("// note"), Some("a comment (`//`)"));
        assert_eq!(line_start_construct("보통 문장입니다."), None);
        assert_eq!(line_start_construct("3.14 입니다."), None);
    }

    #[tokio::test]
    async fn fails_when_code_lost() {
        let ctx = make_context("Use `func()` here.", "여기서 func()를 사용하세요.");
        let result = FormatEvaluator.evaluate(&ctx).await.unwrap();
        assert!(!result.passed);
    }
}
