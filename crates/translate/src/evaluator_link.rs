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
                    message: format!("URL '{}' from source is missing in translation", url),
                });
            }
        }

        // A reStructuredText named reference is its own key: `phrase`_ links
        // to the `.. _phrase:` target elsewhere in the file, and targets stay
        // verbatim. Translating the phrase leaves the reference dangling and
        // the backticks printing as themselves.
        if context.markup == Markup::Rst {
            for named in rst_named_references(&context.source) {
                if !context.translation.contains(&named.check) {
                    issues.push(EvaluationIssue {
                        severity: IssueSeverity::Error,
                        kind: IssueKind::LinkBroken,
                        message: format!(
                            "{} names the target it links to, so the phrase has to \
                             survive the translation. Keep it verbatim, or translate the \
                             visible text with an embedded alias: `번역한 텍스트 <{}_>`_",
                            named.shown,
                            named.check.trim_end_matches('_'),
                        ),
                    });
                }
            }
        }

        // Anonymous references pair with their `.. __:` targets by position
        // and count, file-wide. One reference lost in translation — a bare
        // here__ absorbed into prose — breaks every anonymous link in the
        // file, so the count has to survive even though the text may change.
        if context.markup == Markup::Rst {
            let in_source = rst_anonymous_references(&context.source);
            let in_translation = rst_anonymous_references(&context.translation);
            if in_source != in_translation {
                issues.push(EvaluationIssue {
                    severity: IssueSeverity::Error,
                    kind: IssueKind::LinkBroken,
                    message: format!(
                        "Anonymous reference count changed: source has {in_source} \
                         `text`__ / word__ reference(s), translation has {in_translation}. \
                         They pair with their targets by count, so keep exactly as many — \
                         the text before __ may be translated: here__ → `여기`__."
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

/// Every named-reference phrase in reStructuredText `text`: the content of
/// `` `phrase`_ `` spans and bare `word_` references.
///
/// One trailing underscore only — two make the reference anonymous, targeted
/// by position rather than by name. A span holding `<` embeds its own target,
/// so its text is free; and a double-backtick span is a literal, skipped
/// wholesale so the identifiers inside it are never mistaken for references.
struct NamedReference {
    /// What the translation must contain. For a phrase reference the phrase
    /// itself; for a bare-word reference the word with its underscore, since
    /// the word alone appears in any translation of the sentence.
    check: String,
    /// The reference as the source wrote it, for the message.
    shown: String,
}

fn rst_named_references(text: &str) -> Vec<NamedReference> {
    let chars: Vec<char> = text.chars().collect();
    let mut found: Vec<NamedReference> = Vec::new();
    let mut push = |check: String, shown: String| {
        if !found.iter().any(|f| f.check == check) {
            found.push(NamedReference { check, shown });
        }
    };
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '`' {
            let run = chars[i..].iter().take_while(|c| **c == '`').count();
            let content_start = i + run;
            // Find the closing run of the same length.
            let mut j = content_start;
            let close = loop {
                let Some(offset) = chars[j..].iter().position(|c| *c == '`') else {
                    break None;
                };
                let at = j + offset;
                let len = chars[at..].iter().take_while(|c| **c == '`').count();
                if len == run {
                    break Some(at);
                }
                j = at + len;
            };
            let Some(close) = close else {
                i = content_start;
                continue;
            };
            if run == 1 && chars.get(close + 1) == Some(&'_') && chars.get(close + 2) != Some(&'_')
            {
                let content: String = chars[content_start..close].iter().collect();
                if !content.contains('<') {
                    push(content.clone(), format!("`{content}`_"));
                }
            }
            i = close + run;
        } else if c.is_ascii_alphanumeric() {
            // A bare word ending in a single underscore is a reference too:
            // Python_ in prose links to `.. _Python:`.
            let end = chars[i..]
                .iter()
                .take_while(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '+' | '-' | '_'))
                .count();
            let word: String = chars[i..i + end].iter().collect();
            // Sentence punctuation rides along in the token ("Python_." at a
            // sentence's end); the reference stops at the underscore.
            let word = word.trim_end_matches(['.', '+', '-']);
            if let Some(name) = word.strip_suffix('_')
                && !name.is_empty()
                && !name.ends_with('_')
            {
                push(word.to_string(), word.to_string());
            }
            i += end;
        } else {
            i += 1;
        }
    }
    found
}

/// How many anonymous references `text` carries: `` `phrase`__ `` spans and
/// bare `word__` tokens. Double-backtick literals are skipped wholesale, so a
/// ``dunder.__init__()`` never counts.
fn rst_anonymous_references(text: &str) -> usize {
    let chars: Vec<char> = text.chars().collect();
    let mut count = 0;
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '`' {
            let run = chars[i..].iter().take_while(|c| **c == '`').count();
            let mut j = i + run;
            let close = loop {
                let Some(offset) = chars[j..].iter().position(|c| *c == '`') else {
                    break None;
                };
                let at = j + offset;
                let len = chars[at..].iter().take_while(|c| **c == '`').count();
                if len == run {
                    break Some(at);
                }
                j = at + len;
            };
            let Some(close) = close else {
                i += run;
                continue;
            };
            if run == 1
                && chars.get(close + 1) == Some(&'_')
                && chars.get(close + 2) == Some(&'_')
                && chars.get(close + 3) != Some(&'_')
            {
                count += 1;
            }
            i = close + run;
        } else if c.is_alphanumeric() {
            let end = chars[i..]
                .iter()
                .take_while(|c| c.is_alphanumeric() || matches!(c, '.' | '+' | '-' | '_'))
                .count();
            let word: String = chars[i..i + end].iter().collect();
            let word = word.trim_end_matches(['.', '+', '-']);
            if let Some(name) = word.strip_suffix("__")
                && !name.is_empty()
                && !name.ends_with('_')
            {
                count += 1;
            }
            i += end;
        } else {
            i += 1;
        }
    }
    count
}

/// Extract HTTP(S) URLs without absorbing markup closers or Korean suffixes.
///
/// In `[label](https://example.com)에서`, whitespace tokenization alone reads
/// `)에서` as part of the URL. Parentheses that occur inside a URL are balanced,
/// while the unmatched `)` that closes the Markdown/Verso link ends it.
fn extract_urls(text: &str) -> Vec<String> {
    let mut urls = Vec::new();
    let mut search_from = 0;
    while search_from < text.len() {
        let tail = &text[search_from..];
        let https = tail.find("https://");
        let http = tail.find("http://");
        let Some(relative_start) = https.into_iter().chain(http).min() else {
            break;
        };
        let start = search_from + relative_start;
        let candidate = &text[start..];
        let mut depth = 0usize;
        let mut end = candidate.len();
        for (offset, ch) in candidate.char_indices() {
            match ch {
                c if c.is_whitespace() => {
                    end = offset;
                    break;
                }
                '<' | '>' | '"' | '}' | ']' => {
                    end = offset;
                    break;
                }
                '(' => depth += 1,
                ')' if depth == 0 => {
                    end = offset;
                    break;
                }
                ')' => depth -= 1,
                _ => {}
            }
        }
        let url = candidate[..end].trim_end_matches(['.', ',', ';']);
        if !url.is_empty() {
            urls.push(url.to_string());
        }
        search_from = start + end.max("http://".len());
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
    async fn markdown_link_can_take_a_korean_particle() {
        let ctx = make_context(
            "See [Lean FRO](https://lean-fro.org).",
            "[Lean FRO](https://lean-fro.org)에서 확인하십시오.",
        );
        let result = LinkEvaluator.evaluate(&ctx).await.unwrap();
        assert!(result.passed, "{:?}", result.issues);
    }

    #[test]
    fn a_balanced_parenthesis_can_belong_to_the_url() {
        assert_eq!(
            extract_urls("[article](https://example.com/wiki/Foo_(bar))에서"),
            vec!["https://example.com/wiki/Foo_(bar)"]
        );
    }

    #[test]
    fn latex_command_closer_is_not_part_of_the_url() {
        assert_eq!(
            extract_urls("See \\url{https://example.com/path} for details."),
            vec!["https://example.com/path"]
        );
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

    fn rst_context(source: &str, translation: &str) -> EvaluationContext {
        EvaluationContext {
            source: source.to_string(),
            translation: translation.to_string(),
            glossary: HashMap::new(),
            source_lang: "en".to_string(),
            target_lang: "ko".to_string(),
            markup: Markup::Rst,
        }
    }

    /// The case this was written for: `Development bug/feature tracker`_ came
    /// back as `개발 버그/기능 트래커`_ and no longer named its target.
    #[tokio::test]
    async fn rst_fails_when_a_named_reference_is_translated() {
        let ctx = rst_context(
            "`Development bug/feature tracker`_",
            "`개발 버그/기능 트래커`_",
        );
        let result = LinkEvaluator.evaluate(&ctx).await.unwrap();
        assert!(!result.passed);
        assert!(
            result.issues[0]
                .message
                .contains("Development bug/feature tracker"),
            "{}",
            result.issues[0].message
        );
    }

    #[tokio::test]
    async fn rst_passes_when_the_reference_is_kept_or_aliased() {
        for translation in [
            "`Development bug/feature tracker`_\\ 를 보세요",
            "`개발 버그/기능 트래커 <Development bug/feature tracker_>`_\\ 를 보세요",
        ] {
            let ctx = rst_context("`Development bug/feature tracker`_", translation);
            let result = LinkEvaluator.evaluate(&ctx).await.unwrap();
            assert!(result.passed, "{translation:?}: {:?}", result.issues);
        }
    }

    /// A bare word reference keeps its underscore; the word alone appears in
    /// any translation of the sentence and proves nothing.
    #[tokio::test]
    async fn rst_fails_when_a_bare_reference_loses_its_underscore() {
        let ctx = rst_context(
            "an implementation of Python_ produced with it",
            "그것으로 만들어진 Python 구현입니다",
        );
        let result = LinkEvaluator.evaluate(&ctx).await.unwrap();
        assert!(!result.passed);

        let kept = rst_context(
            "an implementation of Python_ produced with it",
            "그것으로 만들어진 Python_\\ 의 구현입니다",
        );
        assert!(LinkEvaluator.evaluate(&kept).await.unwrap().passed);
    }

    #[test]
    fn named_references_are_extracted_precisely() {
        let names: Vec<String> = rst_named_references(
            "See `the docs`_, the ``literal_`` span, an anonymous `one`__, \
             an embedded `text <https://x.example>`_, and Python_.",
        )
        .into_iter()
        .map(|r| r.shown)
        .collect();
        assert_eq!(names, vec!["`the docs`_", "Python_"]);
    }

    /// Interpreted-text roles end without an underscore and are not references.
    #[test]
    fn roles_are_not_named_references() {
        assert!(rst_named_references(":ref:`contact` and :doc:`intro`").is_empty());
    }

    /// The extradoc case: the bare here__ was absorbed into Korean prose, one
    /// anonymous reference short of the file's targets, and every anonymous
    /// link in the file went dark.
    #[tokio::test]
    async fn rst_fails_when_an_anonymous_reference_is_absorbed() {
        let ctx = rst_context(
            "The complete list is here__ (in alphabetical order).",
            "전체 목록은 여기에서 확인할 수 있습니다 (알파벳 순).",
        );
        let result = LinkEvaluator.evaluate(&ctx).await.unwrap();
        assert!(!result.passed);
        assert!(
            result.issues[0].message.contains("Anonymous"),
            "{}",
            result.issues[0].message
        );
    }

    /// The display text of an anonymous reference is free — only the count
    /// has to hold.
    #[tokio::test]
    async fn rst_passes_when_the_anonymous_reference_is_translated() {
        let ctx = rst_context(
            "The complete list is here__ (in alphabetical order).",
            "전체 목록은 `여기`__\\ 에서 확인할 수 있습니다 (알파벳 순).",
        );
        let result = LinkEvaluator.evaluate(&ctx).await.unwrap();
        assert!(result.passed, "{:?}", result.issues);
    }

    #[test]
    fn anonymous_references_are_counted_precisely() {
        assert_eq!(rst_anonymous_references("see `the docs`__ and here__"), 2);
        assert_eq!(
            rst_anonymous_references("calls ``cont.__init__()`` twice"),
            0
        );
        assert_eq!(
            rst_anonymous_references("a named `ref`_ is not anonymous"),
            0
        );
        assert_eq!(rst_anonymous_references("`여기`__\\ 에서"), 1);
    }
}
