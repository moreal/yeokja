//! AsciiDoc table grammar.
//!
//! A table is a grid, not a sequence of lines. `[cols=...]` fixes the column
//! count and cells flow into that grid in order, so one row may be written
//! across several lines and one cell may cover several columns or rows. This
//! module owns that grammar — the attribute line, the cell specifier and the
//! placement of cells — and reports byte ranges the caller splices
//! translations into.
//!
//! One thing here is yeokja policy rather than AsciiDoc semantics: the first
//! row always names the columns. AsciiDoc only promotes it to a rendered header
//! when a blank line follows it, but selection rules address columns by that
//! text either way, and a table's first row is prose in every case worth
//! translating.

use std::ops::Range;

/// `|===` opens and closes a table.
pub fn is_delimiter(trimmed: &str) -> bool {
    trimmed.starts_with('|') && trimmed[1..].len() >= 3 && trimmed[1..].chars().all(|c| c == '=')
}

/// A row line starts with `|`, or with a cell specifier glued to its first `|`
/// (`2+|Spanning`, `a|Adoc`).
pub fn is_row(trimmed: &str) -> bool {
    match trimmed.find('|') {
        Some(0) => true,
        Some(p) => parse_cell_spec(&trimmed[..p]).is_some(),
        None => false,
    }
}

/// Column count declared by a table's block attribute line, if it declares one.
///
/// `cols=3` is three columns; `cols="1,2,1"` is three columns of those relative
/// widths; `cols="3*"` repeats one width three times. Anything else — style and
/// alignment prefixes such as `cols=">1,.^2"` — still counts one column per
/// comma-separated entry.
pub fn declared_columns(attribute_line: &str) -> Option<usize> {
    let value = attribute_value(attribute_line, "cols")?;
    let specs: Vec<&str> = value
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect();
    if specs.is_empty() {
        return None;
    }
    // A lone number is a column count, not a width.
    if let [only] = specs[..]
        && let Ok(count) = only.parse::<usize>()
    {
        return (count > 0).then_some(count);
    }
    let total: usize = specs.iter().map(|s| repetition(s)).sum();
    (total > 0).then_some(total)
}

/// Leading `N*` of a column spec — how many columns it stands for.
fn repetition(spec: &str) -> usize {
    let digits = spec.chars().take_while(char::is_ascii_digit).count();
    match spec.as_bytes().get(digits) {
        Some(b'*') if digits > 0 => spec[..digits].parse().unwrap_or(1),
        _ => 1,
    }
}

/// Value of `name=` in a `[...]` block attribute line, quotes stripped.
///
/// Splitting respects quotes so that `cols="2,3,3"` survives intact.
fn attribute_value(line: &str, name: &str) -> Option<String> {
    let inner = line.trim().strip_prefix('[')?.strip_suffix(']')?;
    let mut quote: Option<char> = None;
    let mut start = 0;
    let mut parts = Vec::new();
    for (i, c) in inner.char_indices() {
        match quote {
            Some(q) if c == q => quote = None,
            Some(_) => {}
            None if c == '"' || c == '\'' => quote = Some(c),
            None if c == ',' => {
                parts.push(&inner[start..i]);
                start = i + 1;
            }
            None => {}
        }
    }
    parts.push(&inner[start..]);

    parts.iter().find_map(|part| {
        let (key, value) = part.split_once('=')?;
        (key.trim() == name).then(|| {
            let value = value.trim();
            value
                .strip_prefix(['"', '\''])
                .and_then(|v| v.strip_suffix(['"', '\'']))
                .unwrap_or(value)
                .to_string()
        })
    })
}

/// The specifier glued to a cell's `|`, as in `3*2.2+^.^a|`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CellSpec {
    /// `3*` — how many consecutive cells this one text fills.
    pub duplication: usize,
    /// `2+` — columns covered.
    pub colspan: usize,
    /// `.2+` — rows covered.
    pub rowspan: usize,
}

impl Default for CellSpec {
    fn default() -> Self {
        Self {
            duplication: 1,
            colspan: 1,
            rowspan: 1,
        }
    }
}

impl CellSpec {
    /// Columns this cell consumes.
    fn footprint(&self) -> usize {
        self.duplication * self.colspan
    }
}

/// Parse the token glued before a `|`, or `None` when it is ordinary text.
///
/// Grammar: `[N*][C[.R]+][<^>][.<^>][style]`. Every part is optional, but the
/// token must be consumed entirely — a trailing letter that is not a style
/// letter means this was text, not a specifier.
pub fn parse_cell_spec(token: &str) -> Option<CellSpec> {
    if token.is_empty() {
        return None;
    }
    let bytes = token.as_bytes();
    let mut spec = CellSpec::default();
    let mut i = 0;

    // Duplication factor: `3*`.
    let digits = bytes.iter().take_while(|b| b.is_ascii_digit()).count();
    if digits > 0 && bytes.get(digits) == Some(&b'*') {
        spec.duplication = token[..digits].parse().ok()?;
        i = digits + 1;
    }

    // Span: `2+`, `.3+`, `2.3+` — columns before the dot, rows after.
    let run = bytes[i..]
        .iter()
        .take_while(|b| b.is_ascii_digit() || **b == b'.')
        .count();
    if run > 0 && bytes.get(i + run) == Some(&b'+') {
        let (cols, rows) = match token[i..i + run].split_once('.') {
            Some((c, r)) => (c, r),
            None => (&token[i..i + run], ""),
        };
        if !(cols.is_empty() && rows.is_empty()) {
            spec.colspan = cols.parse().unwrap_or(1).max(1);
            spec.rowspan = rows.parse().unwrap_or(1).max(1);
            i += run + 1;
        }
    }

    // Horizontal alignment, then vertical alignment, then a style letter.
    if matches!(bytes.get(i), Some(b'<' | b'^' | b'>')) {
        i += 1;
    }
    if bytes.get(i) == Some(&b'.') && matches!(bytes.get(i + 1), Some(b'<' | b'^' | b'>')) {
        i += 2;
    }
    if matches!(
        bytes.get(i),
        Some(b'a' | b'd' | b'e' | b'h' | b'l' | b'm' | b's' | b'v')
    ) {
        i += 1;
    }

    (i == token.len()).then_some(spec)
}

/// One cell as written on a row line.
#[derive(Debug, Clone)]
pub struct RawCell {
    pub spec: CellSpec,
    /// Byte range of the cell's text within the line.
    pub text: Range<usize>,
}

/// Split a row line into cells.
///
/// Separators, cell specifiers and surrounding whitespace stay outside the
/// reported ranges. Empty cells are reported too: they occupy a column, and
/// dropping them would shift every later cell left.
pub fn split_row(content: &str) -> Vec<RawCell> {
    let bytes = content.as_bytes();
    let separators: Vec<usize> = bytes
        .iter()
        .enumerate()
        .filter(|(i, b)| **b == b'|' && (*i == 0 || bytes[i - 1] != b'\\'))
        .map(|(i, _)| i)
        .collect();
    let Some(&first) = separators.first() else {
        return Vec::new();
    };

    // Whatever precedes the first `|` is that cell's specifier.
    let mut spec = parse_cell_spec(content[..first].trim()).unwrap_or_default();
    let mut cells = Vec::new();
    for (k, &separator) in separators.iter().enumerate() {
        let start = separator + 1;
        let mut end = separators.get(k + 1).copied().unwrap_or(content.len());
        let mut next = CellSpec::default();
        if k + 1 < separators.len() {
            // The non-space run glued to the next `|` may be its specifier.
            let chunk = &content[start..end];
            let token_start = chunk
                .rfind(char::is_whitespace)
                .map(|p| p + chunk[p..].chars().next().unwrap().len_utf8())
                .unwrap_or(0);
            if let Some(parsed) = parse_cell_spec(&chunk[token_start..]) {
                next = parsed;
                end = start + token_start;
            }
        }
        let text = &content[start..end];
        let text_start = start + (text.len() - text.trim_start().len());
        let text_end = start + text.trim_end().len();
        cells.push(RawCell {
            spec,
            text: text_start..text_end.max(text_start),
        });
        spec = next;
    }
    cells
}

/// Where the next cell lands, given the column count and any rowspans still
/// reaching down from earlier rows.
struct Grid {
    width: usize,
    row: usize,
    column: usize,
    /// Per column, the first row no longer covered by a rowspan above it.
    free_from: Vec<usize>,
}

impl Grid {
    fn new(width: usize) -> Self {
        let width = width.max(1);
        Self {
            width,
            row: 0,
            column: 0,
            free_from: vec![0; width],
        }
    }

    /// Place a cell, returning `(row, column)` of its top-left corner.
    fn place(&mut self, spec: &CellSpec) -> (usize, usize) {
        loop {
            if self.column >= self.width {
                self.row += 1;
                self.column = 0;
            }
            if self.free_from[self.column] > self.row {
                self.column += 1;
                continue;
            }
            break;
        }
        let (row, column) = (self.row, self.column);
        let footprint = spec.footprint();
        for slot in &mut self.free_from[column..(column + footprint).min(self.width)] {
            *slot = row + spec.rowspan;
        }
        self.column += footprint;
        (row, column)
    }
}

/// A cell placed in the grid.
#[derive(Debug, Clone)]
pub struct PlacedCell {
    /// Byte range of the cell's text within its line.
    pub text: Range<usize>,
    pub column: usize,
    /// In the first row, which names the columns.
    pub label_row: bool,
}

/// Reads one `|===` table, line by line.
pub struct TableReader {
    /// The delimiter line that closes this table.
    delimiter: String,
    /// Column count from `cols`, when the attribute line declared one.
    declared: Option<usize>,
    /// Built once the width is known: from `cols`, else from the first line.
    grid: Option<Grid>,
    /// First-row text by column.
    labels: Vec<String>,
}

impl TableReader {
    pub fn open(delimiter: &str, declared: Option<usize>) -> Self {
        Self {
            delimiter: delimiter.to_string(),
            declared,
            grid: None,
            labels: Vec::new(),
        }
    }

    pub fn closes(&self, trimmed: &str) -> bool {
        trimmed == self.delimiter
    }

    /// Place every cell on one row line.
    ///
    /// Without a `cols` attribute the first line fixes the column count, which
    /// is what AsciiDoc does — so a table whose header row is written one cell
    /// per line needs `cols` to be read correctly, and gets it.
    pub fn read_row(&mut self, content: &str) -> Vec<PlacedCell> {
        let cells = split_row(content);
        let grid = self.grid.get_or_insert_with(|| {
            let width = self
                .declared
                .unwrap_or_else(|| cells.iter().map(|c| c.spec.footprint()).sum());
            Grid::new(width)
        });

        let placed: Vec<(usize, usize, Range<usize>)> = cells
            .into_iter()
            .map(|cell| {
                let (row, column) = grid.place(&cell.spec);
                (row, column, cell.text)
            })
            .collect();

        placed
            .into_iter()
            .map(|(row, column, text)| {
                let label_row = row == 0;
                if label_row {
                    if self.labels.len() <= column {
                        self.labels.resize(column + 1, String::new());
                    }
                    self.labels[column] = content[text.clone()].trim().to_string();
                }
                PlacedCell {
                    text,
                    column,
                    label_row,
                }
            })
            .collect()
    }

    /// First-row text for a column, when that column is labelled.
    pub fn label(&self, column: usize) -> Option<String> {
        self.labels
            .get(column)
            .filter(|text| !text.is_empty())
            .cloned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cols_gives_the_column_count() {
        assert_eq!(declared_columns(r#"[cols="2,3,3"]"#), Some(3));
        assert_eq!(declared_columns("[cols=3]"), Some(3));
        assert_eq!(declared_columns(r#"[cols="3*"]"#), Some(3));
        assert_eq!(declared_columns(r#"[cols="3*1"]"#), Some(3));
        assert_eq!(declared_columns(r#"[cols="2*,1"]"#), Some(3));
        assert_eq!(declared_columns(r#"[cols=">1,.^2"]"#), Some(2));
        assert_eq!(
            declared_columns(r#"[cols="1,3", options="header"]"#),
            Some(2)
        );
        assert_eq!(declared_columns("[source,erlang]"), None);
        assert_eq!(declared_columns("[%header]"), None);
    }

    #[test]
    fn cell_specs_parse_into_spans() {
        assert_eq!(parse_cell_spec("2+"), Some(CellSpec { colspan: 2, ..Default::default() }));
        assert_eq!(parse_cell_spec(".3+"), Some(CellSpec { rowspan: 3, ..Default::default() }));
        assert_eq!(
            parse_cell_spec("2.3+"),
            Some(CellSpec { colspan: 2, rowspan: 3, ..Default::default() })
        );
        assert_eq!(
            parse_cell_spec("3*"),
            Some(CellSpec { duplication: 3, ..Default::default() })
        );
        assert_eq!(parse_cell_spec("a"), Some(CellSpec::default()));
        assert_eq!(parse_cell_spec("^.>m"), Some(CellSpec::default()));
        assert_eq!(
            parse_cell_spec("2*2.2+^.^a"),
            Some(CellSpec { duplication: 2, colspan: 2, rowspan: 2 })
        );
        // Ordinary text is not a specifier.
        assert_eq!(parse_cell_spec("word"), None);
        assert_eq!(parse_cell_spec("x"), None);
        assert_eq!(parse_cell_spec(""), None);
    }

    /// Column each cell lands in, given a width and the specs on one line.
    fn columns(width: Option<usize>, lines: &[&str]) -> Vec<usize> {
        let mut reader = TableReader::open("|===", width);
        lines
            .iter()
            .flat_map(|line| reader.read_row(line))
            .map(|cell| cell.column)
            .collect()
    }

    #[test]
    fn cols_lets_a_header_row_span_several_lines() {
        // Without `cols` the first line would fix the width at one column and
        // every later cell would pile into column 0.
        let lines = ["| Aspect", "| Interpreter", "| BeamAsm", "| Dispatch", "| A", "| B"];
        assert_eq!(columns(Some(3), &lines), vec![0, 1, 2, 0, 1, 2]);
        assert_eq!(columns(None, &lines), vec![0, 0, 0, 0, 0, 0]);
    }

    #[test]
    fn a_colspan_consumes_the_columns_it_covers() {
        let cells = columns(Some(3), &["|A |B |C", "2+|Wide |Last", "|X |Y |Z"]);
        assert_eq!(cells, vec![0, 1, 2, 0, 2, 0, 1, 2]);
    }

    #[test]
    fn a_rowspan_pushes_later_rows_past_it() {
        // `.2+|A` reserves column 0 for the next row too, so that row's first
        // cell lands in column 1.
        let cells = columns(Some(2), &["|H1 |H2", ".2+|A |B", "|C", "|D |E"]);
        assert_eq!(cells, vec![0, 1, 0, 1, 1, 0, 1]);
    }

    #[test]
    fn duplication_fills_several_columns_from_one_text() {
        let cells = columns(Some(3), &["|A |B |C", "2*|Same |Third"]);
        assert_eq!(cells, vec![0, 1, 2, 0, 2]);
    }

    #[test]
    fn empty_cells_still_occupy_their_column() {
        // `|a|` reads as a specifier, leaving an empty cell before the text.
        let cells = columns(None, &["|Type | Explanation", "|a|\tAn atom value"]);
        assert_eq!(cells, vec![0, 1, 0, 1]);
    }

    #[test]
    fn labels_come_from_the_first_row() {
        let mut reader = TableReader::open("|===", Some(3));
        reader.read_row("| Aspect");
        reader.read_row("| Interpreter");
        reader.read_row("| BeamAsm");
        assert_eq!(reader.label(0).as_deref(), Some("Aspect"));
        assert_eq!(reader.label(1).as_deref(), Some("Interpreter"));
        assert_eq!(reader.label(2).as_deref(), Some("BeamAsm"));
        assert_eq!(reader.label(3), None);
    }

    #[test]
    fn escaped_pipe_does_not_split_a_cell() {
        let cells = split_row(r"|Uses \| pipe |Second");
        let text: Vec<&str> = cells
            .iter()
            .map(|c| &r"|Uses \| pipe |Second"[c.text.clone()])
            .collect();
        assert_eq!(text, vec![r"Uses \| pipe", "Second"]);
    }

    #[test]
    fn delimiters_and_rows_are_recognised() {
        assert!(is_delimiter("|==="));
        assert!(is_delimiter("|=========="));
        assert!(!is_delimiter("|=="));
        assert!(is_row("|Cell"));
        assert!(is_row("2+|Cell"));
        assert!(is_row("a|Cell"));
        assert!(!is_row("plain text"));
        assert!(!is_row("word|other"));
    }
}
