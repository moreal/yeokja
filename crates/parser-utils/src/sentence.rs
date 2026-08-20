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
            // Check if this is an abbreviation. A bare suffix check (`ends_with`)
            // also matches ordinary words that happen to end the same way as a
            // short abbreviation ("interest." ~ "st.", "programs." ~ "ms."), so
            // the character right before the match must not continue a word.
            let current_lower = current.to_lowercase();
            let is_abbreviation = abbreviations.iter().any(|abbr| {
                let Some(prefix_len) = current_lower.len().checked_sub(abbr.len()) else {
                    return false;
                };
                current_lower.is_char_boundary(prefix_len)
                    && &current_lower[prefix_len..] == *abbr
                    && current_lower[..prefix_len]
                        .chars()
                        .next_back()
                        .is_none_or(|c| !c.is_alphanumeric())
            });

            // A dot inside a URL token (e.g. "https://example.com/v1.Get") is
            // not a sentence boundary. Only applies when the token continues
            // after the punctuation; a URL followed by whitespace ends normally.
            let continues_token = i + 1 < len && !chars[i + 1].is_whitespace();
            let inside_url = continues_token && token_is_url(&chars, i);

            if !is_abbreviation && !inside_url {
                // Check if followed by whitespace + uppercase, or end of string
                let next_non_ws = (i + 1..len).find(|&j| !chars[j].is_whitespace());
                let at_end = i + 1 >= len || chars[i + 1..].iter().all(|c| c.is_whitespace());

                // A boundary needs whitespace after the punctuation. Without it
                // the dot sits inside a single token — `world.P"`, `` `.P` `` —
                // and splitting there tears an identifier in half.
                if at_end || (!continues_token && next_non_ws.is_some_and(|j| chars[j].is_uppercase()))
                {
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
    fn short_abbreviation_needs_a_word_boundary() {
        // "st." (Street) and "ms." (Ms./manuscript) are real abbreviations, but a
        // bare suffix check also matched ordinary words ending the same way.
        assert_eq!(
            split_sentences("It was of interest. Type classes are extensible."),
            vec!["It was of interest.", "Type classes are extensible."]
        );
        assert_eq!(
            split_sentences("These are programs. Types classify them."),
            vec!["These are programs.", "Types classify them."]
        );
        assert_eq!(
            split_sentences("Visit St. Louis. It is a city."),
            vec!["Visit St. Louis.", "It is a city."]
        );
    }

    #[test]
    fn dot_inside_token_is_not_a_boundary() {
        // The dot in `world.P"` joins one identifier; splitting there produced
        // a `P":` fragment that the translator then "completed" into prose.
        assert_eq!(
            split_sentences("you get a file \"world.P\":"),
            vec!["you get a file \"world.P\":"]
        );
        assert_eq!(
            split_sentences("In the resulting `.P` file you can see more."),
            vec!["In the resulting `.P` file you can see more."]
        );
    }

    #[test]
    fn boundary_still_splits_with_whitespace() {
        assert_eq!(
            split_sentences("See the docs. Then continue."),
            vec!["See the docs.", "Then continue."]
        );
        assert_eq!(
            split_sentences("One.\nTwo."),
            vec!["One.", "Two."]
        );
    }

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
