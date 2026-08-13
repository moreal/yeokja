use crate::provider::TranslateRequest;
use std::collections::HashMap;

/// Build the translation prompt from a TranslateRequest.
///
/// When `request.prompt_template` is set, it is used with these placeholders:
/// `{source_lang}`, `{target_lang}`, `{glossary}`, `{feedback}`, `{context}`,
/// `{segments}`. Otherwise the built-in template below is used.
pub fn build_prompt(request: &TranslateRequest) -> String {
    let glossary_section = if request.glossary.is_empty() {
        String::new()
    } else {
        let mut terms: Vec<_> = request.glossary.iter().collect();
        terms.sort_by_key(|(k, _)| k.as_str());
        terms
            .iter()
            .map(|(term, translation)| format!("- {term} → {translation}\n"))
            .collect()
    };

    let segments_section: String = request
        .segments
        .iter()
        .map(|(idx, text)| format!("[{idx}] {text}\n"))
        .collect();

    if let Some(template) = &request.prompt_template {
        return template
            .replace("{source_lang}", &request.source_lang)
            .replace("{target_lang}", &request.target_lang)
            .replace("{glossary}", glossary_section.trim_end())
            .replace("{feedback}", request.feedback.as_deref().unwrap_or(""))
            .replace("{context}", &request.block_context)
            .replace("{segments}", segments_section.trim_end());
    }

    let mut prompt = String::new();

    prompt.push_str(&format!(
        "Translate the following sentences from {} to {}.\n",
        request.source_lang, request.target_lang
    ));
    prompt.push_str("Respond with each numbered translation in the same [N] format.\n");
    prompt.push_str("Preserve all markup exactly: links, URLs, bold/italic markers, and inline code.\n");
    prompt.push_str(closing_rule(request.markup));

    if !glossary_section.is_empty() {
        prompt.push_str("\nGlossary (use these translations for the given terms):\n");
        prompt.push_str(&glossary_section);
    }

    if let Some(feedback) = &request.feedback {
        prompt.push_str(&format!("\nPrevious translation had these issues, please fix them:\n{feedback}\n"));
    }

    prompt.push_str(&format!("\nContext (full paragraph):\n{}\n", request.block_context));

    prompt.push_str("\nSentences to translate:\n");
    prompt.push_str(&segments_section);

    prompt
}

/// The rule about inline pairs that `markup` actually has.
///
/// A language that attaches a suffix to the word before it — Korean and its
/// particles, Japanese and its — writes `` `heap`에 ``, and in AsciiDoc that
/// pair never closes: a closing mark against a word character is not a closing
/// mark. Saying so here costs one line and saves a rejected translation.
///
/// Markdown only shares the rule for `_`, and its way out is the other
/// emphasis mark rather than a doubled one: `__x__` is bold, not italic.
fn closing_rule(markup: yeokja_core::parser::Markup) -> &'static str {
    use yeokja_core::parser::Markup;
    match markup {
        Markup::Asciidoc => {
            "A closing `, * or _ that a letter follows does not close the pair. When the \
             translation puts a suffix straight after a marked-up term, double the marks \
             so the pair still closes: `heap` → ``heap``에, *bold* → **bold**를.\n"
        }
        Markup::Markdown => {
            "A closing _ that a letter follows does not close the pair. When the translation \
             puts a suffix straight after an italicised term, use * instead: _arity_ → \
             *arity*는.\n"
        }
    }
}

/// Parse a translation response in [N] format.
pub fn parse_response(response: &str) -> Result<HashMap<usize, String>, String> {
    let mut translations = HashMap::new();

    for line in response.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        if let Some(rest) = line.strip_prefix('[')
            && let Some(bracket_end) = rest.find(']') {
                let idx_str = &rest[..bracket_end];
                if let Ok(idx) = idx_str.parse::<usize>() {
                    let translation = rest[bracket_end + 1..].trim().to_string();
                    if !translation.is_empty() {
                        translations.insert(idx, translation);
                    }
                }
            }
    }

    if translations.is_empty() {
        Err("No translations found in response".to_string())
    } else {
        Ok(translations)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use yeokja_core::parser::Markup;

    fn make_request() -> TranslateRequest {
        let mut glossary = HashMap::new();
        glossary.insert("repository".to_string(), "저장소".to_string());

        TranslateRequest {
            segments: vec![
                (1, "The repository stores all history.".to_string()),
                (2, "Each commit represents a snapshot.".to_string()),
            ],
            block_context: "The repository stores all history. Each commit represents a snapshot.".to_string(),
            glossary,
            source_lang: "en".to_string(),
            target_lang: "ko".to_string(),
            markup: Markup::Markdown,
            feedback: None,
            prompt_template: None,
        }
    }

    #[test]
    fn build_prompt_includes_segments() {
        let prompt = build_prompt(&make_request());
        assert!(prompt.contains("[1] The repository stores all history."));
        assert!(prompt.contains("[2] Each commit represents a snapshot."));
    }

    #[test]
    fn build_prompt_includes_glossary() {
        let prompt = build_prompt(&make_request());
        assert!(prompt.contains("repository → 저장소"));
    }

    #[test]
    fn build_prompt_includes_feedback() {
        let mut req = make_request();
        req.feedback = Some("repository를 저장소로 번역해야 합니다".to_string());
        let prompt = build_prompt(&req);
        assert!(prompt.contains("repository를 저장소로 번역해야 합니다"));
    }

    /// The rule differs by markup, and getting it wrong is worse than silence:
    /// `__x__` is bold in Markdown, so telling it to double `_` would change
    /// what the sentence means.
    #[test]
    fn build_prompt_states_the_closing_rule_of_its_markup() {
        let mut req = make_request();
        req.markup = Markup::Asciidoc;
        let asciidoc = build_prompt(&req);
        assert!(asciidoc.contains("``heap``에"));
        assert!(asciidoc.contains("**bold**를"));

        req.markup = Markup::Markdown;
        let markdown = build_prompt(&req);
        assert!(markdown.contains("*arity*는"));
        assert!(!markdown.contains("``heap``"));
    }

    #[test]
    fn build_prompt_uses_custom_template() {
        let mut req = make_request();
        req.prompt_template = Some(
            "{source_lang}->{target_lang}\nTERMS:\n{glossary}\nTEXT:\n{segments}".to_string(),
        );
        let prompt = build_prompt(&req);
        assert!(prompt.starts_with("en->ko\n"));
        assert!(prompt.contains("TERMS:\n- repository → 저장소"));
        assert!(prompt.contains("TEXT:\n[1] The repository stores all history."));
        // The built-in template's phrasing must not leak in.
        assert!(!prompt.contains("Translate the following sentences"));
    }

    #[test]
    fn parse_response_basic() {
        let response = "[1] 저장소는 모든 이력을 저장합니다.\n[2] 각 커밋은 스냅샷을 나타냅니다.";
        let result = parse_response(response).unwrap();
        assert_eq!(result.len(), 2);
        assert_eq!(result[&1], "저장소는 모든 이력을 저장합니다.");
        assert_eq!(result[&2], "각 커밋은 스냅샷을 나타냅니다.");
    }

    #[test]
    fn parse_response_with_extra_whitespace() {
        let response = "  [1]   Hello translation.  \n\n  [2]   World translation.  ";
        let result = parse_response(response).unwrap();
        assert_eq!(result.len(), 2);
        assert_eq!(result[&1], "Hello translation.");
        assert_eq!(result[&2], "World translation.");
    }

    #[test]
    fn parse_response_empty_fails() {
        let result = parse_response("");
        assert!(result.is_err());
    }

    #[test]
    fn parse_response_no_brackets_fails() {
        let result = parse_response("Just some text without any numbered translations.");
        assert!(result.is_err());
    }
}
