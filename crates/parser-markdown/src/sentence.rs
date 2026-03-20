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

            if !is_abbreviation {
                // Check if followed by whitespace + uppercase, or end of string
                let next_non_ws = (i + 1..len).find(|&j| !chars[j].is_whitespace());
                let at_end = i + 1 >= len || (i + 1 < len && chars[i + 1..].iter().all(|c| c.is_whitespace()));

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
}
