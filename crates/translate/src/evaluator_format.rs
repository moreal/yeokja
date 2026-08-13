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

        // Counted by run, not by character: `` ``code`` `` marks the same one
        // pair as `` `code` ``, written the way an unclosable pair has to be
        // rewritten (see below). Counting characters would read that fix as two
        // markers the source never had and reject it.
        //
        // For the same reason `*` is one count rather than bold and italic
        // separately: `**bold**` and `*bold*` differ only in whether the pair
        // can close against a word, and a translation may have to switch.
        for (mark, name) in [('`', "Inline code markers (`)"), ('*', "Emphasis markers (*)")] {
            let in_source = mark_runs(&context.source, mark);
            let in_translation = mark_runs(&context.translation, mark);
            if in_source != in_translation {
                issues.push(EvaluationIssue {
                    severity: IssueSeverity::Error,
                    kind: IssueKind::FormatLost,
                    message: format!(
                        "{name} count mismatch: source has {in_source}, \
                         translation has {in_translation}"
                    ),
                });
            }
        }

        // A constrained pair has to close next to a non-word character, and
        // Korean writes its particles straight onto the word before them.
        let unclosable = unclosable_pairs(&context.translation, context.markup);
        if !unclosable.is_empty() && unclosable_pairs(&context.source, context.markup).is_empty() {
            // Name every one of them. A segment often carries several marked-up
            // terms and only some of them take a suffix, so "a pair does not
            // close" leaves the translator guessing which — and it fixes one
            // and leaves the rest.
            let shown: Vec<&str> = unclosable.iter().map(|u| u.text.as_str()).collect();
            let unconstrained = unclosable[0].unconstrained;
            issues.push(EvaluationIssue {
                severity: IssueSeverity::Error,
                kind: IssueKind::FormatLost,
                message: format!(
                    "A letter follows the closing mark in {}, so {} pair never closes: \
                     the marks print as themselves and the run swallows the text after \
                     them. Write each as {} — the doubled form closes anywhere — or put \
                     a space or punctuation after the closing mark.",
                    shown.join(", "),
                    if shown.len() == 1 { "the" } else { "each" },
                    unconstrained,
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

/// How many times `mark` opens or closes an inline pair, counting a run of
/// them once.
fn mark_runs(text: &str, mark: char) -> usize {
    let mut runs = 0;
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c != mark {
            continue;
        }
        runs += 1;
        while chars.peek() == Some(&mark) {
            chars.next();
        }
    }
    runs
}

/// An inline pair that cannot close: the offending text as written, and the
/// form of the pair that survives a word character against its closing mark.
struct Unclosable {
    text: String,
    unconstrained: &'static str,
}

/// Every constrained inline pair `text` opens but cannot close.
///
/// AsciiDoc reads `` `code` `` as code only when neither mark touches a word
/// character, and Asciidoctor spells "word character" `\p{Word}` — which every
/// Hangul syllable satisfies. Korean writes its particles onto the end of the
/// word they attach to, so the natural translation of "on the `heap`" ends
/// `` `heap`에 ``: a pair that never closes. Both marks then print as
/// themselves, and the opening one keeps looking for a partner, swallowing the
/// text up to the next mark in the paragraph.
///
/// Markdown shares the rule only for `_`, where CommonMark rules out intraword
/// emphasis. Its code spans and `*` emphasis close against a word just fine, so
/// checking them there would fail translations that are correct.
fn unclosable_pairs(text: &str, markup: Markup) -> Vec<Unclosable> {
    let marks: &[(char, &'static str)] = match markup {
        Markup::Asciidoc => &[('`', "``code``"), ('*', "**bold**"), ('_', "__italic__")],
        // `__italic__` is bold in Markdown, so the way out is the other mark.
        Markup::Markdown => &[('_', "*italic*")],
    };
    let chars: Vec<char> = text.chars().collect();
    let mut found = Vec::new();
    for &(mark, unconstrained) in marks {
        for span in unclosable_spans(&chars, mark) {
            found.push(Unclosable {
                text: chars[span].iter().collect(),
                unconstrained,
            });
        }
    }
    found
}

/// Pair up `mark` the way a constrained pair is read, and report each pair that
/// opens without being able to close — as the span of the pair plus the letter
/// that holds it open.
fn unclosable_spans(chars: &[char], mark: char) -> Vec<std::ops::Range<usize>> {
    let is_word = |c: char| c.is_alphanumeric() || c == '_';
    let mut spans = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] != mark {
            i += 1;
            continue;
        }
        // Doubled marks are the unconstrained form, which closes anywhere.
        if chars.get(i + 1) == Some(&mark) {
            while i < chars.len() && chars[i] == mark {
                i += 1;
            }
            continue;
        }
        // A mark with a word on its left never opens anything: the underscores
        // in `min_heap_size` are text, not markup.
        let opens = (i == 0 || !is_word(chars[i - 1]))
            && chars.get(i + 1).is_some_and(|c| !c.is_whitespace());
        if !opens {
            i += 1;
            continue;
        }
        // The closing mark is the next one that a space does not precede.
        let mut from = i + 1;
        loop {
            let Some(offset) = chars[from..].iter().position(|c| *c == mark) else {
                return spans; // opened at the end of the text; nothing to pair with
            };
            let close = from + offset;
            if chars[close - 1].is_whitespace() {
                from = close + 1;
                continue;
            }
            if chars.get(close + 1).is_some_and(|c| is_word(*c)) {
                spans.push(i..close + 2);
            }
            i = close + 1;
            break;
        }
    }
    spans
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn make_context(source: &str, translation: &str) -> EvaluationContext {
        context_in(Markup::Asciidoc, source, translation)
    }

    fn context_in(markup: Markup, source: &str, translation: &str) -> EvaluationContext {
        EvaluationContext {
            source: source.to_string(),
            translation: translation.to_string(),
            glossary: HashMap::new(),
            source_lang: "en".to_string(),
            target_lang: "ko".to_string(),
            markup,
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

    /// The case this was written for. 858 of theBeamBook's 1974 code spans came
    /// back with a particle against the closing backtick and stopped rendering.
    #[tokio::test]
    async fn fails_when_a_particle_closes_a_code_span() {
        let ctx = make_context(
            "Perform `is_integer` on x0 and jump to the label on failure.",
            "x0에 대해 `is_integer`를 수행하고, 실패하면 레이블로 점프합니다.",
        );
        let result = FormatEvaluator.evaluate(&ctx).await.unwrap();
        assert!(!result.passed);
        assert!(
            result.issues.iter().any(|i| i.message.contains("``code``")),
            "the message should name the form that works: {:?}",
            result.issues
        );
    }

    #[tokio::test]
    async fn passes_when_the_pair_is_doubled() {
        let ctx = make_context(
            "Perform `is_integer` on x0.",
            "x0에 대해 ``is_integer``를 수행합니다.",
        );
        assert!(FormatEvaluator.evaluate(&ctx).await.unwrap().passed);
    }

    #[tokio::test]
    async fn a_markdown_code_span_closes_against_a_particle() {
        // CommonMark has no flanking rule for code spans, so this renders.
        let ctx = context_in(
            Markup::Markdown,
            "Perform `is_integer` on x0.",
            "x0에 대해 `is_integer`를 수행합니다.",
        );
        assert!(FormatEvaluator.evaluate(&ctx).await.unwrap().passed);
    }

    #[tokio::test]
    async fn a_markdown_underscore_still_has_to_close() {
        // CommonMark does rule out intraword `_` emphasis, and points elsewhere.
        let ctx = context_in(
            Markup::Markdown,
            "The _arity_ is the argument count.",
            "_arity_는 인자의 개수입니다.",
        );
        let result = FormatEvaluator.evaluate(&ctx).await.unwrap();
        assert!(!result.passed);
        assert!(result.issues.iter().any(|i| i.message.contains("*italic*")));
    }

    #[test]
    fn an_identifier_is_not_an_unclosed_italic() {
        for text in [
            "off_heap 할당을 사용합니다.",
            "https://example.com/a_b_c 를 보세요.",
            "인자 수는 2*3 입니다.",
            "함수 인자는 `Arity` 입니다",
            "여는 백틱만 있는 `문장",
        ] {
            assert!(
                unclosable_pairs(text, Markup::Asciidoc).is_empty(),
                "{text:?} carries no unclosable pair"
            );
        }
    }

    /// A segment usually carries several marked-up terms and only some of them
    /// take a suffix. Naming one and stopping got the first fixed and the rest
    /// left alone, so every offender is reported.
    #[test]
    fn every_unclosable_pair_is_named() {
        let text = "이 명령어는 `allocate`와 같지만 스택 슬롯을 `NIL`로 지웁니다.";
        let found = unclosable_pairs(text, Markup::Asciidoc);
        let shown: Vec<&str> = found.iter().map(|u| u.text.as_str()).collect();
        assert_eq!(shown, vec!["`allocate`와", "`NIL`로"]);
    }

    #[tokio::test]
    async fn the_message_quotes_the_offending_text() {
        let ctx = make_context(
            "It works as `allocate` but clears the slots to `NIL`.",
            "`allocate`와 같지만 슬롯을 `NIL`로 지웁니다.",
        );
        let result = FormatEvaluator.evaluate(&ctx).await.unwrap();
        let named = result
            .issues
            .iter()
            .find(|i| i.message.contains("never closes"))
            .expect("the unclosable pair should be reported");
        assert!(named.message.contains("`allocate`와"), "{}", named.message);
        assert!(named.message.contains("`NIL`로"), "{}", named.message);
    }

    #[tokio::test]
    async fn fails_when_code_lost() {
        let ctx = make_context("Use `func()` here.", "여기서 func()를 사용하세요.");
        let result = FormatEvaluator.evaluate(&ctx).await.unwrap();
        assert!(!result.passed);
    }
}
