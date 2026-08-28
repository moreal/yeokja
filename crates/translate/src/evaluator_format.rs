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
        // Markdown and Verso use both `_text_` and `*text*` for emphasis. `_`
        // cannot close before a Korean particle, so the prompt deliberately
        // asks the translator to switch that pair to `*`. Count the two spellings
        // together for those markups; treating the instructed rewrite as a lost
        // marker makes every retry repeat an impossible demand.
        // Backticks and asterisks are ordinary punctuation in LaTeX (not
        // Markdown-style inline markup), so applying these checks there rejects
        // valid TeX quotations such as ``term'' and mathematical `*` tokens.
        if context.markup != Markup::Latex {
            let source_code_runs = mark_runs(&context.source, '`')
                - if context.markup == Markup::Rst {
                    rst_reference_runs(&context.source)
                } else {
                    0
                };
            let translation_code_runs = mark_runs(&context.translation, '`')
                - if context.markup == Markup::Rst {
                    rst_reference_runs(&context.translation)
                } else {
                    0
                };
            let checks = [
                (
                    "Inline code markers (`)",
                    source_code_runs,
                    translation_code_runs,
                ),
                (
                    "Emphasis marker runs",
                    emphasis_runs(&context.source, context.markup),
                    emphasis_runs(&context.translation, context.markup),
                ),
            ];
            for (name, in_source, in_translation) in checks {
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
        }

        // Closing somewhere is not closing where the source closed. Asciidoctor
        // does not give up on a pair whose closing mark a letter follows — it
        // keeps looking and closes on a later mark instead, so `_it_는 and
        // _other_ 사이` renders as one emphasis over `it_는 and _other`. The
        // prose in the middle is swallowed and the pairs after it are eaten, yet
        // nothing is left unclosed and the run count still matches. What gives it
        // away is how many pairs actually form.
        for &(mark, unconstrained) in constrained_marks(context.markup) {
            let source = pair_up(&chars(&context.source), mark);
            // A source that leaves a pair open has no count worth matching, and
            // demanding the translation match it would reject the rewrite that
            // fixes it. `chapters/type_system.asciidoc` writes `` `{...}`` ``.
            if !source.unclosable.is_empty() {
                continue;
            }
            let (in_source, in_translation) =
                if mark == '_' && matches!(context.markup, Markup::Markdown | Markup::Verso) {
                    (
                        markdown_emphasis_pairs(&context.source),
                        markdown_emphasis_pairs(&context.translation),
                    )
                } else {
                    (
                        source.formed,
                        pair_up(&chars(&context.translation), mark).formed,
                    )
                };
            if in_source != in_translation {
                issues.push(EvaluationIssue {
                    severity: IssueSeverity::Error,
                    kind: IssueKind::FormatLost,
                    message: format!(
                        "Inline pairs of {mark} do not line up: the source forms {in_source}, \
                         the translation {in_translation}. A pair whose closing mark a letter \
                         follows closes on a later mark instead, swallowing the text between \
                         and absorbing the pairs after it. Write each marked-up term a suffix \
                         follows as {unconstrained}, doubling the mark at BOTH ends."
                    ),
                });
            }
        }

        // AsciiDoc's curved quotes are constrained the same way, and there is no
        // doubled form to escape to: `"`term`"를` prints its marks as themselves.
        if context.markup == Markup::Asciidoc {
            let quotes = unclosable_quotes(&context.translation);
            if !quotes.is_empty() && unclosable_quotes(&context.source).is_empty() {
                issues.push(EvaluationIssue {
                    severity: IssueSeverity::Error,
                    kind: IssueKind::FormatLost,
                    message: format!(
                        "{} never closes: a curved-quote pair cannot close against a letter \
                         either, and it has no doubled form. Write the quotation marks \
                         themselves — “term” — or put a space or punctuation after the \
                         closing mark.",
                        quotes.join(", "),
                    ),
                });
            }
        }

        // reStructuredText recognizes an opening marker only when no word
        // character precedes it and a closing one only when none follows, and
        // there is no doubled form to escape to — the way out is a
        // backslash-escaped space, which renders as nothing.
        if context.markup == Markup::Rst {
            let malformed_roles = rst_malformed_role_closures(&context.translation);
            if !malformed_roles.is_empty() {
                issues.push(EvaluationIssue {
                    severity: IssueSeverity::Error,
                    kind: IssueKind::FormatLost,
                    message: format!(
                        "{} has an extra backtick after an RST role. Preserve the role as, for example, :pep:`649`.",
                        malformed_roles.join(", "),
                    ),
                });
            }

            let broken = rst_broken_pairs(&context.translation);
            if !broken.is_empty() && rst_broken_pairs(&context.source).is_empty() {
                let shown: Vec<&str> = broken.iter().map(String::as_str).collect();
                issues.push(EvaluationIssue {
                    severity: IssueSeverity::Error,
                    kind: IssueKind::FormatLost,
                    message: format!(
                        "{} is not recognized as markup: reStructuredText requires \
                         whitespace or punctuation on the outside of each marker, and \
                         doubling the marks does not help. Separate the word from the \
                         marker with a backslash-escaped space, which renders as \
                         nothing: ``heap``\\ 에, **bold**\\ 를, 실행\\ **될**.",
                        shown.join(", "),
                    ),
                });
            }

            let broken = rst_broken_bracket_references(&context.translation);
            if !broken.is_empty()
                && rst_broken_bracket_references(&context.source).is_empty()
            {
                issues.push(EvaluationIssue {
                    severity: IssueSeverity::Error,
                    kind: IssueKind::FormatLost,
                    message: format!(
                        "{} is not recognized as a footnote or citation reference: \
                         reStructuredText requires whitespace or punctuation after the \
                         trailing underscore. Separate a Korean particle with a \
                         backslash-escaped space, which renders as nothing: [2]_\\ 에.",
                        broken.join(", "),
                    ),
                });
            }
        }

        // Verso roles carry semantic identifiers in their headers, and code
        // roles use their payload to locate checked examples. They can remain
        // superficially balanced after a translator changes or drops those
        // values, so compare the structural tokens themselves rather than only
        // counting braces and backticks. Visible `[labels]` remain free to be
        // translated; only their bracketed shape is recorded.
        if context.markup == Markup::Verso {
            let source = verso_structure(&context.source);
            let translation = verso_structure(&context.translation);
            if source != translation {
                issues.push(EvaluationIssue {
                    severity: IssueSeverity::Error,
                    kind: IssueKind::FormatLost,
                    message: format!(
                        "Verso role structure changed: preserve every `{{role arguments}}` \
                         header and every backticked code/math payload exactly. Visible \
                         text inside `[labels]` may be translated. Source tokens: {source:?}; \
                         translation tokens: {translation:?}"
                    ),
                });
            }
        }

        if context.markup == Markup::Latex {
            let mut source = latex_structure(&context.source);
            let mut translation = latex_structure(&context.translation);
            // Korean grammar routinely moves a displayed term or reference to
            // another part of the sentence. The safety property is that every
            // structural token survives byte-for-byte, including duplicates;
            // their prose-level order is not itself LaTeX syntax.
            source.sort_unstable();
            translation.sort_unstable();
            if !is_multiset_subset(&source, &translation) {
                issues.push(EvaluationIssue {
                    severity: IssueSeverity::Error,
                    kind: IssueKind::FormatLost,
                    message: format!(
                        "LaTeX structure changed: preserve every source command, brace, bracket, \
                         comment placeholder, and mathematical expression (natural sentence \
                         reordering, translated prose inside \\text{{...}}, and moved terminal \
                         punctuation are allowed). Source tokens: \
                         {source:?}; translation tokens: {translation:?}"
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
                    "{} never closes: the marks print as themselves and the run swallows \
                     the text after them. Write {} as {}, doubling the mark at BOTH ends \
                     — doubling the closing one alone closes neither way — or put a space \
                     or punctuation after the closing mark.",
                    shown.join(", "),
                    if shown.len() == 1 { "it" } else { "each" },
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
///
/// Curved quotes borrow the backtick — `"`term`"` is a quotation, not code — so
/// those marks are not counted. The only way to write a quotation whose closing
/// mark a Korean particle follows is to give up the construct and write “term”
/// outright, and counting the borrowed marks would read that fix as two code
/// markers dropped.
fn mark_runs(text: &str, mark: char) -> usize {
    let chars = chars(text);
    let borrowed: std::collections::HashSet<usize> = if mark == '`' {
        curved_quotes(&chars)
            .into_iter()
            .flat_map(|(open, close)| [open, close])
            .collect()
    } else {
        std::collections::HashSet::new()
    };
    let counts = |at: usize| chars[at] == mark && !borrowed.contains(&at);

    let mut runs = 0;
    let mut i = 0;
    while i < chars.len() {
        if !counts(i) {
            i += 1;
            continue;
        }
        runs += 1;
        while i < chars.len() && counts(i) {
            i += 1;
        }
    }
    runs
}

/// Emphasis marker runs whose spelling may change without changing meaning.
fn emphasis_runs(text: &str, markup: Markup) -> usize {
    let stars = mark_runs(text, '*');
    if matches!(markup, Markup::Markdown | Markup::Verso) {
        stars + mark_runs(text, '_')
    } else {
        stars
    }
}

/// Emphasis pairs that actually form under Markdown/Verso flanking rules.
///
/// `_` needs the constrained pairing simulation, while `*` is allowed directly
/// before a Korean word character and each two marker runs form one pair.
fn markdown_emphasis_pairs(text: &str) -> usize {
    pair_up(&chars(text), '_').formed + mark_runs(text, '*') / 2
}

/// What a mark does across a stretch of text: how many pairs it actually forms,
/// and every pair it opens without being able to close.
///
/// The two answer different questions. A swallowed pair is closed — just not
/// where the source closed it — so only `formed` tells it apart from prose.
#[derive(Default)]
struct Pairing {
    formed: usize,
    unclosable: Vec<std::ops::Range<usize>>,
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
/// Which marks that covers is `constrained_marks`.
fn unclosable_pairs(text: &str, markup: Markup) -> Vec<Unclosable> {
    let chars = chars(text);
    let mut found = Vec::new();
    for &(mark, unconstrained) in constrained_marks(markup) {
        for span in pair_up(&chars, mark).unclosable {
            found.push(Unclosable {
                text: chars[span].iter().collect(),
                unconstrained,
            });
        }
    }
    found
}

fn chars(text: &str) -> Vec<char> {
    text.chars().collect()
}

/// The marks that open a constrained pair, each with the unconstrained form to
/// rewrite it as.
///
/// Markdown shares the rule only for `_`, where CommonMark rules out intraword
/// emphasis. Its code spans and `*` emphasis close against a word just fine, so
/// checking them there would fail translations that are correct. And
/// `__italic__` is bold in Markdown, so the way out is the other mark.
fn constrained_marks(markup: Markup) -> &'static [(char, &'static str)] {
    match markup {
        Markup::Asciidoc => &[('`', "``code``"), ('*', "**bold**"), ('_', "__italic__")],
        Markup::Markdown | Markup::Verso => &[('_', "*italic*")],
        // reStructuredText pairs are checked by `rst_broken_pairs`: every one
        // of its marker forms is constrained, so the doubled-form advice these
        // entries carry would be wrong there.
        Markup::Rst | Markup::Latex => &[],
    }
}

/// LaTeX syntax whose spelling is independent of the visible prose around it.
///
/// The evaluator compares the resulting tokens as a multiset: Korean sentence
/// order may move complete commands and math spans, while changing or dropping
/// even one token (including one of two duplicates) is still rejected.
fn latex_structure(text: &str) -> Vec<String> {
    let masked = mask_latex_visible_text_arguments(text);
    let text = masked.as_str();
    let mut tokens = Vec::new();
    let bytes = text.as_bytes();
    let mut at = 0usize;
    while at < bytes.len() {
        if text[at..].starts_with('⟦')
            && let Some(close) = text[at..].find('⟧')
        {
            let end = at + close + '⟧'.len_utf8();
            tokens.push(format!("comment:{}", &text[at..end]));
            at = end;
            continue;
        }

        if bytes[at] == b'$' && !is_escaped(bytes, at) {
            let delimiter = if bytes.get(at + 1) == Some(&b'$') {
                "$$"
            } else {
                "$"
            };
            let start = at;
            at += delimiter.len();
            while at < bytes.len() {
                if text[at..].starts_with(delimiter) && !is_escaped(bytes, at) {
                    at += delimiter.len();
                    break;
                }
                at += text[at..].chars().next().map_or(1, char::len_utf8);
            }
            tokens.push(format!("math:{}", normalize_math_token(&text[start..at])));
            continue;
        }

        if text[at..].starts_with("\\(") || text[at..].starts_with("\\[") {
            let (close, width) = if text[at..].starts_with("\\(") {
                ("\\)", 2)
            } else {
                ("\\]", 2)
            };
            let start = at;
            at += width;
            if let Some(offset) = text[at..].find(close) {
                at += offset + close.len();
            } else {
                at = text.len();
            }
            tokens.push(format!("math:{}", normalize_math_token(&text[start..at])));
            continue;
        }

        if bytes[at] == b'\\' {
            let start = at;
            at += 1;
            if at < bytes.len() && (bytes[at].is_ascii_alphabetic() || bytes[at] == b'@') {
                while at < bytes.len() && (bytes[at].is_ascii_alphabetic() || bytes[at] == b'@') {
                    at += 1;
                }
            } else if at < bytes.len() {
                at += text[at..].chars().next().map_or(1, char::len_utf8);
            }
            let command = &text[start + 1..at];
            // A backslash followed by a space is a typographic interword-space
            // hint (most often after i.e.), not semantic document structure.
            // Korean normally drops it with the preceding Latin abbreviation.
            if command.chars().all(char::is_whitespace) {
                continue;
            }
            // TeX accent commands and dotless i/j spell visible Latin text.
            // Transliteration into Hangul legitimately removes both the accent
            // command and its local braces.
            if matches!(command, "i" | "j") {
                continue;
            }
            if latex_accent_command(command) {
                if let Some(end) = latex_argument_end(text, at, b'{', b'}') {
                    at = end;
                } else if at < bytes.len() {
                    if bytes[at] == b'\\' {
                        at += 1;
                        while at < bytes.len()
                            && (bytes[at].is_ascii_alphabetic() || bytes[at] == b'@')
                        {
                            at += 1;
                        }
                    } else {
                        at += text[at..].chars().next().map_or(1, char::len_utf8);
                    }
                }
                continue;
            }
            tokens.push(format!("command:{}", &text[start..at]));
            if latex_opaque_argument_command(command) {
                while let Some(end) = latex_argument_end(text, at, b'[', b']') {
                    tokens.push(format!("opaque:{}", &text[at..end]));
                    at = end;
                }
                if let Some(end) = latex_argument_end(text, at, b'{', b'}') {
                    tokens.push(format!("opaque:{}", &text[at..end]));
                    at = end;
                }
            } else if command == "href"
                && let Some(end) = latex_argument_end(text, at, b'{', b'}')
            {
                // The destination is semantic; the following visible label is prose.
                tokens.push(format!("opaque:{}", &text[at..end]));
                at = end;
            }
            continue;
        }

        let ch = text[at..].chars().next().unwrap();
        if matches!(ch, '{' | '}' | '[' | ']' | '&' | '#' | '~') {
            tokens.push(format!("syntax:{ch}"));
        }
        at += ch.len_utf8();
    }
    tokens
}

fn mask_latex_visible_text_arguments(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut output = String::with_capacity(text.len());
    let mut at = 0usize;
    while at < bytes.len() {
        if text[at..].starts_with("\\text") {
            let command_end = at + "\\text".len();
            let mut open = command_end;
            while bytes.get(open).is_some_and(u8::is_ascii_whitespace) {
                open += 1;
            }
            if bytes.get(open) == Some(&b'{')
                && let Some(end) = latex_argument_end(text, command_end, b'{', b'}')
            {
                output.push_str(&text[at..=open]);
                output.push_str("VISIBLE_PROSE");
                output.push('}');
                at = end;
                continue;
            }
        }
        let ch = text[at..].chars().next().unwrap();
        output.push(ch);
        at += ch.len_utf8();
    }
    output
}

fn normalize_math_token(token: &str) -> String {
    let (open, close) = if token.starts_with("$$") && token.ends_with("$$") {
        ("$$", "$$")
    } else if token.starts_with('$') && token.ends_with('$') {
        ("$", "$")
    } else if token.starts_with("\\(") && token.ends_with("\\)") {
        ("\\(", "\\)")
    } else if token.starts_with("\\[") && token.ends_with("\\]") {
        ("\\[", "\\]")
    } else {
        return token.to_string();
    };
    let inner = token[open.len()..token.len() - close.len()].trim_end();
    let inner = inner
        .strip_suffix(['.', ',', ';', ':', '?', '!'])
        .unwrap_or(inner)
        .trim_end();
    format!("{open}{inner}{close}")
}

fn latex_accent_command(command: &str) -> bool {
    matches!(
        command,
        "\"" | "'" | "^" | "~" | "=" | "b" | "c" | "d" | "H" | "k" | "r" | "t" | "u" | "v"
    )
}

fn is_multiset_subset(required: &[String], available: &[String]) -> bool {
    let mut counts = std::collections::HashMap::<&str, usize>::new();
    for token in available {
        *counts.entry(token).or_default() += 1;
    }
    required.iter().all(|token| {
        let Some(count) = counts.get_mut(token.as_str()) else {
            return false;
        };
        if *count == 0 {
            return false;
        }
        *count -= 1;
        true
    })
}

fn is_escaped(bytes: &[u8], at: usize) -> bool {
    let mut slashes = 0usize;
    let mut cursor = at;
    while cursor > 0 && bytes[cursor - 1] == b'\\' {
        slashes += 1;
        cursor -= 1;
    }
    slashes % 2 == 1
}

fn latex_opaque_argument_command(command: &str) -> bool {
    matches!(
        command,
        "Cref"
            | "cref"
            | "cite"
            | "citeauthor"
            | "citep"
            | "citet"
            | "eqref"
            | "include"
            | "includegraphics"
            | "input"
            | "label"
            | "pageref"
            | "ref"
            | "url"
    )
}

fn latex_argument_end(text: &str, mut at: usize, open: u8, close: u8) -> Option<usize> {
    let bytes = text.as_bytes();
    while bytes.get(at).is_some_and(u8::is_ascii_whitespace) {
        at += 1;
    }
    if bytes.get(at) != Some(&open) {
        return None;
    }
    let mut depth = 0usize;
    let mut cursor = at;
    while cursor < bytes.len() {
        if bytes[cursor] == open && !is_escaped(bytes, cursor) {
            depth += 1;
        } else if bytes[cursor] == close && !is_escaped(bytes, cursor) {
            depth -= 1;
            if depth == 0 {
                return Some(cursor + 1);
            }
        }
        cursor += 1;
    }
    None
}

/// Sorted structural tokens from Verso inline roles and math.
///
/// Sorting deliberately permits a natural sentence reordering. The tokens are
/// still a multiset, so dropping either of two identical references is caught.
fn verso_structure(text: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut at = 0;
    while at < text.len() {
        if text[at..].starts_with("$$`") || text[at..].starts_with("$`") {
            let prefix_len = if text[at..].starts_with("$$`") { 3 } else { 2 };
            let payload_start = at + prefix_len;
            if let Some(close) = text[payload_start..].find('`') {
                let end = payload_start + close + 1;
                tokens.push(format!("math:{}", &text[at..end]));
                at = end;
                continue;
            }
        }

        if text[at..].starts_with('{') {
            let name_start = at + 1;
            let starts_as_role = text[name_start..]
                .chars()
                .next()
                .is_some_and(|c| c.is_ascii_alphabetic());
            if starts_as_role && let Some(close_offset) = text[name_start..].find('}') {
                let header_end = name_start + close_offset + 1;
                let header = &text[at..header_end];
                if text[header_end..].starts_with('`') {
                    let payload_start = header_end + 1;
                    if let Some(close) = text[payload_start..].find('`') {
                        let end = payload_start + close + 1;
                        tokens.push(format!("role-code:{header}{}", &text[header_end..end]));
                        at = end;
                        continue;
                    }
                }
                if text[header_end..].starts_with('[') {
                    tokens.push(format!("role-label:{header}"));
                } else {
                    tokens.push(format!("role:{header}"));
                }
                at = header_end;
                continue;
            }
        }

        let ch = text[at..].chars().next().unwrap();
        at += ch.len_utf8();
    }
    tokens.sort();
    tokens
}

#[derive(Debug)]
struct VersoCodeToken {
    range: std::ops::Range<usize>,
    header: String,
    payload: String,
    raw: String,
}

/// Restore non-breaking spaces in opaque Verso role payloads.
///
/// Models commonly turn an invisible U+00A0 into U+0020 or the literal text
/// `\u{a0}`. The payload is code, not prose. When a translated role has the
/// same header and otherwise-identical payload, put back the exact source token
/// before format evaluation and persistence. Tokens are matched as a multiset
/// so natural sentence reordering remains allowed.
pub(crate) fn restore_verso_code_whitespace(source: &str, translation: &str) -> String {
    let source_tokens = verso_code_tokens(source);
    let translation_tokens = verso_code_tokens(translation);
    let mut used = vec![false; translation_tokens.len()];
    let mut replacements = Vec::new();

    for source_token in source_tokens
        .iter()
        .filter(|token| token.payload.contains('\u{a0}'))
    {
        let normalized_source = normalize_verso_code_whitespace(&source_token.payload);
        let Some((index, translated_token)) =
            translation_tokens
                .iter()
                .enumerate()
                .find(|(index, token)| {
                    !used[*index]
                        && token.header == source_token.header
                        && normalize_verso_code_whitespace(&token.payload) == normalized_source
                })
        else {
            continue;
        };
        used[index] = true;
        replacements.push((translated_token.range.clone(), source_token.raw.clone()));
    }

    let mut restored = translation.to_string();
    replacements.sort_by_key(|(range, _)| std::cmp::Reverse(range.start));
    for (range, source_token) in replacements {
        restored.replace_range(range, &source_token);
    }
    restored
}

fn normalize_verso_code_whitespace(payload: &str) -> String {
    payload.replace("\\u{a0}", " ").replace('\u{a0}', " ")
}

fn verso_code_tokens(text: &str) -> Vec<VersoCodeToken> {
    let mut tokens = Vec::new();
    let mut at = 0;
    while at < text.len() {
        if text[at..].starts_with('{') {
            let name_start = at + 1;
            let starts_as_role = text[name_start..]
                .chars()
                .next()
                .is_some_and(|ch| ch.is_ascii_alphabetic());
            if starts_as_role && let Some(close_offset) = text[name_start..].find('}') {
                let header_end = name_start + close_offset + 1;
                if text[header_end..].starts_with('`') {
                    let payload_start = header_end + 1;
                    if let Some(close) = text[payload_start..].find('`') {
                        let payload_end = payload_start + close;
                        let end = payload_end + 1;
                        tokens.push(VersoCodeToken {
                            range: at..end,
                            header: text[at..header_end].to_string(),
                            payload: text[payload_start..payload_end].to_string(),
                            raw: text[at..end].to_string(),
                        });
                        at = end;
                        continue;
                    }
                }
            }
        }
        let ch = text[at..].chars().next().unwrap();
        at += ch.len_utf8();
    }
    tokens
}

/// Every AsciiDoc curved-quote pair `text` opens but cannot close.
///
/// `"`term`"` is constrained like the rest, so a Korean particle against its
/// closing mark holds it open. Unlike `` `code` `` it has no doubled form to
/// escape to — the way out is to write “term” with the quotation marks
/// themselves, which is what the pair would have rendered as.
fn unclosable_quotes(text: &str) -> Vec<String> {
    let chars = chars(text);
    curved_quotes(&chars)
        .into_iter()
        .filter(|&(_, close)| chars.get(close + 2).is_some_and(|c| is_word(*c)))
        .map(|(open, close)| chars[open - 1..close + 2].iter().collect())
        .collect()
}

/// Every AsciiDoc curved-quote pair in `chars`, as the indices of the two
/// backticks it borrows.
///
/// The pair opens on `` "` `` and closes on the mirrored `` `" ``, and only
/// there — `'` behaves the same way for single quotes.
fn curved_quotes(chars: &[char]) -> Vec<(usize, usize)> {
    let mut found = Vec::new();
    let mut i = 0;
    while i + 1 < chars.len() {
        let quote = chars[i];
        if (quote != '"' && quote != '\'') || chars[i + 1] != '`' {
            i += 1;
            continue;
        }
        let mut at = i + 2;
        let mut close = None;
        while at + 1 < chars.len() {
            if chars[at] == '`' && chars[at + 1] == quote {
                close = Some(at);
                break;
            }
            at += 1;
        }
        match close {
            Some(close) => {
                found.push((i + 1, close));
                i = close + 2;
            }
            None => i += 1,
        }
    }
    found
}

/// Asciidoctor's `\p{Word}`, which every Hangul syllable satisfies.
fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// Pair up runs of `mark` the way AsciiDoc reads them, and report each pair
/// that opens without being able to close — as the span of the pair plus the
/// character holding it open.
///
/// A run of two marks is the unconstrained form and pairs with another run of
/// two, anywhere. A lone mark is the constrained form and pairs with another
/// lone mark, but only when nothing sits against either end: not a word
/// character, not a quote, not another mark. So half-doubling a pair —
/// `` `Atom``이라는 `` — closes neither way, and that is exactly what a
/// translator reaches for first on being told to double the marks.
fn pair_up(chars: &[char], mark: char) -> Pairing {
    // Asciidoctor's own flanking test, which is why `'` is in here: it turns
    // `` `Atom`'s `` into a curly quote and no code span at all.
    let blocked = |c: char| c.is_alphanumeric() || c == '_' || c == '"' || c == '\'' || c == mark;
    let run_len = |at: usize| chars[at..].iter().take_while(|c| **c == mark).count();

    let mut pairing = Pairing::default();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] != mark {
            i += 1;
            continue;
        }
        let open_len = run_len(i);
        let after_open = i + open_len;
        // A lone mark with a word against its left opens nothing: the
        // underscores in `min_heap_size` are text, not markup.
        let opens = open_len > 1
            || ((i == 0 || !blocked(chars[i - 1]))
                && chars.get(after_open).is_some_and(|c| !c.is_whitespace()));
        if !opens {
            i = after_open;
            continue;
        }
        // Look for a run that can close this one. AsciiDoc keeps looking past
        // a candidate it cannot use, which is why `_temp_alloc_ is` closes on
        // the trailing underscore rather than giving up on the one inside the
        // identifier. Failing to look past it called the English source broken
        // too, and a check that rejects the source rejects nothing.
        let mut from = after_open;
        let mut first_candidate = None;
        let closes_at = loop {
            let Some(offset) = chars[from..].iter().position(|c| *c == mark) else {
                break None;
            };
            let close = from + offset;
            let close_len = run_len(close);
            first_candidate.get_or_insert(close + close_len);
            let usable = close_len == open_len
                && (open_len > 1
                    || (!chars[close - 1].is_whitespace()
                        && !chars.get(close + 1).is_some_and(|c| blocked(*c))));
            if usable {
                break Some(close + close_len);
            }
            from = close + close_len;
        };
        match (closes_at, first_candidate) {
            (Some(end), _) => {
                pairing.formed += 1;
                i = end;
            }
            // A mark with nothing after it to pair with is text, not markup.
            (None, None) => break,
            (None, Some(end)) => {
                pairing.unclosable.push(i..(end + 1).min(chars.len()));
                i = end;
            }
        }
    }
    pairing
}

/// Every reStructuredText inline pair `text` writes against a word character.
///
/// Docutils recognizes an opening marker only when whitespace or punctuation
/// precedes it, and a closing one only when whitespace or punctuation follows
/// it — and a Hangul syllable is a letter. The natural translation of "on the
/// ``heap``" ends `` ``heap``에 ``, which renders with both marker runs
/// printed as themselves. Unlike AsciiDoc there is no unconstrained form:
/// doubling is a different construct under the same rule. The way out is a
/// backslash-escaped space (`` ``heap``\ 에 ``), which renders as nothing.
///
/// Reference suffixes (`` `name`_ ``, `` `name`__ ``) belong to the closing
/// marker, so the rule applies after the underscores.
fn rst_broken_pairs(text: &str) -> Vec<String> {
    let chars = chars(text);
    let backtick_mask = rst_backtick_mask(&chars);
    let run_len = |at: usize, mark: char| chars[at..].iter().take_while(|c| **c == mark).count();
    let mut found = Vec::new();

    for mark in ['`', '*'] {
        // (start index, run length) of the currently open marker, if any.
        let mut open: Option<(usize, usize)> = None;
        let mut i = 0;
        while i < chars.len() {
            if chars[i] != mark || (mark == '*' && backtick_mask[i]) {
                i += 1;
                continue;
            }
            let len = run_len(i, mark);
            match open {
                None => {
                    // An opener needs text right after it; a run followed by
                    // whitespace is prose (`2 * 3`).
                    if !chars.get(i + len).is_some_and(|c| !c.is_whitespace()) {
                        i += len;
                        continue;
                    }
                    let blocked = i > 0 && is_word(chars[i - 1]);
                    if !blocked {
                        open = Some((i, len));
                    } else if chars[i + len..].contains(&mark) {
                        // A word glued to the front (`실행**될 수 있는**`)
                        // keeps the pair from opening — but only call it a
                        // pair when a partner run exists; `2*3` is arithmetic.
                        let end = (i + len + 1).min(chars.len());
                        found.push(chars[i.saturating_sub(1)..end].iter().collect());
                    }
                    i += len;
                }
                Some((oi, olen)) => {
                    // Only a run of the opener's own length closes it: `` and
                    // ` are different constructs in reStructuredText.
                    if len != olen || chars[i - 1].is_whitespace() {
                        i += len;
                        continue;
                    }
                    // `_` and `__` after a closing backtick are the reference
                    // suffix; the rule applies to the character after them.
                    let mut end = i + len;
                    if mark == '`' {
                        while chars.get(end).is_some_and(|c| *c == '_') {
                            end += 1;
                        }
                    }
                    // Docutils accepts whitespace or closing punctuation after
                    // an end-string; a word character or an opening bracket
                    // (`` ``x``(y) ``, dropped space and all) blocks it.
                    if chars
                        .get(end)
                        .is_some_and(|c| is_word(*c) || matches!(c, '(' | '[' | '{' | '<'))
                    {
                        found.push(chars[oi..end + 1].iter().collect());
                    }
                    open = None;
                    i += len;
                }
            }
        }
    }
    found
}

fn rst_backtick_mask(chars: &[char]) -> Vec<bool> {
    let mut mask = vec![false; chars.len()];
    let mut at = 0usize;
    while at < chars.len() {
        if chars[at] != '`' {
            at += 1;
            continue;
        }
        let run = chars[at..].iter().take_while(|ch| **ch == '`').count();
        let mut scan = at + run;
        let close = loop {
            let Some(offset) = chars[scan..].iter().position(|ch| *ch == '`') else {
                break None;
            };
            let candidate = scan + offset;
            let candidate_run = chars[candidate..]
                .iter()
                .take_while(|ch| **ch == '`')
                .count();
            if candidate_run == run {
                break Some(candidate + run);
            }
            scan = candidate + candidate_run;
        };
        let Some(end) = close else {
            at += run;
            continue;
        };
        mask[at..end].fill(true);
        at = end;
    }
    mask
}

/// Footnote and citation references whose trailing underscore touches a word.
///
/// A Korean particle naturally produces ``[2]_에서`` or ``[RFC]_는``. As with
/// other RST inline markup, the adjacent word character prevents docutils from
/// recognizing the reference. A backslash-escaped space keeps the source valid
/// without adding visible whitespace: ``[2]_\ 에서``.
fn rst_broken_bracket_references(text: &str) -> Vec<String> {
    let chars = chars(text);
    let mut found = Vec::new();
    let mut at = 0usize;

    while at < chars.len() {
        if chars[at] != '[' {
            at += 1;
            continue;
        }
        let Some(close_offset) = chars[at + 1..]
            .iter()
            .position(|ch| *ch == ']' || *ch == '\n')
        else {
            break;
        };
        let close = at + 1 + close_offset;
        if chars[close] == '\n' {
            at = close + 1;
            continue;
        }
        let underscore = close + 1;
        let after = underscore + 1;
        if chars.get(underscore) == Some(&'_') {
            if at > 0 && is_word(chars[at - 1]) {
                found.push(chars[at - 1..=underscore].iter().collect());
            }
            if chars
                .get(after)
                .is_some_and(|ch| is_word(*ch) || matches!(ch, '(' | '[' | '{' | '<'))
            {
                found.push(chars[at..=after].iter().collect());
            }
        }
        at = close + 1;
    }
    found
}

fn rst_malformed_role_closures(text: &str) -> Vec<String> {
    let chars = chars(text);
    let mut found = Vec::new();
    let mut at = 0usize;
    while at < chars.len() {
        if chars[at] != ':' {
            at += 1;
            continue;
        }
        let Some(open_offset) = chars[at + 1..]
            .iter()
            .position(|ch| *ch == '`' || ch.is_whitespace())
        else {
            break;
        };
        let open = at + 1 + open_offset;
        if chars[open] != '`' || chars.get(open.wrapping_sub(1)) != Some(&':') {
            at = open + 1;
            continue;
        }
        let Some(close_offset) = chars[open + 1..].iter().position(|ch| *ch == '`') else {
            break;
        };
        let close = open + 1 + close_offset;
        if chars.get(close + 1) == Some(&'`') {
            found.push(chars[at..=close + 1].iter().collect());
        }
        at = close + 1;
    }
    found
}

/// Insert invisible RST boundaries where Korean particles touch inline markup.
///
/// This is a deterministic typography repair, not a translation decision. It
/// runs before evaluation so a model does not spend retries rediscovering the
/// same ``\\ `` escape for every literal, role, emphasis span, footnote, and
/// citation. If the source segment itself looks structurally incomplete (for
/// example because a literal spans two parser segments), that class of repair
/// is skipped rather than guessing at a segment boundary.
pub(crate) fn repair_rst_boundaries(source: &str, translation: &str) -> String {
    let repaired_line_start;
    let translation = if line_start_construct(source).is_none()
        && matches!(
            line_start_construct(translation),
            Some("an attribute entry (`:name:`)") | Some("a block title (`.`)")
        )
    {
        let trimmed = translation.trim_start();
        let indent_len = translation.len() - trimmed.len();
        repaired_line_start = format!(
            "{}관련 {trimmed}",
            &translation[..indent_len]
        );
        repaired_line_start.as_str()
    } else {
        translation
    };

    let chars = chars(translation);
    let backtick_mask = rst_backtick_mask(&chars);
    let mut insertions = std::collections::BTreeSet::new();
    let mut spaces = std::collections::BTreeSet::new();

    for token in source.split_whitespace() {
        let url = token
            .trim_start_matches(['(', '[', '{', '<', '\'', '"'])
            .trim_end_matches(['.', ',', ';', ':', ')', ']', '}', '>', '\'', '"']);
        if !url.starts_with("http://") && !url.starts_with("https://") {
            continue;
        }
        for (byte_at, _) in translation.match_indices(url) {
            let byte_end = byte_at + url.len();
            if translation[byte_end..]
                .chars()
                .next()
                .is_some_and(is_word)
            {
                insertions.insert(translation[..byte_end].chars().count());
            }
        }
    }

    if rst_broken_bracket_references(source).is_empty() {
        let mut at = 0usize;
        while at < chars.len() {
            if chars[at] != '[' {
                at += 1;
                continue;
            }
            let Some(close_offset) = chars[at + 1..]
                .iter()
                .position(|ch| *ch == ']' || *ch == '\n')
            else {
                break;
            };
            let close = at + 1 + close_offset;
            if chars[close] == '\n' {
                at = close + 1;
                continue;
            }
            let underscore = close + 1;
            let after = underscore + 1;
            if chars.get(underscore) == Some(&'_') {
                if at > 0 && is_word(chars[at - 1]) {
                    spaces.insert(at);
                }
                if chars
                    .get(after)
                    .is_some_and(|ch| is_word(*ch) || matches!(ch, '(' | '[' | '{' | '<'))
                {
                    insertions.insert(after);
                }
            }
            at = close + 1;
        }
    }

    if rst_broken_pairs(source).is_empty() {
        let run_len = |at: usize, mark: char| chars[at..].iter().take_while(|c| **c == mark).count();
        for mark in ['`', '*'] {
            let mut open: Option<(usize, usize)> = None;
            let mut at = 0usize;
            while at < chars.len() {
                if chars[at] != mark || (mark == '*' && backtick_mask[at]) {
                    at += 1;
                    continue;
                }
                let len = run_len(at, mark);
                match open {
                    None => {
                        if !chars.get(at + len).is_some_and(|ch| !ch.is_whitespace()) {
                            at += len;
                            continue;
                        }
                        let blocked = at > 0 && is_word(chars[at - 1]);
                        if !blocked {
                            open = Some((at, len));
                        } else if chars[at + len..].contains(&mark) {
                            insertions.insert(at);
                            open = Some((at, len));
                        }
                        at += len;
                    }
                    Some((_, open_len)) => {
                        if len != open_len || chars[at - 1].is_whitespace() {
                            at += len;
                            continue;
                        }
                        let mut end = at + len;
                        if mark == '`' {
                            while chars.get(end).is_some_and(|ch| *ch == '_') {
                                end += 1;
                            }
                        }
                        if chars
                            .get(end)
                            .is_some_and(|ch| is_word(*ch) || matches!(ch, '(' | '[' | '{' | '<'))
                        {
                            insertions.insert(end);
                        }
                        open = None;
                        at += len;
                    }
                }
            }
        }
    }

    if insertions.is_empty() && spaces.is_empty() {
        return translation.to_string();
    }
    let mut repaired = String::with_capacity(
        translation.len() + insertions.len() * 2 + spaces.len(),
    );
    for boundary in 0..=chars.len() {
        if spaces.contains(&boundary) {
            repaired.push(' ');
        } else if insertions.contains(&boundary) {
            repaired.push_str("\\ ");
        }
        if let Some(ch) = chars.get(boundary) {
            repaired.push(*ch);
        }
    }
    repaired
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
    // `..` opens a comment, directive, or hyperlink target in
    // reStructuredText, and a nested ordered list in AsciiDoc; `...` is an
    // ellipsis and reads as prose.
    if text == ".." || text.starts_with(".. ") {
        return Some("an explicit-markup start (`..`)");
    }
    let mut chars = text.chars();
    let first = chars.next()?;
    let rest = chars.as_str();
    let spaced = rest.starts_with([' ', '\t']);
    match first {
        // `.Title` names the block below it; `...` is an ellipsis.
        '.' if !rest.is_empty() && !rest.starts_with(['.', ' ', '\t']) => {
            Some("a block title (`.`)")
        }
        '*' | '-' | '+' if spaced => Some("a list item"),
        '=' if spaced => Some("a section title (`=`)"),
        '#' if spaced => Some("a heading (`#`)"),
        '>' if spaced => Some("a block quote (`>`)"),
        '|' => Some("a table cell (`|`)"),
        '/' if rest.starts_with('/') => Some("a comment (`//`)"),
        '[' if text.ends_with(']') => Some("an attribute line (`[...]`)"),
        ':' => {
            let end = rest.find(':')?;
            (end > 0).then_some("an attribute entry (`:name:`)")
        }
        _ => None,
    }
}

/// Backtick runs used by an RST named reference are hyperlink syntax, not
/// inline code markers. This covers both `label`_ and a translated visible
/// label written as `번역 <label_>`_.
fn rst_reference_runs(text: &str) -> usize {
    let chars = chars(text);
    let mut runs = 0;
    let mut at = 0;
    while at < chars.len() {
        if chars[at] != '`'
            || chars.get(at.wrapping_sub(1)) == Some(&'`')
            || chars.get(at + 1) == Some(&'`')
        {
            at += 1;
            continue;
        }
        let mut scan = at + 1;
        let mut close = None;
        while scan < chars.len() {
            if chars[scan] != '`' {
                scan += 1;
                continue;
            }
            let run = chars[scan..].iter().take_while(|ch| **ch == '`').count();
            if run > 1 {
                scan += run;
                continue;
            }
            if chars.get(scan + 1) == Some(&'_') {
                close = Some(scan);
            }
            scan += 1;
            break;
        }
        if close.is_some() {
            runs += 2;
        }
        at = scan;
    }
    runs
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
        assert_eq!(
            line_start_construct("= Title"),
            Some("a section title (`=`)")
        );
        assert_eq!(line_start_construct("* item"), Some("a list item"));
        assert_eq!(
            line_start_construct("[source,erlang]"),
            Some("an attribute line (`[...]`)")
        );
        assert_eq!(
            line_start_construct(":toc: left"),
            Some("an attribute entry (`:name:`)")
        );
        assert_eq!(
            line_start_construct(":Contact person:"),
            Some("an attribute entry (`:name:`)")
        );
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

    #[tokio::test]
    async fn markdown_allows_underscore_emphasis_to_become_stars_before_a_particle() {
        let ctx = context_in(
            Markup::Markdown,
            "The _arity_ is the argument count.",
            "인자 개수는 *arity*입니다.",
        );
        let result = FormatEvaluator.evaluate(&ctx).await.unwrap();
        assert!(result.passed, "{:?}", result.issues);

    }

    #[tokio::test]
    async fn verso_allows_underscore_emphasis_to_become_stars_before_a_particle() {
        let ctx = context_in(
            Markup::Verso,
            "It serves as _data_ for the lookup.",
            "조회에 사용할 *데이터*로서 역할을 합니다.",
        );
        let result = FormatEvaluator.evaluate(&ctx).await.unwrap();
        assert!(result.passed, "{:?}", result.issues);

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

    /// Doubling only one end is the first thing a translator tries on being
    /// told to double the marks, and it renders as nothing either way.
    #[test]
    fn a_half_doubled_pair_closes_neither_way() {
        for text in [
            "청크 `Atom``이라는 이름을 씁니다.",
            "청크 ``Atom`이라는 이름을 씁니다.",
        ] {
            assert!(
                !unclosable_pairs(text, Markup::Asciidoc).is_empty(),
                "{text:?} closes neither as a constrained nor an unconstrained pair"
            );
        }
    }

    /// AsciiDoc looks past a closer it cannot use. `_temp_alloc_` closes on
    /// its trailing underscore, not on the one inside the identifier — so the
    /// English reads fine and only the Korean, which glues 은 to the end, does
    /// not. Calling both broken would have switched the check off for the pair.
    #[test]
    fn a_closer_is_sought_past_the_one_that_cannot_close() {
        assert!(
            unclosable_pairs("The allocator _temp_alloc_ is used.", Markup::Asciidoc).is_empty()
        );
        assert!(
            !unclosable_pairs(
                "할당자 _temp_alloc_은 임시 할당에 씁니다.",
                Markup::Asciidoc
            )
            .is_empty()
        );
    }

    /// Asciidoctor blocks a closing mark on a quote as well as on a letter.
    #[test]
    fn a_quote_against_the_closing_mark_blocks_it() {
        assert!(!unclosable_pairs("the `Atom`\"quoted\" chunk", Markup::Asciidoc).is_empty());
        assert!(unclosable_pairs("the `Atom`. chunk", Markup::Asciidoc).is_empty());
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

    /// Shipped in theBeamBook and read as correct by every earlier check: the
    /// first pair cannot close against 의, so Asciidoctor closes it on the mark
    /// that should have opened the second one. One emphasis covers
    /// `erl_process.c_의 _check_balance`, and both marks stay unclosed and
    /// nothing is missing, so neither the run count nor the unclosable check
    /// sees it.
    #[tokio::test]
    async fn fails_when_a_pair_closes_on_a_later_mark_and_swallows_the_prose() {
        let ctx = make_context(
            "This is done by the function _check_balance_ in _erl_process.c_.",
            "이는 _erl_process.c_의 _check_balance_ 함수에 의해 수행됩니다.",
        );
        let result = FormatEvaluator.evaluate(&ctx).await.unwrap();
        assert!(!result.passed);
        assert!(
            result
                .issues
                .iter()
                .any(|i| i.message.contains("do not line up")),
            "{:?}",
            result.issues
        );
    }

    /// The same thing happens to code spans, and costs more: this one shipped
    /// in theBeamBook as a single span reading `io:format`은 … `recon_trace`,
    /// with the prose between it set as code.
    #[tokio::test]
    async fn fails_when_a_code_span_closes_on_a_later_mark() {
        let ctx = make_context(
            "`io:format` offers a quick method, whereas `erl_tracer` and `recon_trace` \
             provide deeper insights.",
            "`io:format`은 빠른 방법을 제공하는 반면, `erl_tracer`와 `recon_trace` 같은 \
             도구는 더 깊은 통찰을 제공합니다.",
        );
        let result = FormatEvaluator.evaluate(&ctx).await.unwrap();
        assert!(!result.passed);
        assert!(
            result
                .issues
                .iter()
                .any(|i| i.message.contains("do not line up")),
            "{:?}",
            result.issues
        );
    }

    /// The same segment written the way it has to be written passes.
    #[tokio::test]
    async fn passes_when_both_ends_are_doubled() {
        let ctx = make_context(
            "This is done by the function _check_balance_ in _erl_process.c_.",
            "이는 __erl_process.c__의 __check_balance__ 함수에 의해 수행됩니다.",
        );
        let result = FormatEvaluator.evaluate(&ctx).await.unwrap();
        assert!(result.passed, "{:?}", result.issues);

    }

    /// A mark with a word against its left opens nothing, so this emphasis is
    /// never opened rather than never closed — the asterisks print as themselves.
    #[tokio::test]
    async fn fails_when_a_suffix_swallows_the_opening_mark() {
        let ctx = make_context(
            "two or more processes that *can* execute independently",
            "서로 독립적으로 실행*될 수 있는* 둘 이상의 프로세스",
        );
        let result = FormatEvaluator.evaluate(&ctx).await.unwrap();
        assert!(!result.passed, "{:?}", result.issues);
    }

    /// Curved quotes are constrained too, and have no doubled form to escape to.
    #[tokio::test]
    async fn fails_when_a_curved_quote_cannot_close() {
        let ctx = make_context(
            "BEAM uses the GCC extension \"`labels as values`\".",
            "BEAM은 GCC 확장 기능인 \"`labels as values`\"를 사용합니다.",
        );
        let result = FormatEvaluator.evaluate(&ctx).await.unwrap();
        assert!(!result.passed);
        assert!(
            result
                .issues
                .iter()
                .any(|i| i.message.contains("curved-quote")),
            "{:?}",
            result.issues
        );
    }

    /// Writing the quotation marks outright is the only way out, so it must not
    /// read as two code markers dropped.
    #[tokio::test]
    async fn passes_when_a_curved_quote_becomes_the_quotation_marks() {
        let ctx = make_context(
            "BEAM uses the GCC extension \"`labels as values`\".",
            "BEAM은 GCC 확장 기능인 “labels as values”를 사용합니다.",
        );
        let result = FormatEvaluator.evaluate(&ctx).await.unwrap();
        assert!(result.passed, "{:?}", result.issues);
    }

    /// A quotation the source itself leaves open is the source's business.
    #[tokio::test]
    async fn passes_when_the_source_quote_cannot_close_either() {
        let ctx = make_context("the \"`term`\"s here", "여기 \"`term`\"의");
        let result = FormatEvaluator.evaluate(&ctx).await.unwrap();
        assert!(result.passed, "{:?}", result.issues);
    }

    /// The RST counterpart of the particle problem: docutils wants whitespace
    /// or punctuation after a closing marker, and a Hangul particle is neither.
    #[tokio::test]
    async fn rst_fails_when_a_particle_follows_a_closing_marker() {
        let ctx = context_in(
            Markup::Rst,
            "Objects are allocated on the ``heap``.",
            "객체는 ``heap``에 할당됩니다.",
        );
        let result = FormatEvaluator.evaluate(&ctx).await.unwrap();
        assert!(!result.passed);
        assert!(
            result
                .issues
                .iter()
                .any(|i| i.message.contains("backslash-escaped")),
            "{:?}",
            result.issues
        );
    }

    /// The documented escape — a backslash-escaped space — must pass, and so
    /// must the marker counts it leaves behind.
    #[tokio::test]
    async fn rst_passes_with_a_backslash_escaped_space() {
        let ctx = context_in(
            Markup::Rst,
            "Objects are allocated on the ``heap``.",
            "객체는 ``heap``\\ 에 할당됩니다.",
        );
        let result = FormatEvaluator.evaluate(&ctx).await.unwrap();
        assert!(result.passed, "{:?}", result.issues);
    }

    /// Doubling is not an escape in RST — `` and ` are different constructs
    /// under the same recognition rule.
    #[tokio::test]
    async fn rst_doubled_markers_do_not_escape() {
        let ctx = context_in(
            Markup::Rst,
            "two or more processes that **can** execute independently",
            "서로 독립적으로 실행**될 수 있는** 둘 이상의 프로세스",
        );
        let result = FormatEvaluator.evaluate(&ctx).await.unwrap();
        assert!(!result.passed, "{:?}", result.issues);
    }

    /// `` `name`_ `` carries its reference suffix outside the backticks; the
    /// recognition rule applies after the underscores.
    #[test]
    fn rst_reference_suffix_is_part_of_the_marker() {
        assert!(rst_broken_pairs("consult the `PyPy website`_ for details").is_empty());
        assert_eq!(
            rst_broken_pairs("`PyPy website`_를 참고하십시오"),
            vec!["`PyPy website`_를"]
        );
        assert!(rst_broken_pairs("`PyPy website`_\\ 를 참고하십시오").is_empty());
    }

    #[tokio::test]
    async fn rst_footnote_reference_requires_a_boundary_before_a_particle() {
        let ctx = context_in(
            Markup::Rst,
            "Follow the procedure [2]_ carefully.",
            "절차 [2]_에서 설명한 대로 주의해서 진행합니다.",
        );
        let result = FormatEvaluator.evaluate(&ctx).await.unwrap();
        assert!(!result.passed, "{:?}", result.issues);
        assert!(
            result
                .issues
                .iter()
                .any(|issue| issue.message.contains("[2]_에")),
            "{:?}",
            result.issues
        );
    }

    #[tokio::test]
    async fn rst_footnote_reference_requires_a_boundary_after_the_previous_word() {
        let ctx = context_in(
            Markup::Rst,
            "Discussed by Hye-Shik Chang [1]_.",
            "Hye-Shik Chang[1]_\\ 에 의해 논의되었습니다.",
        );
        let result = FormatEvaluator.evaluate(&ctx).await.unwrap();
        assert!(!result.passed, "{:?}", result.issues);
    }

    #[tokio::test]
    async fn rst_footnote_reference_accepts_a_backslash_escaped_space() {
        let ctx = context_in(
            Markup::Rst,
            "Follow the procedure [2]_ carefully.",
            "절차 [2]_\\ 에서 설명한 대로 주의해서 진행합니다.",
        );
        let result = FormatEvaluator.evaluate(&ctx).await.unwrap();
        assert!(result.passed, "{:?}", result.issues);
    }

    #[test]
    fn rst_boundary_repair_handles_particles_on_inline_constructs() {
        assert_eq!(
            repair_rst_boundaries(
                "See ``heap``, *meaning*, :pep:`310`, and [2]_ for details.",
                "``heap``에, *의미*를, :pep:`310`뿐 아니라 Chang[2]_에서도 확인합니다.",
            ),
            "``heap``\\ 에, *의미*\\ 를, :pep:`310`\\ 뿐 아니라 Chang [2]_\\ 에서도 확인합니다."
        );
        assert_eq!(
            repair_rst_boundaries("can execute independently", "실행**될 수 있는** 작업"),
            "실행\\ **될 수 있는** 작업"
        );
        assert_eq!(
            repair_rst_boundaries(
                "The ``**kwargs: Unpack[K]`` allows *inferring* a TypedDict.",
                "``**kwargs: Unpack[K]``\\ 는 TypedDict를 *추론*할 수 있게 합니다.",
            ),
            "``**kwargs: Unpack[K]``\\ 는 TypedDict를 *추론*\\ 할 수 있게 합니다."
        );
    }

    #[test]
    fn rst_boundary_repair_keeps_roles_and_dot_names_out_of_column_zero() {
        assert_eq!(
            repair_rst_boundaries(
                "As explained in :pep:`252`, descriptors have a get method.",
                ":pep:`252`\\ 에서 설명한 것처럼 디스크립터에는 get 메서드가 있습니다.",
            ),
            "관련 :pep:`252`\\ 에서 설명한 것처럼 디스크립터에는 get 메서드가 있습니다."
        );
        assert_eq!(
            repair_rst_boundaries("The .NET platform is supported.", ".NET 플랫폼을 지원합니다."),
            "관련 .NET 플랫폼을 지원합니다."
        );
    }

    #[test]
    fn rst_boundary_repair_separates_a_url_from_a_korean_particle() {
        assert_eq!(
            repair_rst_boundaries(
                "Results are published on http://docs.python.org.",
                "결과는 http://docs.python.org에서 공개됩니다.",
            ),
            "결과는 http://docs.python.org\\ 에서 공개됩니다."
        );
    }

    #[tokio::test]
    async fn rst_translated_named_reference_alias_is_not_inline_code() {
        let ctx = context_in(
            Markup::Rst,
            "Package docutils.parsers: markup parsers_.",
            "docutils.parsers 패키지: 마크업 `파서 <parsers_>`_.",
        );
        let result = FormatEvaluator.evaluate(&ctx).await.unwrap();
        assert!(result.passed, "{:?}", result.issues);

        let ctx = context_in(
            Markup::Rst,
            "Read the `strong arguments`_ in ``python-dev``.",
            "``python-dev``\\ 에서 `강력한 주장 <strong arguments_>`_\\ 을 읽으십시오.",
        );
        let result = FormatEvaluator.evaluate(&ctx).await.unwrap();
        assert!(result.passed, "{:?}", result.issues);

        let ctx = context_in(
            Markup::Rst,
            "See `the definitions <https://example.com>`__ of an ``.add_note()`` method.",
            "`여러 ``.add_note()`` 메서드 정의 <https://example.com>`__\\ 를 보십시오.",
        );
        let result = FormatEvaluator.evaluate(&ctx).await.unwrap();
        assert!(result.passed, "{:?}", result.issues);

        let ctx = context_in(
            Markup::Rst,
            "Use ``value`` as described by the `reference`_.",
            "`참조 <reference_>`_\\ 에 설명된 대로 ``value``\\ 를 사용합니다.",
        );
        let result = FormatEvaluator.evaluate(&ctx).await.unwrap();
        assert!(result.passed, "{:?}", result.issues);
    }

    #[test]
    fn rst_boundary_repair_respects_a_source_split_inside_markup() {
        let source = "RHS`` would roughly be ``fallback``";
        let translation = "RHS`` 는 대략 ``fallback``\\ 과 같습니다";
        assert_eq!(repair_rst_boundaries(source, translation), translation);
    }

    #[test]
    fn rst_bracket_reference_detection_covers_citations_and_named_footnotes() {
        assert_eq!(
            rst_broken_bracket_references("[RFC]_는 표준입니다. [#named]_에서 계속됩니다."),
            vec!["[RFC]_는", "[#named]_에"]
        );
        assert!(rst_broken_bracket_references("[RFC]_\\ 는 표준입니다.").is_empty());
    }

    #[test]
    fn rst_prose_is_not_markup() {
        for text in [
            "결과는 2 * 3 입니다.",
            "the value 2*3 appears once",
            ":ref:`contact` 부분을 보십시오.",
            "``code`` 다음에 공백이 있습니다.",
            "인용 부호 “안”의 텍스트.",
        ] {
            assert!(
                rst_broken_pairs(text).is_empty(),
                "{text:?} carries no broken pair"
            );
        }
    }

    #[test]
    fn rst_role_content_against_a_particle_is_broken() {
        assert_eq!(
            rst_broken_pairs(":ref:`contact`를 보십시오"),
            vec!["`contact`를"]
        );
    }

    #[tokio::test]
    async fn rst_role_rejects_an_extra_closing_backtick() {
        let ctx = context_in(
            Markup::Rst,
            "PEP :pep:`649` defines the behavior.",
            ":pep:`649``\\ 에서 동작을 정의합니다.",
        );
        let result = FormatEvaluator.evaluate(&ctx).await.unwrap();
        assert!(!result.passed, "{:?}", result.issues);
        assert!(
            result
                .issues
                .iter()
                .any(|issue| issue.message.contains("extra backtick")),
            "{:?}",
            result.issues
        );
    }

    /// A translation must not begin with `..`: at the start of a line it opens
    /// a comment and swallows what follows.
    #[test]
    fn explicit_markup_start_is_recognised() {
        assert_eq!(
            line_start_construct(".. 참고하십시오"),
            Some("an explicit-markup start (`..`)")
        );
        assert_eq!(line_start_construct("... 그리고 계속됩니다"), None);
    }

    /// Curved quotes borrow the backtick but are not code, so a quotation the
    /// translation keeps as a quotation is not a code span gained.
    #[test]
    fn curved_quote_marks_are_not_counted_as_code_markers() {
        assert_eq!(mark_runs("the \"`term`\" here", '`'), 0);
        assert_eq!(mark_runs("the `code` here", '`'), 2);
        assert_eq!(mark_runs("\"`quoted`\" and `code`", '`'), 2);
    }

    #[tokio::test]
    async fn verso_allows_visible_labels_to_be_translated() {
        let ctx = context_in(
            Markup::Verso,
            "See {ref \"getting-started\"}[the previous chapter].",
            "{ref \"getting-started\"}[이전 장]을 참고하십시오.",
        );
        let result = FormatEvaluator.evaluate(&ctx).await.unwrap();
        assert!(result.passed, "{:?}", result.issues);
    }

    #[tokio::test]
    async fn verso_rejects_changed_role_headers_and_code_payloads() {
        for translation in [
            "{anchorName wrong}`List.map`을 사용합니다.",
            "{anchorName map}`List.filter`를 사용합니다.",
            "`List.map`을 사용합니다.",
        ] {
            let ctx = context_in(
                Markup::Verso,
                "Use {anchorName map}`List.map`.",
                translation,
            );
            let result = FormatEvaluator.evaluate(&ctx).await.unwrap();
            assert!(!result.passed, "{translation:?} should fail");
            assert!(
                result
                    .issues
                    .iter()
                    .any(|issue| issue.message.contains("Verso role")),
                "{:?}",
                result.issues
            );
        }
    }

    #[tokio::test]
    async fn verso_allows_roles_to_move_with_the_translated_sentence() {
        let ctx = context_in(
            Markup::Verso,
            "{lit}`lake` invokes {lit}`lean`.",
            "{lit}`lean`은 {lit}`lake`가 호출합니다.",
        );
        let result = FormatEvaluator.evaluate(&ctx).await.unwrap();
        assert!(result.passed, "{:?}", result.issues);
    }

    #[test]
    fn verso_code_nonbreaking_spaces_are_restored_before_evaluation() {
        let nbsp = '\u{a0}';
        let source = format!("Use {{lit}}`{nbsp}... ` after {{kw}}`in` and {{lit}}`{nbsp}= `.");
        let translation = "{kw}`in` 뒤에 {lit}`\\u{a0}... `와 {lit}` = `을 사용합니다.";
        let restored = restore_verso_code_whitespace(&source, translation);
        assert_eq!(
            restored,
            format!("{{kw}}`in` 뒤에 {{lit}}`{nbsp}... `와 {{lit}}`{nbsp}= `을 사용합니다.")
        );
    }

    #[test]
    fn verso_code_repair_does_not_hide_a_changed_payload() {
        let source = "Use {lit}`\u{a0}...`.";
        let translation = "{lit}`other`를 사용합니다.";
        assert_eq!(
            restore_verso_code_whitespace(source, translation),
            translation
        );
    }

    #[tokio::test]
    async fn latex_preserves_commands_math_references_and_comment_placeholders() {
        let ctx = context_in(
            Markup::Latex,
            "See \\Cref{thm:main}: a \\emph{group} $G$ works. ⟦YKTEXC0⟧",
            "\\Cref{thm:main}을 보십시오. \\emph{군} $G$는 작동합니다. ⟦YKTEXC0⟧",
        );
        let result = FormatEvaluator.evaluate(&ctx).await.unwrap();
        assert!(result.passed, "{:?}", result.issues);
    }

    #[tokio::test]
    async fn latex_rejects_changed_math_and_reference_targets() {
        for translation in [
            "\\Cref{thm:other}에 따르면 $G$가 작동합니다.",
            "\\Cref{thm:main}에 따르면 $H$가 작동합니다.",
        ] {
            let ctx = context_in(
                Markup::Latex,
                "By \\Cref{thm:main}, $G$ works.",
                translation,
            );
            let result = FormatEvaluator.evaluate(&ctx).await.unwrap();
            assert!(!result.passed, "{translation:?} should fail");
        }
    }

    #[tokio::test]
    async fn latex_allows_complete_tokens_to_move_with_korean_word_order() {
        let ctx = context_in(
            Markup::Latex,
            r"For $x \in X$, see \Cref{thm:main} and use \emph{compactness}.",
            r"\emph{콤팩트성}을 사용하고 \Cref{thm:main}을 보십시오. 단, $x \in X$입니다.",
        );
        let result = FormatEvaluator.evaluate(&ctx).await.unwrap();
        assert!(result.passed, "{:?}", result.issues);
    }

    #[tokio::test]
    async fn latex_rejects_a_dropped_duplicate_token() {
        let ctx = context_in(Markup::Latex, "$G$ acts on $G$.", "$G$가 작용합니다.");
        let result = FormatEvaluator.evaluate(&ctx).await.unwrap();
        assert!(!result.passed);
    }

    #[tokio::test]
    async fn latex_allows_additional_valid_math_notation() {
        let ctx = context_in(
            Markup::Latex,
            "The characteristic is zero and its submodules stabilize.",
            "표수는 $0$이고 $M$의 부분가군은 안정화됩니다.",
        );
        let result = FormatEvaluator.evaluate(&ctx).await.unwrap();
        assert!(result.passed, "{:?}", result.issues);
    }

    #[tokio::test]
    async fn latex_ignores_a_dropped_interword_spacing_hint() {
        let ctx = context_in(
            Markup::Latex,
            r"The value is fixed, i.e.\ it cannot move.",
            "그 값은 고정되어 있습니다. 즉, 움직일 수 없습니다.",
        );
        let result = FormatEvaluator.evaluate(&ctx).await.unwrap();
        assert!(result.passed, "{:?}", result.issues);
    }

    #[tokio::test]
    async fn latex_allows_visible_prose_inside_math_to_be_translated() {
        let ctx = context_in(
            Markup::Latex,
            r"The value is $\sup\{x \mid x \text{ compact}\}$.",
            r"그 값은 $\sup\{x \mid x \text{ 콤팩트}\}$입니다.",
        );
        let result = FormatEvaluator.evaluate(&ctx).await.unwrap();
        assert!(result.passed, "{:?}", result.issues);
    }

    #[tokio::test]
    async fn latex_allows_terminal_punctuation_to_move_out_of_math() {
        let ctx = context_in(
            Markup::Latex,
            r"Show that \[ T^\dagger = p(T). \]",
            r"다음을 보이십시오. \[ T^\dagger = p(T) \]",
        );
        let result = FormatEvaluator.evaluate(&ctx).await.unwrap();
        assert!(result.passed, "{:?}", result.issues);
    }

    #[tokio::test]
    async fn latex_allows_tex_accents_to_be_transliterated() {
        for source in [r#"G\"{o}del"#, r"\v{C}ech", r"\^{e}tre"] {
            let ctx = context_in(Markup::Latex, source, "한글 음역");
            let result = FormatEvaluator.evaluate(&ctx).await.unwrap();
            assert!(result.passed, "{source:?}: {:?}", result.issues);
        }
    }

    #[tokio::test]
    async fn latex_does_not_treat_tex_quotes_as_inline_code() {
        let ctx = context_in(Markup::Latex, "A ``group''.", "어떤 ‘군’입니다.");
        let result = FormatEvaluator.evaluate(&ctx).await.unwrap();
        assert!(result.passed, "{:?}", result.issues);
    }
}
