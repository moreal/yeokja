use std::ops::Range;

use yeokja_parser_utils::normalize_inline_text;

pub struct DirectiveTable {
    pub title: Option<DirectiveField>,
    pub cells: Vec<DirectiveCell>,
}

pub struct DirectiveField {
    pub text: String,
    pub span: Range<usize>,
}

pub struct DirectiveCell {
    pub text: String,
    pub span: Range<usize>,
    pub column: usize,
    pub label_row: bool,
    pub header: Option<String>,
    pub csv_quoted: bool,
}

struct Line<'a> {
    start: usize,
    content: &'a str,
}

impl Line<'_> {
    fn end(&self) -> usize {
        self.start + self.content.len()
    }

    fn indent(&self) -> usize {
        self.content.len() - self.content.trim_start().len()
    }

    fn is_blank(&self) -> bool {
        self.content.trim().is_empty()
    }
}

struct RawCell {
    start: usize,
    last_nonblank_end: Option<usize>,
}

impl RawCell {
    fn new(source: &str, start: usize, line_end: usize) -> Self {
        let last_nonblank_end = (!source[start..line_end].trim().is_empty()).then(|| {
            line_end - source[start..line_end].len() + source[start..line_end].trim_end().len()
        });
        Self {
            start,
            last_nonblank_end,
        }
    }

    fn include(&mut self, line: &Line<'_>) {
        if !line.is_blank() {
            self.last_nonblank_end =
                Some(line.end() - line.content.len() + line.content.trim_end().len());
        }
    }

    fn finish(self, source: &str) -> (Range<usize>, String) {
        let end = self.last_nonblank_end.unwrap_or(self.start);
        let span = self.start..end;
        let text = normalize_inline_text(&source[span.clone()]);
        (span, text)
    }
}

fn marker_text_start(line: &Line<'_>, indent: usize, marker: &str) -> Option<usize> {
    if line.indent() != indent {
        return None;
    }
    let rest = &line.content[indent..];
    let after = rest.strip_prefix(marker)?;
    if !after.is_empty() && !after.starts_with(char::is_whitespace) {
        return None;
    }
    let whitespace = after.len() - after.trim_start().len();
    Some(line.start + indent + marker.len() + whitespace)
}

fn title_after_marker(
    source: &str,
    line: &Line<'_>,
    indent: usize,
) -> Option<Option<DirectiveField>> {
    let marker = ".. list-table::";
    if line.indent() != indent {
        return None;
    }
    let after = line.content[indent..].strip_prefix(marker)?;
    let leading = after.len() - after.trim_start().len();
    let trailing = after.trim_end().len();
    Some((!after.trim().is_empty()).then(|| {
        let span = line.start + indent + marker.len() + leading
            ..line.start + indent + marker.len() + trailing;
        DirectiveField {
            text: source[span.clone()].to_string(),
            span,
        }
    }))
}

/// Parse a `list-table` directive in an absolute source range. The range is
/// expected to cover the directive line and all of its indented content.
pub fn parse_list(source: &str, range: Range<usize>) -> Option<DirectiveTable> {
    let mut offset = range.start;
    let lines: Vec<Line<'_>> = source[range.clone()]
        .split_inclusive('\n')
        .map(|raw| {
            let start = offset;
            offset += raw.len();
            let content = raw.strip_suffix('\n').unwrap_or(raw);
            let content = content.strip_suffix('\r').unwrap_or(content);
            Line { start, content }
        })
        .collect();
    let first = lines.first()?;
    let directive_indent = first.indent();
    let title = title_after_marker(source, first, directive_indent)?;

    let mut header_rows = 0usize;
    let mut saw_row = false;
    let mut rows: Vec<Vec<(Range<usize>, String)>> = Vec::new();
    let mut current_row: Option<Vec<(Range<usize>, String)>> = None;
    let mut current_cell: Option<RawCell> = None;

    for line in lines.iter().skip(1) {
        if !saw_row && line.indent() == directive_indent + 3 {
            if let Some(value) = line.content[directive_indent + 3..].strip_prefix(":header-rows:")
            {
                header_rows = value.trim().parse().ok()?;
                continue;
            }
        }

        if let Some(start) = marker_text_start(line, directive_indent + 3, "* -") {
            saw_row = true;
            if let Some(cell) = current_cell.take() {
                current_row.as_mut()?.push(cell.finish(source));
            }
            if let Some(row) = current_row.take() {
                rows.push(row);
            }
            current_row = Some(Vec::new());
            current_cell = Some(RawCell::new(source, start, line.end()));
            continue;
        }

        if let Some(start) = marker_text_start(line, directive_indent + 5, "-") {
            let row = current_row.as_mut()?;
            if let Some(cell) = current_cell.take() {
                row.push(cell.finish(source));
            }
            current_cell = Some(RawCell::new(source, start, line.end()));
            continue;
        }

        if let Some(cell) = &mut current_cell {
            cell.include(line);
        }
    }
    if let Some(cell) = current_cell {
        current_row.as_mut()?.push(cell.finish(source));
    }
    if let Some(row) = current_row {
        rows.push(row);
    }

    if header_rows > rows.len() {
        return None;
    }
    if let Some(expected_columns) = rows
        .iter()
        .find(|row| row.iter().any(|(_, text)| !text.is_empty()))
        .map(Vec::len)
    {
        if rows
            .iter()
            .filter(|row| row.iter().any(|(_, text)| !text.is_empty()))
            .any(|row| row.len() != expected_columns)
        {
            return None;
        }
    }

    let columns = rows.iter().map(Vec::len).max().unwrap_or(0);
    let mut headers = vec![None; columns];
    for row in rows.iter().take(header_rows) {
        for (column, (_, text)) in row.iter().enumerate() {
            if !text.is_empty() {
                headers[column] = Some(text.clone());
            }
        }
    }

    let cells = rows
        .into_iter()
        .enumerate()
        .flat_map(|(row, cells)| {
            let label_row = row < header_rows;
            cells.into_iter().enumerate().map({
                let headers = headers.clone();
                move |(column, (span, text))| DirectiveCell {
                    text,
                    span,
                    column,
                    label_row,
                    header: (!label_row).then(|| headers[column].clone()).flatten(),
                    csv_quoted: false,
                }
            })
        })
        .collect();

    Some(DirectiveTable { title, cells })
}
