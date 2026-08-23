use std::collections::{HashMap, HashSet};
use std::ops::Range;
use yeokja_core::hash::content_hash;
use yeokja_core::model::*;
use yeokja_core::parser::{DocumentParser, Markup, TranslationMap};
use yeokja_parser_utils::{apply_splices, join_segments_with_translations};

/// A source-preserving parser for book-style LaTeX.
///
/// It offers visible prose, headings, theorem titles, and list items for
/// translation while leaving display math, diagrams, labels, and layout
/// commands byte-identical. Inline commands and math stay in the offered text
/// so the translator sees the complete sentence and the format evaluator can
/// require their exact preservation.
pub struct LatexParser;

const OPAQUE_ENVIRONMENTS: &[&str] = &[
    "align",
    "align*",
    "alignat",
    "alignat*",
    "aligned",
    "array",
    "asy",
    "asydef",
    "bmatrix",
    "cases",
    "chart",
    "diagram",
    "equation",
    "equation*",
    "gather",
    "gather*",
    "matrix",
    "multline",
    "multline*",
    "pmatrix",
    "smallmatrix",
    "split",
    "tabular",
    "tikzcd",
    "tikzpicture",
    "verbatim",
    "verbatim*",
];

const CODE_ENVIRONMENTS: &[&str] = &[
    "asy",
    "lstlisting",
    "minted",
    "tikzcd",
    "tikzpicture",
    "verbatim",
    "verbatim*",
];

const TITLE_COMMANDS: &[(&str, u8)] = &[
    ("part", 1),
    ("chapter", 1),
    ("chapter*", 1),
    ("section", 2),
    ("section*", 2),
    ("subsection", 3),
    ("subsection*", 3),
    ("subsubsection", 4),
    ("subsubsection*", 4),
    ("paragraph", 5),
    ("title", 1),
    ("subtitle", 2),
];

const TEXT_ARGUMENT_COMMANDS: &[&str] = &["caption", "pitch", "prototype", "todo", "missingfigure"];

/// Commands whose arguments are ordinary visible text even when the command
/// itself occurs inside display math or another otherwise opaque structure.
///
/// These arguments are offered as auxiliary segments and applied in a second
/// reconstruction pass. That lets `\text{otherwise}` be translated without
/// offering the surrounding equation, matrix, or diagram to the model.
const VISIBLE_TEXT_COMMANDS: &[&str] = &[
    "emph",
    "intertext",
    "shortintertext",
    "text",
    "textbf",
    "textit",
    "textnormal",
    "textrm",
    "textsc",
    "vocab",
];

const TITLED_ENVIRONMENTS: &[&str] = &[
    "abuse",
    "claim",
    "corollary",
    "definition",
    "dproblem",
    "example",
    "exercise",
    "fact",
    "hint",
    "lemma",
    "problem",
    "proof",
    "proposition",
    "ques",
    "remark",
    "remark*",
    "sol",
    "soln",
    "sproblem",
    "step",
    "subproof",
    "theorem",
];

#[derive(Clone, Copy)]
struct PendingBlock {
    start: usize,
    end: usize,
    block_type: BlockType,
}

struct Builder<'a> {
    source: &'a str,
    sections: Vec<Section>,
    section_idx: usize,
    block_idx: usize,
    pending: Option<PendingBlock>,
    opaque: Vec<String>,
    display_math: Option<&'static str>,
}

impl<'a> Builder<'a> {
    fn new(source: &'a str) -> Self {
        Self {
            source,
            sections: vec![Section { blocks: Vec::new() }],
            section_idx: 0,
            block_idx: 0,
            pending: None,
            opaque: Vec::new(),
            display_math: None,
        }
    }

    fn add_text(&mut self, range: Range<usize>, block_type: BlockType) {
        if range.start >= range.end {
            return;
        }
        match &mut self.pending {
            Some(run) if run.block_type == block_type => run.end = range.end,
            Some(_) => {
                self.flush();
                self.pending = Some(PendingBlock {
                    start: range.start,
                    end: range.end,
                    block_type,
                });
            }
            None => {
                self.pending = Some(PendingBlock {
                    start: range.start,
                    end: range.end,
                    block_type,
                });
            }
        }
    }

    fn flush(&mut self) {
        let Some(run) = self.pending.take() else {
            return;
        };
        self.push_span(run.start..run.end, run.block_type, None);
    }

    fn start_section(&mut self) {
        self.flush();
        if !self.sections.last().unwrap().blocks.is_empty() {
            self.sections.push(Section { blocks: Vec::new() });
            self.section_idx += 1;
            self.block_idx = 0;
        }
    }

    fn push_span(&mut self, span: Range<usize>, block_type: BlockType, heading_level: Option<u8>) {
        let raw = &self.source[span.clone()];
        let masked = mask_comments(raw).0;
        let normalized = normalize_latex_text(&masked);
        if !has_visible_prose(&normalized) {
            return;
        }
        let segment = Segment {
            id: SegmentId::new(self.section_idx, self.block_idx, 0),
            source_hash: content_hash(&normalized),
            source: normalized.clone(),
            block_type,
        };
        self.sections.last_mut().unwrap().blocks.push(Block {
            block_type,
            segments: vec![segment],
            raw_content: normalized,
            heading_level,
            span: Some(span),
            role: BlockRole::None,
            translatable: true,
        });
        self.block_idx += 1;
    }

    fn finish(mut self) -> Document {
        self.flush();
        self.sections.retain(|section| !section.blocks.is_empty());
        Document {
            sections: self.sections,
            source: self.source.to_string(),
        }
    }
}

impl DocumentParser for LatexParser {
    fn parse(&self, source: &str) -> Document {
        let mut builder = Builder::new(source);
        let mut line_start = 0usize;

        for line in source.split_inclusive('\n') {
            let content = line.strip_suffix('\n').unwrap_or(line);
            parse_line(&mut builder, content, line_start);
            line_start += line.len();
        }
        if line_start < source.len() {
            parse_line(&mut builder, &source[line_start..], line_start);
        }

        let mut document = builder.finish();
        append_visible_text_arguments(&mut document);
        document
    }

    fn reconstruct(&self, document: &Document, translations: &TranslationMap) -> String {
        let mut splices = Vec::new();
        for section in &document.sections {
            for block in &section.blocks {
                let Some(span) = &block.span else {
                    continue;
                };
                if !block.translatable
                    || !block
                        .segments
                        .iter()
                        .any(|segment| translations.contains_key(&segment.id))
                {
                    continue;
                }
                let translated = join_segments_with_translations(&block.segments, translations);
                let raw = &document.source[span.clone()];
                let (_, comments) = mask_comments(raw);
                splices.push((span.clone(), restore_comments(translated, &comments)));
            }
        }
        let reconstructed = apply_splices(&document.source, splices);
        let replacements: HashMap<_, _> = document
            .sections
            .iter()
            .flat_map(|section| &section.blocks)
            .filter(|block| block.span.is_none() && block.block_type == BlockType::Table)
            .flat_map(|block| &block.segments)
            .filter_map(|segment| {
                translations
                    .get(&segment.id)
                    .map(|translation| (segment.source.as_str(), translation.as_str()))
            })
            .collect();
        let refined = replace_visible_text_arguments(&reconstructed, &replacements);
        disambiguate_korean_after_control_words(&refined)
    }

    fn markup(&self) -> Markup {
        Markup::Latex
    }
}

fn parse_line(builder: &mut Builder<'_>, line: &str, base: usize) {
    let trimmed_start = line.len() - line.trim_start().len();
    let trimmed_end = line.trim_end().len();
    if trimmed_start >= trimmed_end {
        builder.flush();
        return;
    }
    let trimmed = &line[trimmed_start..trimmed_end];

    if !builder.opaque.is_empty() {
        update_opaque_stack(trimmed, &mut builder.opaque);
        return;
    }

    if let Some(close) = builder.display_math {
        if trimmed.contains(close) {
            builder.display_math = None;
        }
        return;
    }

    if is_comment_only(trimmed) {
        builder.flush();
        return;
    }

    if leading_command(trimmed).is_some_and(|(command, _)| command == "begin")
        && let Some((environment, _, _)) = first_environment_command(trimmed, "begin")
        && is_opaque_environment(environment)
    {
        builder.flush();
        update_opaque_stack(trimmed, &mut builder.opaque);
        return;
    }

    if trimmed.starts_with("\\[") && !trimmed.contains("\\]") {
        builder.flush();
        builder.display_math = Some("\\]");
        return;
    }
    if trimmed.starts_with("$$") && trimmed[2..].find("$$").is_none() {
        builder.flush();
        builder.display_math = Some("$$");
        return;
    }
    if (trimmed.starts_with("\\[") && trimmed.ends_with("\\]"))
        || (trimmed.starts_with("$$") && trimmed.ends_with("$$"))
    {
        builder.flush();
        return;
    }

    if let Some((command, command_end)) = leading_command(trimmed) {
        if let Some((_, level)) = TITLE_COMMANDS.iter().find(|(name, _)| *name == command) {
            builder.flush();
            if *level <= 2 {
                builder.start_section();
            }
            if let Some(inner) = mandatory_argument(trimmed, command_end) {
                let span = (base + trimmed_start + inner.start)..(base + trimmed_start + inner.end);
                builder.push_span(span, BlockType::Heading, Some(*level));
            } else if let Some(open) = trimmed[command_end..].find('{') {
                // A title may continue on later lines. Keep the visible tail
                // in a pending heading block so following lines are joined
                // into the same translatable span.
                let at = command_end + open + 1;
                let rest = &trimmed[at..];
                let leading = rest.len() - rest.trim_start().len();
                let end = rest.trim_end().len();
                if leading < end {
                    builder.add_text(
                        (base + trimmed_start + at + leading)..(base + trimmed_start + at + end),
                        BlockType::Heading,
                    );
                }
            }
            return;
        }

        if TEXT_ARGUMENT_COMMANDS.contains(&command) {
            builder.flush();
            if let Some(inner) = mandatory_argument(trimmed, command_end) {
                let span = (base + trimmed_start + inner.start)..(base + trimmed_start + inner.end);
                builder.push_span(span, BlockType::Paragraph, None);
            } else if let Some(open) = trimmed[command_end..].find('{') {
                // A command such as `\prototype{...` may continue on later
                // lines. Offer the visible tail of its first line instead of
                // silently dropping it; subsequent lines are parsed normally.
                let at = command_end + open + 1;
                let rest = &trimmed[at..];
                let leading = rest.len() - rest.trim_start().len();
                let end = rest.trim_end().len();
                if leading < end {
                    builder.add_text(
                        (base + trimmed_start + at + leading)..(base + trimmed_start + at + end),
                        BlockType::Paragraph,
                    );
                }
            }
            return;
        }

        if command == "begin" {
            builder.flush();
            if let Some((environment, _, after)) = first_environment_command(trimmed, "begin")
                && TITLED_ENVIRONMENTS.contains(&environment)
                && let Some(title) = optional_argument(trimmed, after)
            {
                let span = (base + trimmed_start + title.start)..(base + trimmed_start + title.end);
                builder.push_span(span, BlockType::Heading, Some(5));
            }
            return;
        }

        if command == "end" {
            builder.flush();
            return;
        }

        if command == "ii" || command == "item" {
            builder.flush();
            let mut at = command_end;
            if let Some(label) = optional_argument(trimmed, at) {
                at = label.end + 1;
            }
            let rest = &trimmed[at..];
            let leading = rest.len() - rest.trim_start().len();
            let end = rest.trim_end().len();
            if leading < end {
                builder.add_text(
                    (base + trimmed_start + at + leading)..(base + trimmed_start + at + end),
                    BlockType::ListItem,
                );
            }
            return;
        }

        if is_prefix_marker_command(command) {
            builder.flush();
            if let Some(tail) = visible_tail_after_prefix_commands(trimmed, command_end) {
                builder.add_text(
                    (base + trimmed_start + tail.start)..(base + trimmed_start + tail.end),
                    BlockType::Paragraph,
                );
            }
            return;
        }

        if is_layout_command(command) {
            builder.flush();
            return;
        }
    }

    if trimmed == "}" || trimmed == "{" {
        builder.flush();
        return;
    }

    let block_type = if line.trim_start().starts_with('>') {
        BlockType::BlockQuote
    } else {
        builder
            .pending
            .map_or(BlockType::Paragraph, |pending| pending.block_type)
    };
    builder.add_text((base + trimmed_start)..(base + trimmed_end), block_type);
}

fn is_prefix_marker_command(command: &str) -> bool {
    matches!(
        command,
        "footnotesize"
            | "fourchili"
            | "noindent"
            | "normalsize"
            | "onechili"
            | "par"
            | "scriptsize"
            | "small"
            | "threechili"
            | "twochili"
    )
}

fn visible_tail_after_prefix_commands(text: &str, mut at: usize) -> Option<Range<usize>> {
    loop {
        while text.as_bytes().get(at).is_some_and(u8::is_ascii_whitespace) {
            at += 1;
        }
        let Some((command, command_end)) = leading_command(&text[at..]) else {
            break;
        };
        if !is_prefix_marker_command(command) {
            break;
        }
        at += command_end;
    }

    let rest = &text[at..];
    let leading = rest.len() - rest.trim_start().len();
    let end = rest.trim_end().len();
    if leading >= end {
        return None;
    }
    let candidate = &rest[leading..end];
    if candidate.starts_with("\\fbox") || candidate.starts_with("\\includegraphics") {
        return None;
    }
    Some((at + leading)..(at + end))
}

fn leading_command(text: &str) -> Option<(&str, usize)> {
    let bytes = text.as_bytes();
    if bytes.first() != Some(&b'\\') {
        return None;
    }
    let mut end = 1usize;
    while end < bytes.len() && (bytes[end].is_ascii_alphabetic() || bytes[end] == b'@') {
        end += 1;
    }
    if end == 1 {
        return None;
    }
    if bytes.get(end) == Some(&b'*') {
        end += 1;
    }
    Some((&text[1..end], end))
}

fn mandatory_argument(text: &str, mut at: usize) -> Option<Range<usize>> {
    while text.as_bytes().get(at).is_some_and(u8::is_ascii_whitespace) {
        at += 1;
    }
    if text.as_bytes().get(at) != Some(&b'{') {
        return None;
    }
    balanced_argument(text, at, b'{', b'}')
}

fn optional_argument(text: &str, mut at: usize) -> Option<Range<usize>> {
    while text.as_bytes().get(at).is_some_and(u8::is_ascii_whitespace) {
        at += 1;
    }
    if text.as_bytes().get(at) != Some(&b'[') {
        return None;
    }
    balanced_argument(text, at, b'[', b']')
}

fn balanced_argument(text: &str, open_at: usize, open: u8, close: u8) -> Option<Range<usize>> {
    let bytes = text.as_bytes();
    let mut depth = 0usize;
    let mut at = open_at;
    while at < bytes.len() {
        if bytes[at] == open && !is_escaped(bytes, at) {
            depth += 1;
        } else if bytes[at] == close && !is_escaped(bytes, at) {
            depth -= 1;
            if depth == 0 {
                return Some((open_at + 1)..at);
            }
        }
        at += 1;
    }
    None
}

fn first_environment_command<'a>(text: &'a str, command: &str) -> Option<(&'a str, usize, usize)> {
    let prefix = format!("\\{command}{{");
    let start = text.find(&prefix)?;
    let name_start = start + prefix.len();
    let close = text[name_start..].find('}')? + name_start;
    Some((&text[name_start..close], start, close + 1))
}

fn update_opaque_stack(text: &str, stack: &mut Vec<String>) {
    let mut events = Vec::new();
    let mut cursor = 0usize;
    while cursor < text.len() {
        let begin = text[cursor..]
            .find("\\begin{")
            .map(|at| (cursor + at, true));
        let end = text[cursor..].find("\\end{").map(|at| (cursor + at, false));
        let Some((at, opening)) = (match (begin, end) {
            (Some(a), Some(b)) => Some(if a.0 <= b.0 { a } else { b }),
            (Some(a), None) => Some(a),
            (None, Some(b)) => Some(b),
            (None, None) => None,
        }) else {
            break;
        };
        let command = if opening { "begin" } else { "end" };
        if let Some((environment, _, after)) = first_environment_command(&text[at..], command) {
            events.push((opening, environment.to_string()));
            cursor = at + after;
        } else {
            cursor = at + 1;
        }
    }
    for (opening, environment) in events {
        if opening {
            stack.push(environment);
        } else if stack.last().is_some_and(|last| last == &environment) {
            stack.pop();
        }
    }
}

fn is_opaque_environment(environment: &str) -> bool {
    OPAQUE_ENVIRONMENTS.contains(&environment)
}

fn is_comment_only(text: &str) -> bool {
    text.starts_with('%') && !text.starts_with("\\%")
}

fn is_layout_command(command: &str) -> bool {
    matches!(
        command,
        "addcontentsline"
            | "appendix"
            | "author"
            | "backmatter"
            | "bibliography"
            | "bigskip"
            | "bgroup"
            | "captionof"
            | "centering"
            | "clearpage"
            | "cleardoublepage"
            | "Closesolutionfile"
            | "date"
            | "documentclass"
            | "egroup"
            | "include"
            | "includegraphics"
            | "index"
            | "input"
            | "label"
            | "mainmatter"
            | "maketitle"
            | "medskip"
            | "newcommand"
            | "newcounter"
            | "newpage"
            | "noindent"
            | "onechili"
            | "Opensolutionfile"
            | "pagebreak"
            | "par"
            | "parttoc"
            | "printbibliography"
            | "providecommand"
            | "renewcommand"
            | "setcounter"
            | "smallskip"
            | "tableofcontents"
            | "vfill"
            | "vspace"
    )
}

fn normalize_latex_text(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn has_visible_prose(text: &str) -> bool {
    let bytes = text.as_bytes();
    let mut at = 0usize;
    let mut math_delimiter: Option<&str> = None;
    while at < bytes.len() {
        if let Some(delimiter) = math_delimiter {
            if text[at..].starts_with(delimiter) && !is_escaped(bytes, at) {
                at += delimiter.len();
                math_delimiter = None;
            } else {
                at += text[at..].chars().next().map_or(1, char::len_utf8);
            }
            continue;
        }
        if bytes[at] == b'$' && !is_escaped(bytes, at) {
            let delimiter = if bytes.get(at + 1) == Some(&b'$') {
                "$$"
            } else {
                "$"
            };
            at += delimiter.len();
            math_delimiter = Some(delimiter);
            continue;
        }
        if text[at..].starts_with("\\(") {
            at += 2;
            math_delimiter = Some("\\)");
            continue;
        }
        if text[at..].starts_with("\\[") {
            at += 2;
            math_delimiter = Some("\\]");
            continue;
        }
        if bytes[at] == b'\\' {
            let command_start = at + 1;
            at = command_start;
            while at < bytes.len() && (bytes[at].is_ascii_alphabetic() || bytes[at] == b'@') {
                at += 1;
            }
            let command = &text[command_start..at];
            if opaque_argument_command(command) {
                while let Some(argument) = optional_argument(text, at) {
                    at = argument.end + 1;
                }
                if let Some(argument) = mandatory_argument(text, at) {
                    at = argument.end + 1;
                }
                continue;
            }
            if at < bytes.len() && !bytes[at].is_ascii_alphabetic() {
                at += 1;
            }
            continue;
        }
        let ch = text[at..].chars().next().unwrap();
        if ch.is_ascii_alphabetic() {
            return true;
        }
        at += ch.len_utf8();
    }
    false
}

fn opaque_argument_command(command: &str) -> bool {
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

/// Append one auxiliary block for each distinct visible-text command argument.
///
/// The blocks deliberately have no source span. Their translations are used
/// by `replace_visible_text_arguments` after ordinary paragraph splices have
/// been applied, so they can safely refine text nested inside a translated
/// paragraph without creating overlapping source edits.
fn append_visible_text_arguments(document: &mut Document) {
    let arguments = visible_text_arguments(&document.source);
    let table_cells = table_cell_spans(&document.source);
    if arguments.is_empty() && table_cells.is_empty() {
        return;
    }

    let section_idx = document.sections.len();
    let mut blocks: Vec<_> = arguments
        .into_iter()
        .enumerate()
        .map(|(block_idx, source)| {
            let segment = Segment {
                id: SegmentId::new(section_idx, block_idx, 0),
                source_hash: content_hash(&source),
                source: source.clone(),
                block_type: BlockType::Table,
            };
            Block {
                block_type: BlockType::Table,
                segments: vec![segment],
                raw_content: source,
                heading_level: None,
                span: None,
                role: BlockRole::None,
                translatable: true,
            }
        })
        .collect();

    for span in table_cells {
        let raw = &document.source[span.clone()];
        let normalized = normalize_latex_text(&mask_comments(raw).0);
        if !has_visible_prose(&normalized) {
            continue;
        }
        let block_idx = blocks.len();
        let segment = Segment {
            id: SegmentId::new(section_idx, block_idx, 0),
            source_hash: content_hash(&normalized),
            source: normalized.clone(),
            block_type: BlockType::Table,
        };
        blocks.push(Block {
            block_type: BlockType::Table,
            segments: vec![segment],
            raw_content: normalized,
            heading_level: None,
            span: Some(span),
            role: BlockRole::None,
            translatable: true,
        });
    }

    document.sections.push(Section { blocks });
}

fn visible_text_arguments(source: &str) -> Vec<String> {
    let bytes = source.as_bytes();
    let mut arguments = Vec::new();
    let mut seen = HashSet::new();
    let mut at = 0usize;

    while at < bytes.len() {
        if bytes[at] == b'%' && !is_escaped(bytes, at) {
            at = source[at..]
                .find('\n')
                .map_or(source.len(), |offset| at + offset + 1);
            continue;
        }
        if bytes[at] == b'\\'
            && let Some((command, _)) = leading_command(&source[at..])
            && command == "begin"
            && let Some((environment, _, after)) = first_environment_command(&source[at..], "begin")
            && CODE_ENVIRONMENTS.contains(&environment)
        {
            let marker = format!("\\end{{{environment}}}");
            at = source[at + after..]
                .find(&marker)
                .map_or(source.len(), |offset| at + after + offset + marker.len());
            continue;
        }
        if bytes[at] == b'\\'
            && let Some((command, command_end)) = leading_command(&source[at..])
            && let Some(argument) = visible_argument(&source[at..], command, command_end)
        {
            let normalized = normalize_latex_text(&source[at + argument.start..at + argument.end]);
            if has_english_fragment(&normalized) && seen.insert(normalized.clone()) {
                arguments.push(normalized);
            }
            at += argument.end + 1;
            continue;
        }
        at += source[at..].chars().next().map_or(1, char::len_utf8);
    }

    arguments
}

fn visible_argument(text: &str, command: &str, command_end: usize) -> Option<Range<usize>> {
    if VISIBLE_TEXT_COMMANDS.contains(&command) {
        return mandatory_argument(text, command_end);
    }
    match command {
        "hyperref" => {
            let label = optional_argument(text, command_end)?;
            mandatory_argument(text, label.end + 1)
        }
        "href" => {
            let destination = mandatory_argument(text, command_end)?;
            mandatory_argument(text, destination.end + 1)
        }
        _ => None,
    }
}

fn has_english_fragment(text: &str) -> bool {
    !text
        .chars()
        .any(|ch| ('\u{ac00}'..='\u{d7a3}').contains(&ch))
        && text
            .split(|ch: char| !ch.is_ascii_alphabetic())
            .any(|word| word.len() >= 2)
}

/// Return reader-visible cells from `tabular` environments.
///
/// The main line parser treats tables as opaque because splitting arbitrary
/// LaTeX on `&` would corrupt equations. Inside a known `tabular`, however,
/// top-level `&` and `\\` delimit cells and rows. Braces, inline math, and
/// comments are tracked so separators nested in cell content remain intact.
fn table_cell_spans(source: &str) -> Vec<Range<usize>> {
    let mut spans = Vec::new();
    let mut cursor = 0usize;
    while let Some(found) = source[cursor..].find("\\begin{tabular}") {
        let begin = cursor + found;
        let tail = &source[begin..];
        let Some((_, _, mut content_at)) = first_environment_command(tail, "begin") else {
            cursor = begin + 1;
            continue;
        };
        if let Some(optional) = optional_argument(tail, content_at) {
            content_at = optional.end + 1;
        }
        let Some(columns) = mandatory_argument(tail, content_at) else {
            cursor = begin + 1;
            continue;
        };
        let content_start = begin + columns.end + 1;
        let Some(end_offset) = source[content_start..].find("\\end{tabular}") else {
            break;
        };
        let content_end = content_start + end_offset;
        split_table_cells(source, content_start..content_end, &mut spans);
        cursor = content_end + "\\end{tabular}".len();
    }
    spans
}

fn split_table_cells(source: &str, content: Range<usize>, spans: &mut Vec<Range<usize>>) {
    let bytes = source.as_bytes();
    let mut cell_start = content.start;
    let mut at = content.start;
    let mut brace_depth = 0usize;
    let mut math = false;

    while at < content.end {
        if bytes[at] == b'%' && !is_escaped(bytes, at) {
            let next = source[at..content.end]
                .find('\n')
                .map_or(content.end, |offset| at + offset + 1);
            // A trailing comment after a row delimiter belongs to no cell.
            // Start the next cell after its newline instead of letting the
            // comment make that whole cell look comment-only.
            if source[cell_start..at].trim().is_empty() {
                cell_start = next;
            }
            at = next;
            continue;
        }
        if bytes[at] == b'$' && !is_escaped(bytes, at) {
            math = !math;
            at += 1;
            continue;
        }
        if !math {
            if bytes[at] == b'{' && !is_escaped(bytes, at) {
                brace_depth += 1;
            } else if bytes[at] == b'}' && !is_escaped(bytes, at) {
                brace_depth = brace_depth.saturating_sub(1);
            } else if brace_depth == 0 && bytes[at] == b'&' {
                push_table_cell_span(source, cell_start..at, spans);
                cell_start = at + 1;
            } else if brace_depth == 0 && bytes[at] == b'\\' && bytes.get(at + 1) == Some(&b'\\') {
                push_table_cell_span(source, cell_start..at, spans);
                at += 1;
                cell_start = at + 1;
            }
        }
        at += source[at..].chars().next().map_or(1, char::len_utf8);
    }
    push_table_cell_span(source, cell_start..content.end, spans);
}

fn push_table_cell_span(source: &str, mut span: Range<usize>, spans: &mut Vec<Range<usize>>) {
    while span.start < span.end && source.as_bytes()[span.start].is_ascii_whitespace() {
        span.start += 1;
    }
    while span.start < span.end && source.as_bytes()[span.end - 1].is_ascii_whitespace() {
        span.end -= 1;
    }
    if source[span.clone()].starts_with("\\hline") {
        span.start += "\\hline".len();
        while span.start < span.end && source.as_bytes()[span.start].is_ascii_whitespace() {
            span.start += 1;
        }
    }
    if span.start < span.end && !is_comment_only(&source[span.clone()]) {
        spans.push(span);
    }
}

fn replace_visible_text_arguments(source: &str, replacements: &HashMap<&str, &str>) -> String {
    if replacements.is_empty() {
        return source.to_string();
    }

    let bytes = source.as_bytes();
    let mut splices = Vec::new();
    let mut at = 0usize;
    while at < bytes.len() {
        if bytes[at] == b'%' && !is_escaped(bytes, at) {
            at = source[at..]
                .find('\n')
                .map_or(source.len(), |offset| at + offset + 1);
            continue;
        }
        if bytes[at] == b'\\'
            && let Some((command, command_end)) = leading_command(&source[at..])
            && let Some(argument) = visible_argument(&source[at..], command, command_end)
        {
            let span = (at + argument.start)..(at + argument.end);
            let normalized = normalize_latex_text(&source[span.clone()]);
            if let Some(replacement) = replacements.get(normalized.as_str()) {
                splices.push((span, (*replacement).to_string()));
                at += argument.end + 1;
                continue;
            }
        }
        at += source[at..].chars().next().map_or(1, char::len_utf8);
    }

    apply_splices(source, splices)
}

/// A translated Korean particle may follow a TeX control word that the source
/// terminated with `\ `, for example `\LaTeX\에서` or `\dots\을`. Under
/// LuaTeX the Hangul becomes part of the second control sequence. Replace only
/// that unsafe boundary with an empty group, preserving intentional control
/// symbols everywhere else.
fn disambiguate_korean_after_control_words(source: &str) -> String {
    let bytes = source.as_bytes();
    let mut output = String::with_capacity(source.len());
    let mut copied = 0usize;
    let mut at = 0usize;

    while at < bytes.len() {
        if bytes[at] != b'\\' {
            at += source[at..].chars().next().map_or(1, char::len_utf8);
            continue;
        }
        let mut command_end = at + 1;
        while command_end < bytes.len() && bytes[command_end].is_ascii_alphabetic() {
            command_end += 1;
        }
        if command_end == at + 1 || bytes.get(command_end) != Some(&b'\\') {
            at += 1;
            continue;
        }
        let hangul_at = command_end + 1;
        let Some(ch) = source.get(hangul_at..).and_then(|tail| tail.chars().next()) else {
            break;
        };
        if ('\u{ac00}'..='\u{d7a3}').contains(&ch) {
            output.push_str(&source[copied..command_end]);
            output.push_str("{}");
            copied = hangul_at;
            at = hangul_at;
        } else {
            at += 1;
        }
    }
    output.push_str(&source[copied..]);
    output
}

/// Replace comments with stable tokens before whitespace normalization. A `%`
/// comments out its newline, so losing that newline during translation can
/// silently hide the remainder of a LaTeX document.
fn mask_comments(text: &str) -> (String, Vec<(String, String)>) {
    let bytes = text.as_bytes();
    let mut output = String::with_capacity(text.len());
    let mut comments = Vec::new();
    let mut copied = 0usize;
    let mut at = 0usize;
    while at < bytes.len() {
        if bytes[at] == b'%' && !is_escaped(bytes, at) {
            output.push_str(&text[copied..at]);
            let end = text[at..]
                .find('\n')
                .map_or(text.len(), |offset| at + offset + 1);
            let token = format!("⟦YKTEXC{}⟧", comments.len());
            output.push_str(&token);
            comments.push((token, text[at..end].to_string()));
            copied = end;
            at = end;
        } else {
            at += text[at..].chars().next().map_or(1, char::len_utf8);
        }
    }
    output.push_str(&text[copied..]);
    (output, comments)
}

fn restore_comments(mut text: String, comments: &[(String, String)]) -> String {
    for (token, comment) in comments {
        text = text.replace(token, comment);
    }
    text
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

#[cfg(test)]
mod tests {
    use super::*;

    fn translated(document: &Document, pairs: &[(&str, &str)]) -> TranslationMap {
        let mut map = TranslationMap::new();
        for segment in document.translatable_segments() {
            if let Some((_, value)) = pairs.iter().find(|(source, _)| *source == segment.source) {
                map.insert(segment.id.clone(), (*value).to_string());
            }
        }
        map
    }

    #[test]
    fn parses_headings_prose_inline_math_and_commands() {
        let source = "\\chapter{Groups}\nA \\emph{group} $G$ has an identity.\n";
        let document = LatexParser.parse(source);
        let segments = document.translatable_segments();
        assert_eq!(segments.len(), 3);
        assert_eq!(segments[0].source, "Groups");
        assert_eq!(segments[1].source, "A \\emph{group} $G$ has an identity.");
        assert_eq!(segments[2].source, "group");

        let map = translated(
            &document,
            &[
                ("Groups", "군"),
                (
                    "A \\emph{group} $G$ has an identity.",
                    "$G$라는 \\emph{군}에는 항등원이 있습니다.",
                ),
            ],
        );
        assert_eq!(
            LatexParser.reconstruct(&document, &map),
            "\\chapter{군}\n$G$라는 \\emph{군}에는 항등원이 있습니다.\n"
        );
    }

    #[test]
    fn skips_display_math_diagrams_and_labels() {
        let source = "Intro.\n\\[ x^2 = 1 \\]\n\\label{eq:x}\n\\begin{asy}\ndraw((0,0)--(1,1));\n\\end{asy}\nOutro.\n";
        let document = LatexParser.parse(source);
        let sources: Vec<_> = document
            .translatable_segments()
            .into_iter()
            .map(|segment| segment.source.as_str())
            .collect();
        assert_eq!(sources, ["Intro.", "Outro."]);
    }

    #[test]
    fn translates_visible_text_inside_opaque_math() {
        let source = "\\[ f(x) = \\begin{cases} 1 & \\text{if $x>0$} \\\\ 0 & \\text{otherwise} \\end{cases} \\]\n";
        let document = LatexParser.parse(source);
        let sources: Vec<_> = document
            .translatable_segments()
            .into_iter()
            .map(|segment| segment.source.as_str())
            .collect();
        assert_eq!(sources, ["if $x>0$", "otherwise"]);

        let map = translated(
            &document,
            &[("if $x>0$", "$x>0$인 경우"), ("otherwise", "그 밖의 경우")],
        );
        assert_eq!(
            LatexParser.reconstruct(&document, &map),
            "\\[ f(x) = \\begin{cases} 1 & \\text{$x>0$인 경우} \\\\ 0 & \\text{그 밖의 경우} \\end{cases} \\]\n"
        );
    }

    #[test]
    fn auxiliary_text_translation_refines_an_outer_paragraph() {
        let source = "Use $f(x)=\\text{otherwise}$ now.\n";
        let document = LatexParser.parse(source);
        let map = translated(
            &document,
            &[
                (
                    "Use $f(x)=\\text{otherwise}$ now.",
                    "$f(x)=\\text{otherwise}$를 이제 사용합니다.",
                ),
                ("otherwise", "그 밖의 경우"),
            ],
        );
        assert_eq!(
            LatexParser.reconstruct(&document, &map),
            "$f(x)=\\text{그 밖의 경우}$를 이제 사용합니다.\n"
        );
    }

    #[test]
    fn translates_visible_link_labels_but_not_destinations() {
        let source = "\\hyperref[part:algebra]{Abstract Algebra} and \\href{https://example.com}{project page}.\n";
        let document = LatexParser.parse(source);
        let map = translated(
            &document,
            &[
                ("Abstract Algebra", "추상대수학"),
                ("project page", "프로젝트 페이지"),
            ],
        );
        assert_eq!(
            LatexParser.reconstruct(&document, &map),
            "\\hyperref[part:algebra]{추상대수학} and \\href{https://example.com}{프로젝트 페이지}.\n"
        );
    }

    #[test]
    fn inline_matrix_does_not_hide_the_surrounding_prose() {
        let source = "if $v = \\begin{bmatrix} 1 \\\\ 0 \\end{bmatrix}$, continue.\n";
        let document = LatexParser.parse(source);
        assert_eq!(
            document.translatable_segments()[0].source,
            "if $v = \\begin{bmatrix} 1 \\\\ 0 \\end{bmatrix}$, continue."
        );
    }

    #[test]
    fn multiline_text_argument_keeps_its_first_line() {
        let source = "\\prototype{$R$ is not Noetherian,\n  but useful.}\n";
        let document = LatexParser.parse(source);
        let sources: Vec<_> = document
            .translatable_segments()
            .into_iter()
            .map(|segment| segment.source.as_str())
            .collect();
        assert!(sources.contains(&"$R$ is not Noetherian, but useful.}"));
    }

    #[test]
    fn multiline_heading_keeps_its_first_line() {
        let source = "\\section{Local rings and residue fields:\n  linking germs to values}\n";
        let document = LatexParser.parse(source);
        let sources: Vec<_> = document
            .translatable_segments()
            .into_iter()
            .map(|segment| segment.source.as_str())
            .collect();
        assert_eq!(
            sources,
            ["Local rings and residue fields: linking germs to values}"]
        );
    }

    #[test]
    fn visible_text_in_comments_is_not_offered() {
        let source = "% \\text{not rendered}\n\\[x=1\\]\n";
        let document = LatexParser.parse(source);
        assert!(document.translatable_segments().is_empty());
    }

    #[test]
    fn visible_text_inside_diagram_code_is_not_offered() {
        let source = "\\begin{asy}\nlabel(\"$f^{\\text{pre}}(U)$\", origin);\n\\end{asy}\n\\[x=\\text{otherwise}\\]\n";
        let document = LatexParser.parse(source);
        let sources: Vec<_> = document
            .translatable_segments()
            .into_iter()
            .map(|segment| segment.source.as_str())
            .collect();
        assert_eq!(sources, ["otherwise"]);
    }

    #[test]
    fn translates_text_after_chained_prefix_commands() {
        let source =
            "\\par \\scriptsize Image from \\cite{source}\n\\noindent Last updated \\today.\n";
        let document = LatexParser.parse(source);
        let sources: Vec<_> = document
            .translatable_segments()
            .into_iter()
            .map(|segment| segment.source.as_str())
            .collect();
        assert_eq!(
            sources,
            ["Image from \\cite{source}", "Last updated \\today."]
        );
    }

    #[test]
    fn translates_visible_tabular_cells_without_touching_structure() {
        let source = "\\begin{tabular}{lc}\n  Name & Value \\\\ % first row\n  Kernel & ideal \\\\ \\hline\n  $x$ & prime element \\\\ % last row\n\\end{tabular}\n";
        let document = LatexParser.parse(source);
        let sources: Vec<_> = document
            .translatable_segments()
            .into_iter()
            .map(|segment| segment.source.as_str())
            .collect();
        assert_eq!(
            sources,
            ["Name", "Value", "Kernel", "ideal", "prime element"]
        );

        let map = translated(
            &document,
            &[
                ("Name", "이름"),
                ("Value", "값"),
                ("Kernel", "핵"),
                ("ideal", "아이디얼"),
                ("prime element", "소원소"),
            ],
        );
        assert_eq!(
            LatexParser.reconstruct(&document, &map),
            "\\begin{tabular}{lc}\n  이름 & 값 \\\\ % first row\n  핵 & 아이디얼 \\\\ \\hline\n  $x$ & 소원소 \\\\ % last row\n\\end{tabular}\n"
        );
    }

    #[test]
    fn list_markers_and_theorem_titles_stay_outside_spans() {
        let source = "\\begin{theorem}[Main result]\n\\begin{itemize}\n  \\ii First item.\n  \\ii Second item.\n\\end{itemize}\n\\end{theorem}\n";
        let document = LatexParser.parse(source);
        let sources: Vec<_> = document
            .translatable_segments()
            .into_iter()
            .map(|segment| segment.source.as_str())
            .collect();
        assert_eq!(sources, ["Main result", "First item.", "Second item."]);
    }

    #[test]
    fn parses_proposition_titles() {
        let source = "\\begin{proposition}[Equivalent conditions]\nBody.\n\\end{proposition}\n";
        let document = LatexParser.parse(source);
        let sources: Vec<_> = document
            .translatable_segments()
            .into_iter()
            .map(|segment| segment.source.as_str())
            .collect();
        assert_eq!(sources, ["Equivalent conditions", "Body."]);
    }

    #[test]
    fn comments_are_masked_and_restored_with_newlines() {
        let source = "We fuse\\footnote{%\n  This is a note.\n} paths together.\n";
        let document = LatexParser.parse(source);
        let segment = document.translatable_segments()[0];
        assert_eq!(
            segment.source,
            "We fuse\\footnote{⟦YKTEXC0⟧ This is a note. } paths together."
        );
        let mut map = TranslationMap::new();
        map.insert(
            segment.id.clone(),
            "경로를 융합합니다\\footnote{⟦YKTEXC0⟧ 이것은 주석입니다.}.".to_string(),
        );
        assert_eq!(
            LatexParser.reconstruct(&document, &map),
            "경로를 융합합니다\\footnote{%\n 이것은 주석입니다.}.\n"
        );
    }

    #[test]
    fn escaped_percent_is_visible_text_not_a_comment() {
        let source = "It succeeds with 50\\% probability.\n";
        let document = LatexParser.parse(source);
        assert_eq!(
            document.translatable_segments()[0].source,
            "It succeeds with 50\\% probability."
        );
    }

    #[test]
    fn terminates_control_words_before_korean_particles() {
        let source = "Text.\n";
        let document = LatexParser.parse(source);
        let map = translated(
            &document,
            &[("Text.", "\\LaTeX\\에서 수열 \\dots\\을 봅니다.")],
        );
        assert_eq!(
            LatexParser.reconstruct(&document, &map),
            "\\LaTeX{}에서 수열 \\dots{}을 봅니다.\n"
        );
    }

    #[test]
    fn empty_translation_map_is_byte_identical() {
        let source = "\\section{Title}\n\nSome text with $x \\in X$. % keep this\n";
        let document = LatexParser.parse(source);
        assert_eq!(
            LatexParser.reconstruct(&document, &TranslationMap::new()),
            source
        );
    }
}
