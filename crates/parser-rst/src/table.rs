//! Grid and simple table geometry.
//!
//! A translated cell almost never keeps its width, and docutils reads table
//! alignment in display columns (a Hangul syllable is two), so translating a
//! cell means redrawing the whole table. The geometry here is parsed from the
//! table's source text — once when the document is parsed, to emit a block per
//! cell, and once at reconstruction, to lay the translated cells back out.
//! Both readings share `parse`, so they enumerate cells identically.
//!
//! Anything this module cannot lay out faithfully — cell spans, several header
//! borders, text bleeding into a column gap — makes `parse` return `None`, and
//! the caller keeps the table verbatim, exactly as before cell translation
//! existed.

use crate::{display_width, is_wide};

pub struct Table {
    pub kind: Kind,
    /// Leading spaces shared by every line (tables inside a block quote).
    pub indent: usize,
    /// Original usable text width of each column in display columns: the wrap
    /// target when a translation needs more room than the original had.
    pub col_widths: Vec<usize>,
    /// Display-column gap after each column but the last (simple tables only;
    /// the corpus writes both one- and two-space gaps, so they are preserved).
    pub gaps: Vec<usize>,
    pub rows: Vec<Row>,
    /// A `=` border separates header rows from the body.
    pub has_header: bool,
}

#[derive(Debug, PartialEq)]
pub enum Kind {
    Grid,
    Simple,
}

pub struct Row {
    pub cells: Vec<Cell>,
    /// This row sits above the header border and names the columns.
    pub label: bool,
    /// A blank line preceded this row in the source (simple tables allow them).
    pub blank_before: bool,
}

pub struct Cell {
    /// Normalized text: fragments trimmed and joined with single spaces — what
    /// the cell's segment carries.
    pub text: String,
    /// The source line fragments, so an untranslated cell is written back with
    /// its original wrapping.
    pub fragments: Vec<String>,
}

/// Whether a cell's text is offered for translation. Shared by parsing (which
/// emits a block per such cell) and reconstruction (which maps blocks back to
/// cells), so the two enumerations cannot drift apart.
pub fn cell_is_translatable(text: &str) -> bool {
    text.chars().any(char::is_alphanumeric)
}

pub fn parse(text: &str) -> Option<Table> {
    let lines: Vec<&str> = text.lines().map(|l| l.trim_end()).collect();
    let first = lines.first()?;
    let indent = first.len() - first.trim_start().len();
    if first.trim_start().starts_with('+') {
        parse_grid(&lines, indent)
    } else {
        parse_simple(&lines, indent)
    }
}

/// The line without the table's indent; `None` when the line is not indented
/// that far (a ragged table is not laid out).
fn strip_indent<'a>(line: &'a str, indent: usize) -> Option<&'a str> {
    if line.len() < indent || !line[..indent].chars().all(|c| c == ' ') {
        return None;
    }
    Some(&line[indent..])
}

/// Byte index where display column `col` starts, reading the line as padded
/// with virtual spaces past its end. `None` when the column falls inside a
/// double-width character.
fn byte_at_col(line: &str, col: usize) -> Option<usize> {
    let mut cur = 0;
    for (i, c) in line.char_indices() {
        if cur == col {
            return Some(i);
        }
        if cur > col {
            return None;
        }
        cur += if is_wide(c) { 2 } else { 1 };
    }
    (cur <= col).then_some(line.len())
}

/// The text covering display columns `from..to` (`None` for to-the-end).
fn col_slice<'a>(line: &'a str, from: usize, to: Option<usize>) -> Option<&'a str> {
    let a = byte_at_col(line, from)?;
    let b = match to {
        Some(to) => byte_at_col(line, to)?,
        None => line.len(),
    };
    Some(&line[a..b.max(a)])
}

fn normalize_fragments(fragments: &[String]) -> String {
    fragments
        .iter()
        .map(|f| f.as_str())
        .filter(|f| !f.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

/// Drop a cell's trailing empty fragments: they are artifacts of the row's
/// other cells being taller, and would keep the row that tall even when a
/// translation next to them got shorter.
fn trim_fragments(mut fragments: Vec<String>) -> Vec<String> {
    while fragments.last().is_some_and(|f| f.is_empty()) && fragments.len() > 1 {
        fragments.pop();
    }
    fragments
}

fn parse_grid(lines: &[&str], indent: usize) -> Option<Table> {
    let content: Option<Vec<&str>> = lines.iter().map(|l| strip_indent(l, indent)).collect();
    let content = content?;

    // Column boundaries come from the first border; every other border must
    // repeat them exactly — a `+` missing or added elsewhere is a cell span,
    // which this layout cannot redraw.
    let boundaries: Vec<usize> = content[0]
        .char_indices()
        .filter(|(_, c)| *c == '+')
        .map(|(i, _)| i)
        .collect();
    if boundaries.len() < 2
        || boundaries[0] != 0
        || *boundaries.last().unwrap() != content[0].len() - 1
        || !content[0].chars().all(|c| matches!(c, '+' | '-'))
    {
        return None;
    }
    for pair in boundaries.windows(2) {
        if pair[1] - pair[0] < 2 {
            return None;
        }
    }

    // Read the line sequence: borders delimit rows of cell lines.
    enum GridLine<'a> {
        Border(char),
        Cells(&'a str),
    }
    let mut parsed = Vec::new();
    for line in &content {
        if line.starts_with('+') {
            let plus: Vec<usize> = line
                .char_indices()
                .filter(|(_, c)| *c == '+')
                .map(|(i, _)| i)
                .collect();
            let filler: Vec<char> = line.chars().filter(|c| !matches!(c, '+')).collect();
            let kind = *filler.first()?;
            if plus != boundaries
                || !matches!(kind, '-' | '=')
                || !filler.iter().all(|c| *c == kind)
            {
                return None;
            }
            parsed.push(GridLine::Border(kind));
        } else if line.starts_with('|') {
            for &b in &boundaries {
                let at = byte_at_col(line, b)?;
                if line[at..].chars().next() != Some('|') {
                    return None;
                }
            }
            if display_width(line) != *boundaries.last().unwrap() + 1 {
                return None;
            }
            parsed.push(GridLine::Cells(line));
        } else {
            return None;
        }
    }

    let (GridLine::Border('-'), GridLine::Border('-')) = (parsed.first()?, parsed.last()?) else {
        return None;
    };

    let mut rows = Vec::new();
    let mut current: Vec<&str> = Vec::new();
    let mut header_after: Option<usize> = None;
    for line in &parsed[1..] {
        match line {
            GridLine::Cells(text) => current.push(text),
            GridLine::Border(kind) => {
                if current.is_empty() {
                    return None; // two consecutive borders
                }
                let cells = (0..boundaries.len() - 1)
                    .map(|i| {
                        let fragments: Option<Vec<String>> = current
                            .iter()
                            .map(|l| {
                                col_slice(l, boundaries[i] + 1, Some(boundaries[i + 1]))
                                    .map(|s| s.trim().to_string())
                            })
                            .collect();
                        let fragments = trim_fragments(fragments?);
                        Some(Cell {
                            text: normalize_fragments(&fragments),
                            fragments,
                        })
                    })
                    .collect::<Option<Vec<Cell>>>()?;
                rows.push(Row {
                    cells,
                    label: false,
                    blank_before: false,
                });
                current.clear();
                if *kind == '=' {
                    if header_after.is_some() {
                        return None; // a second header border
                    }
                    header_after = Some(rows.len());
                }
            }
        }
    }

    if rows.is_empty() {
        return None;
    }
    if let Some(after) = header_after {
        if after == rows.len() {
            return None; // header border closing the table
        }
        for row in &mut rows[..after] {
            row.label = true;
        }
    }

    let col_widths = boundaries
        .windows(2)
        .map(|pair| (pair[1] - pair[0] - 1).saturating_sub(2).max(1))
        .collect();

    Some(Table {
        kind: Kind::Grid,
        indent,
        col_widths,
        gaps: Vec::new(),
        rows,
        has_header: header_after.is_some(),
    })
}

fn parse_simple(lines: &[&str], indent: usize) -> Option<Table> {
    // Columns come from the runs of `=` in the top border. The last column is
    // open-ended: docutils lets its text run past the border.
    let first = strip_indent(lines[0], indent)?;
    let mut runs: Vec<(usize, usize)> = Vec::new();
    let mut pos = 0;
    for piece in first.split(' ') {
        if !piece.is_empty() {
            if !piece.chars().all(|c| c == '=') {
                return None;
            }
            runs.push((pos, pos + piece.len()));
        }
        pos += piece.len() + 1;
    }
    if runs.len() < 2 || runs[0].0 != 0 {
        return None;
    }
    let gaps: Vec<usize> = runs.windows(2).map(|p| p[1].0 - p[0].1).collect();

    let is_border = |line: &str| {
        !line.is_empty() && line.chars().all(|c| c == '=' || c == ' ')
    };

    if lines.len() < 3 || strip_indent(lines[lines.len() - 1], indent)? != first {
        return None;
    }

    let mut rows: Vec<Row> = Vec::new();
    let mut header_after: Option<usize> = None;
    let mut pending_blank = false;

    for line in &lines[1..lines.len() - 1] {
        if line.trim().is_empty() {
            pending_blank = !rows.is_empty();
            continue;
        }
        let content = strip_indent(line, indent)?;
        if content.chars().all(|c| c == '-' || c == ' ') && content.contains('-') {
            return None; // a column-span underline
        }
        if is_border(content) {
            if content != first || header_after.is_some() || rows.is_empty() {
                return None;
            }
            header_after = Some(rows.len());
            pending_blank = false;
            continue;
        }

        // Slice the line at the column boundaries; text bleeding into a gap
        // means the table does not follow the borders, so it stays verbatim.
        let mut pieces = Vec::new();
        for (i, &(start, end)) in runs.iter().enumerate() {
            let last = i == runs.len() - 1;
            let piece = col_slice(content, start, (!last).then_some(end))?;
            if !last {
                let gap = col_slice(content, end, Some(runs[i + 1].0))?;
                if !gap.chars().all(|c| c == ' ') {
                    return None;
                }
            }
            pieces.push(piece);
        }

        let head = pieces[0].trim();
        if head.is_empty() {
            // Continuation of the current row.
            let row = rows.last_mut()?;
            for (cell, piece) in row.cells.iter_mut().zip(&pieces) {
                cell.fragments.push(piece.trim().to_string());
            }
            pending_blank = false;
            continue;
        }
        // `..` marks a new row whose first cell is empty; anything else is the
        // first cell's text.
        let first_cell = if head == ".." { "" } else { head };
        let mut cells: Vec<Cell> = pieces
            .iter()
            .map(|p| Cell {
                text: String::new(),
                fragments: vec![p.trim().to_string()],
            })
            .collect();
        cells[0].fragments[0] = first_cell.to_string();
        rows.push(Row {
            cells,
            label: false,
            blank_before: pending_blank,
        });
        pending_blank = false;
    }

    if rows.is_empty() {
        return None;
    }
    for row in &mut rows {
        for cell in &mut row.cells {
            cell.fragments = trim_fragments(std::mem::take(&mut cell.fragments));
            cell.text = normalize_fragments(&cell.fragments);
        }
    }
    if let Some(after) = header_after {
        if after == rows.len() {
            return None;
        }
        for row in &mut rows[..after] {
            row.label = true;
        }
    }

    Some(Table {
        kind: Kind::Simple,
        indent,
        col_widths: runs.iter().map(|(s, e)| e - s).collect(),
        gaps,
        rows,
        has_header: header_after.is_some(),
    })
}

/// Lay the table back out with `texts` in its cells — `texts[row][col]` is the
/// translated text, `None` keeping the cell's original fragments. Column
/// widths are recomputed in display columns; translated cells wrap at spaces
/// to the column's original width where the layout allows a cell to span
/// lines.
pub fn render(table: &Table, texts: &[Vec<Option<String>>]) -> String {
    // Resolve each cell into the fragments it will occupy.
    let fragments: Vec<Vec<Vec<String>>> = table
        .rows
        .iter()
        .zip(texts)
        .map(|(row, row_texts)| {
            row.cells
                .iter()
                .enumerate()
                .map(|(c, cell)| match &row_texts[c] {
                    Some(text) if *text != cell.text => {
                        // A simple table reads a non-blank first column as a
                        // new row, so the first cell must hold one line.
                        if table.kind == Kind::Simple && c == 0 {
                            vec![text.replace('\n', " ")]
                        } else {
                            wrap(&text.replace('\n', " "), table.col_widths[c])
                        }
                    }
                    _ => cell.fragments.clone(),
                })
                .collect()
        })
        .collect();

    let empty_first_cell = |r: usize| -> bool {
        table.kind == Kind::Simple
            && fragments[r][0].iter().all(|f| f.is_empty())
            && table.rows[r].cells.len() > 1
    };

    // A column never narrows below its original width: untouched cells render
    // back byte-for-byte, and a translation only ever widens its column. The
    // last column of a simple table is unbounded — docutils lets its text run
    // past the border — so its border keeps the original length.
    let mut widths: Vec<usize> = table.col_widths.clone();
    for (r, row) in fragments.iter().enumerate() {
        for (c, cell) in row.iter().enumerate() {
            if table.kind == Kind::Simple && c == row.len() - 1 {
                continue;
            }
            for f in cell {
                widths[c] = widths[c].max(display_width(f));
            }
            if c == 0 && empty_first_cell(r) {
                widths[0] = widths[0].max(2); // room for the `..` marker
            }
        }
    }

    let mut out: Vec<String> = Vec::new();
    let indent = " ".repeat(table.indent);

    let grid_border = |kind: char| -> String {
        let mut line = String::from("+");
        for w in &widths {
            line.push_str(&kind.to_string().repeat(w + 2));
            line.push('+');
        }
        format!("{indent}{line}")
    };
    let simple_border = || -> String {
        let mut line = String::new();
        for (c, w) in widths.iter().enumerate() {
            line.push_str(&"=".repeat(*w));
            if c < widths.len() - 1 {
                line.push_str(&" ".repeat(table.gaps[c].max(1)));
            }
        }
        format!("{indent}{line}")
    };

    match table.kind {
        Kind::Grid => {
            out.push(grid_border('-'));
            let label_rows = table.rows.iter().filter(|r| r.label).count();
            for (r, row) in fragments.iter().enumerate() {
                let height = row.iter().map(|c| c.len()).max().unwrap_or(1).max(1);
                for k in 0..height {
                    let mut line = String::from("|");
                    for (c, cell) in row.iter().enumerate() {
                        let frag = cell.get(k).map(|s| s.as_str()).unwrap_or("");
                        let pad = widths[c] - display_width(frag);
                        line.push(' ');
                        line.push_str(frag);
                        line.push_str(&" ".repeat(pad + 1));
                        line.push('|');
                    }
                    out.push(format!("{indent}{line}"));
                }
                let closing = if table.has_header && r + 1 == label_rows { '=' } else { '-' };
                out.push(grid_border(closing));
            }
        }
        Kind::Simple => {
            out.push(simple_border());
            let label_rows = table.rows.iter().filter(|r| r.label).count();
            for (r, row) in fragments.iter().enumerate() {
                if r == label_rows && table.has_header {
                    out.push(simple_border());
                }
                if table.rows[r].blank_before {
                    out.push(String::new());
                }
                let height = row.iter().map(|c| c.len()).max().unwrap_or(1).max(1);
                for k in 0..height {
                    let mut line = String::new();
                    let mut col = 0;
                    for (c, cell) in row.iter().enumerate() {
                        let frag = if c == 0 && k == 0 && empty_first_cell(r) {
                            ".."
                        } else {
                            cell.get(k).map(|s| s.as_str()).unwrap_or("")
                        };
                        if !frag.is_empty() {
                            line.push_str(&" ".repeat(col - display_width(&line)));
                            line.push_str(frag);
                        }
                        col += widths[c];
                        if c < row.len() - 1 {
                            col += table.gaps[c].max(1);
                        }
                    }
                    let line = line.trim_end();
                    out.push(if line.is_empty() {
                        String::new()
                    } else {
                        format!("{indent}{line}")
                    });
                }
            }
            out.push(simple_border());
        }
    }
    out.join("\n")
}

/// Greedy wrap at single spaces by display width. Runs of spaces survive as
/// empty words, so interior spacing is preserved when no break lands on it. A
/// word wider than `target` takes a line of its own.
fn wrap(text: &str, target: usize) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    let mut current: Option<String> = None;
    for word in text.split(' ') {
        current = Some(match current {
            None => word.to_string(),
            Some(mut line) => {
                if display_width(&line) + 1 + display_width(word) <= target
                    || line.trim().is_empty()
                {
                    line.push(' ');
                    line.push_str(word);
                    line
                } else {
                    lines.push(line);
                    word.to_string()
                }
            }
        });
    }
    if let Some(line) = current {
        lines.push(line);
    }
    if lines.is_empty() {
        lines.push(String::new());
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    const GRID: &str = "\
+-------+--------------+
| value | result       |
+=======+==============+
| CC    | compiler to  |
|       | use          |
+-------+--------------+";

    #[test]
    fn grid_geometry_reads_cells_and_header() {
        let table = parse(GRID).unwrap();
        assert_eq!(table.kind, Kind::Grid);
        assert!(table.has_header);
        assert_eq!(table.rows.len(), 2);
        assert!(table.rows[0].label);
        assert_eq!(table.rows[0].cells[0].text, "value");
        assert_eq!(table.rows[1].cells[1].text, "compiler to use");
    }

    #[test]
    fn grid_renders_untranslated_cells_verbatim() {
        let table = parse(GRID).unwrap();
        let texts = vec![vec![None, None], vec![None, None]];
        assert_eq!(render(&table, &texts), GRID);
    }

    #[test]
    fn grid_redraws_to_korean_display_width() {
        let table = parse(GRID).unwrap();
        let texts = vec![
            vec![Some("값".to_string()), Some("결과".to_string())],
            vec![None, Some("사용할 컴파일러".to_string())],
        ];
        let drawn = render(&table, &texts);
        // Columns keep their original width; the second cell wraps to it, a
        // Hangul syllable counting as two columns.
        assert_eq!(
            drawn,
            "\
+-------+--------------+
| 값    | 결과         |
+=======+==============+
| CC    | 사용할       |
|       | 컴파일러     |
+-------+--------------+"
        );
        // The redrawn table reads back to the translated cells.
        let again = parse(&drawn).unwrap();
        assert_eq!(again.rows[1].cells[1].text, "사용할 컴파일러");
    }

    #[test]
    fn grid_with_a_span_is_rejected() {
        let spanned = "\
+-------+------+
| both columns |
+-------+------+";
        assert!(parse(spanned).is_none());
    }

    const SIMPLE: &str = "\
=====  ==========
Level  Description
=====  ==========
0      all off
1      some on,
       most off

jit    all on";

    #[test]
    fn simple_geometry_reads_rows_and_continuations() {
        let table = parse(&format!("{SIMPLE}\n=====  ==========")).unwrap();
        assert_eq!(table.kind, Kind::Simple);
        assert!(table.has_header);
        assert_eq!(table.rows.len(), 4);
        assert_eq!(table.rows[2].cells[1].text, "some on, most off");
        assert!(table.rows[3].blank_before);
    }

    #[test]
    fn simple_renders_untranslated_cells_verbatim() {
        let source = format!("{SIMPLE}\n=====  ==========");
        let table = parse(&source).unwrap();
        let texts: Vec<Vec<Option<String>>> =
            table.rows.iter().map(|r| vec![None; r.cells.len()]).collect();
        assert_eq!(render(&table, &texts), source);
    }

    #[test]
    fn simple_redraws_translated_cells() {
        let source = format!("{SIMPLE}\n=====  ==========");
        let table = parse(&source).unwrap();
        let mut texts: Vec<Vec<Option<String>>> =
            table.rows.iter().map(|r| vec![None; r.cells.len()]).collect();
        texts[1][1] = Some("전부 끔".to_string());
        let drawn = render(&table, &texts);
        let again = parse(&drawn).unwrap();
        assert_eq!(again.rows[1].cells[1].text, "전부 끔");
        assert_eq!(again.rows[2].cells[1].text, "some on, most off");
    }

    #[test]
    fn simple_first_column_never_wraps() {
        let source = "==  ==\naa  bb\n==  ==";
        let table = parse(source).unwrap();
        let texts = vec![vec![
            Some("아주 긴 첫 칸 번역".to_string()),
            Some("둘".to_string()),
        ]];
        let drawn = render(&table, &texts);
        let again = parse(&drawn).unwrap();
        assert_eq!(again.rows.len(), 1);
        assert_eq!(again.rows[0].cells[0].text, "아주 긴 첫 칸 번역");
    }

    #[test]
    fn dot_dot_marks_an_empty_first_cell_row() {
        let source = "===  ===\nfoo  bar\n..   baz\n===  ===";
        let table = parse(source).unwrap();
        assert_eq!(table.rows.len(), 2);
        assert_eq!(table.rows[1].cells[0].text, "");
        assert_eq!(table.rows[1].cells[1].text, "baz");
        let texts = vec![vec![None, None], vec![None, Some("바즈".to_string())]];
        let drawn = render(&table, &texts);
        let again = parse(&drawn).unwrap();
        assert_eq!(again.rows.len(), 2);
        assert_eq!(again.rows[1].cells[0].text, "");
        assert_eq!(again.rows[1].cells[1].text, "바즈");
    }

    #[test]
    fn indented_table_keeps_its_indent() {
        let source = "    ==  ==\n    aa  bb\n    ==  ==";
        let table = parse(source).unwrap();
        assert_eq!(table.indent, 4);
        let texts = vec![vec![None, Some("나".to_string())]];
        let drawn = render(&table, &texts);
        assert!(drawn.lines().all(|l| l.is_empty() || l.starts_with("    ")));
        assert_eq!(parse(&drawn).unwrap().rows[0].cells[1].text, "나");
    }

    #[test]
    fn a_column_span_underline_is_rejected() {
        let source = "==  ==\naa  bb\n------\ncc  dd\n==  ==";
        assert!(parse(source).is_none());
    }

    #[test]
    fn text_bleeding_into_a_gap_is_rejected() {
        let source = "===  ===\nfoo bar x\n===  ===";
        assert!(parse(source).is_none());
    }
}
