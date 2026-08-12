/// Split text into sentences.
/// Uses punctuation-based heuristics: splits on '.', '!', '?' followed by whitespace or end of string.
/// Preserves abbreviations like "e.g.", "i.e.", "Dr.", "Mr.", "Mrs.", "etc." by not splitting after them.
pub fn split_sentences(text: &str) -> Vec<String> {
    if text.trim().is_empty() {
        return Vec::new();
    }

    let abbreviations = [
        "e.g.", "i.e.", "etc.", "vs.", "dr.", "mr.", "mrs.", "ms.", "prof.",
        "inc.", "ltd.", "jr.", "sr.", "st.", "ave.", "dept.", "est.", "approx.",
        "fig.", "vol.", "no.",
    ];

    let mut sentences = Vec::new();
    let mut current = String::new();
    let chars: Vec<char> = text.chars().collect();
    let len = chars.len();
    let mut i = 0;

    while i < len {
        current.push(chars[i]);

        if matches!(chars[i], '.' | '!' | '?') {
            // Check if this is an abbreviation
            let current_lower = current.to_lowercase();
            let is_abbreviation = abbreviations.iter().any(|abbr| current_lower.ends_with(abbr));

            // A dot inside a URL token (e.g. "https://example.com/v1.Get") is
            // not a sentence boundary. Only applies when the token continues
            // after the punctuation; a URL followed by whitespace ends normally.
            let continues_token = i + 1 < len && !chars[i + 1].is_whitespace();
            let inside_url = continues_token && token_is_url(&chars, i);

            if !is_abbreviation && !inside_url {
                // Check if followed by whitespace + uppercase, or end of string
                let next_non_ws = (i + 1..len).find(|&j| !chars[j].is_whitespace());
                let at_end = i + 1 >= len || chars[i + 1..].iter().all(|c| c.is_whitespace());

                if at_end || next_non_ws.is_some_and(|j| chars[j].is_uppercase()) {
                    let trimmed = current.trim().to_string();
                    if !trimmed.is_empty() {
                        sentences.push(trimmed);
                    }
                    current = String::new();
                    // Skip whitespace after sentence-ending punctuation
                    while i + 1 < len && chars[i + 1].is_whitespace() {
                        i += 1;
                    }
                }
            }
        }

        i += 1;
    }

    let trimmed = current.trim().to_string();
    if !trimmed.is_empty() {
        sentences.push(trimmed);
    }

    sentences
}

/// Whether the whitespace-delimited token containing position `end` looks like a URL.
fn token_is_url(chars: &[char], end: usize) -> bool {
    let mut start = end;
    while start > 0 && !chars[start - 1].is_whitespace() {
        start -= 1;
    }
    let token: String = chars[start..=end].iter().collect();
    token.contains("://") || token.to_lowercase().starts_with("www.")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simple_sentences() {
        let result = split_sentences("Hello world. Goodbye world.");
        assert_eq!(result, vec!["Hello world.", "Goodbye world."]);
    }

    #[test]
    fn multiple_punctuation() {
        let result = split_sentences("What? Really! Yes.");
        assert_eq!(result, vec!["What?", "Really!", "Yes."]);
    }

    #[test]
    fn abbreviation_not_split() {
        let result = split_sentences("Use e.g. this method. It works.");
        assert_eq!(result, vec!["Use e.g. this method.", "It works."]);
    }

    #[test]
    fn single_sentence() {
        let result = split_sentences("Just one sentence.");
        assert_eq!(result, vec!["Just one sentence."]);
    }

    #[test]
    fn empty_input() {
        let result = split_sentences("");
        assert!(result.is_empty());
    }

    #[test]
    fn no_punctuation() {
        let result = split_sentences("No ending punctuation");
        assert_eq!(result, vec!["No ending punctuation"]);
    }

    #[test]
    fn url_with_inner_dot_not_split() {
        let result = split_sentences("Call https://api.example.com/v1.Get to fetch data.");
        assert_eq!(
            result,
            vec!["Call https://api.example.com/v1.Get to fetch data."]
        );
    }

    #[test]
    fn sentence_ending_with_url_still_splits() {
        let result = split_sentences("Visit https://example.com. Next sentence here.");
        assert_eq!(
            result,
            vec!["Visit https://example.com.", "Next sentence here."]
        );
    }

    #[test]
    fn www_url_not_split() {
        let result = split_sentences("See www.example.com/a.Bpage for details.");
        assert_eq!(result, vec!["See www.example.com/a.Bpage for details."]);
    }

    #[test]
    fn markdown_link_url_not_split() {
        let result = split_sentences(
            "Read [the guide](https://docs.example.com/guide.V2) carefully. Then start.",
        );
        assert_eq!(
            result,
            vec![
                "Read [the guide](https://docs.example.com/guide.V2) carefully.",
                "Then start."
            ]
        );
    }
}
