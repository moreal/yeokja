/// Split text into sentences.
/// Uses punctuation-based heuristics: splits on '.', '!', '?' followed by whitespace or end of string.
/// Preserves abbreviations like "e.g.", "i.e.", "Dr.", "Mr.", "Mrs.", "etc." by not splitting after them.
///
/// Punctuation inside backtick-delimited inline markup — reST literals
/// (``` ``…`` ```), interpreted text and roles (`` `…` ``, `` :role:`…` ``),
/// Markdown/AsciiDoc code spans — never ends a sentence. A backtick run only
/// opens markup if it sits where an opener can (see [`can_open_markup`]) and
/// a run at least as long closes it later in the text, so a stray backtick
/// cannot swallow the rest of a paragraph. The closer may be longer than the
/// opener because reST literal content can itself end in a backtick
/// (``` ``:func:`filter``` ```).
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

    // Length of the backtick run that opened the inline markup we are in.
    let mut open_markup: Option<usize> = None;

    while i < len {
        if chars[i] == '`' {
            let run = backtick_run_len(&chars, i);
            match open_markup {
                Some(n) if run >= n => open_markup = None,
                None if can_open_markup(&chars, i) && has_closing_run(&chars, i + run, run) => {
                    open_markup = Some(run)
                }
                _ => {}
            }
            current.extend(&chars[i..i + run]);
            i += run;
            continue;
        }

        current.push(chars[i]);

        if open_markup.is_none() && matches!(chars[i], '.' | '!' | '?') {
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

/// Number of consecutive backticks starting at `start`.
fn backtick_run_len(chars: &[char], start: usize) -> usize {
    chars[start..].iter().take_while(|&&c| c == '`').count()
}

/// Whether a backtick run starting at `start` can open inline markup.
///
/// Mirrors the reST start-string rule: the opener must follow the start of
/// the text, whitespace, or an opening/joining character such as `(`, `[`,
/// `"`, `:` — never a word character or a closing bracket. That keeps a
/// *closing* backtick (`` <url>`__ ``, ``` ```url`` ```) from pairing with
/// an unrelated literal further on.
fn can_open_markup(chars: &[char], start: usize) -> bool {
    match start.checked_sub(1).map(|p| chars[p]) {
        None => true,
        Some(prev) => {
            prev.is_whitespace()
                || (!prev.is_alphanumeric() && !matches!(prev, ')' | ']' | '}' | '>'))
        }
    }
}

/// Whether a run of at least `run` backticks appears at or after `from`.
fn has_closing_run(chars: &[char], mut from: usize, run: usize) -> bool {
    while from < chars.len() {
        if chars[from] == '`' {
            let n = backtick_run_len(chars, from);
            if n >= run {
                return true;
            }
            from += n;
        } else {
            from += 1;
        }
    }
    false
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
    fn question_mark_inside_rst_literal_is_not_a_boundary() {
        // PEP 532: the `??` operator sits inside an inline literal; the splitter
        // tore the list item into "``LHS ??" and "RHS`` would roughly be ...".
        let line = "``LHS ?? RHS`` would roughly be ``is_not_sentinel(LHS, None) else RHS``";
        assert_eq!(split_sentences(line), vec![line]);
    }

    #[test]
    fn dot_inside_inline_literal_is_not_a_boundary() {
        assert_eq!(
            split_sentences("Call ``foo. Bar`` to start. Then stop."),
            vec!["Call ``foo. Bar`` to start.", "Then stop."]
        );
        // Interpreted text / roles use single backticks.
        assert_eq!(
            split_sentences("See :func:`os.path. Join` here. Then stop."),
            vec!["See :func:`os.path. Join` here.", "Then stop."]
        );
        // Markdown / AsciiDoc code spans.
        assert_eq!(
            split_sentences("Run `echo hi. Then` now. Next sentence."),
            vec!["Run `echo hi. Then` now.", "Next sentence."]
        );
    }

    #[test]
    fn sentence_ending_right_after_closing_literal_still_splits() {
        assert_eq!(
            split_sentences("Use ``foo.bar``. Next sentence starts here."),
            vec!["Use ``foo.bar``.", "Next sentence starts here."]
        );
        assert_eq!(
            split_sentences("Use `foo`. Next one."),
            vec!["Use `foo`.", "Next one."]
        );
    }

    #[test]
    fn unmatched_backtick_does_not_swallow_the_paragraph() {
        // A stray backtick (no closer later in the paragraph) is plain text.
        assert_eq!(
            split_sentences("A stray ` here. Next sentence. And another."),
            vec!["A stray ` here.", "Next sentence.", "And another."]
        );
        // A shorter run does not close a longer opener.
        assert_eq!(
            split_sentences("Mixed ``foo` bar. Next sentence."),
            vec!["Mixed ``foo` bar.", "Next sentence."]
        );
    }

    #[test]
    fn backtick_glued_to_a_closing_bracket_or_word_cannot_open_markup() {
        // A block that starts mid-link (reST footnote body) begins with the
        // link's *closing* backtick; a later ``…`` must not pair with it.
        assert_eq!(
            split_sentences(
                "<https://example.com/x>`__. This script replays commits. It runs ``make pack`` after that."
            ),
            vec![
                "<https://example.com/x>`__.",
                "This script replays commits.",
                "It runs ``make pack`` after that."
            ]
        );
        // PEP 759: ```url`` <url>`__ — the run after `com` follows a letter.
        assert_eq!(
            split_sentences(
                "Hosted on ```https://foo.example.com`` <https://foo.example.com>`__ which fails. When it fails, run ``foo`` again."
            ),
            vec![
                "Hosted on ```https://foo.example.com`` <https://foo.example.com>`__ which fails.",
                "When it fails, run ``foo`` again."
            ]
        );
        // Openers after whitespace, brackets, quotes or a role colon still work.
        assert_eq!(
            split_sentences("Use (``a. B``) and :func:`c. D` and \"`e. F`\" here. Then stop."),
            vec!["Use (``a. B``) and :func:`c. D` and \"`e. F`\" here.", "Then stop."]
        );
    }

    #[test]
    fn literal_whose_content_ends_in_a_backtick_closes_with_a_longer_run() {
        // reST: ``:func:`filter``` is a literal containing ":func:`filter`";
        // the closing run is three backticks. Staying "inside" past it merged
        // whole sentences until the next ``…`` in the paragraph.
        assert_eq!(
            split_sentences(
                "For example, ``:func:`filter``` could refer to a function. In contrast, ``:func:`foo.filter``` clearly refers to ``foo``."
            ),
            vec![
                "For example, ``:func:`filter``` could refer to a function.",
                "In contrast, ``:func:`foo.filter``` clearly refers to ``foo``."
            ]
        );
    }

    #[test]
    fn code_span_may_contain_shorter_backtick_run() {
        assert_eq!(
            split_sentences("Type `` ` `` to quote. Then go."),
            vec!["Type `` ` `` to quote.", "Then go."]
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
