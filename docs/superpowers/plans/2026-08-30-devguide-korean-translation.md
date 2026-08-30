# Python Devguide Korean Translation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a reproducible yeokja project that translates the complete pinned `python/devguide` RST corpus into Korean with Codex and produces a verified Sphinx HTML site.

**Architecture:** Extend the span-based RST parser only for the two reader-facing directive table forms proven missing from the current parser, then mirror all 64 pinned RST files into yeokja state and derived Korean output. Assemble the translated overlay over the untouched upstream submodule, append narrowly scoped Korean Sphinx configuration in the disposable tree, and gate completion with status, mechanical evaluation, source/state/output, HTML-link, and warning-as-error build checks.

**Tech Stack:** Rust 2024, yeokja RST parser/core/CLI, Python 3 standard library, Sphinx 9.1+, TOML, Pi provider with OpenAI Codex `gpt-5.6-sol`.

**Spec:** `docs/superpowers/specs/2026-08-30-devguide-korean-translation-design.md`

## Global Constraints

- Pin `python/devguide` at commit `261dc2116ca81985c5c0cfc59db5a251d2c8db96`.
- Do not configure this submodule as shallow: `sphinx-last-updated-by-git` emits one `git.too_shallow` warning per source file for a shallow clone.
- Translate every one of the 64 `*.rst` files under the pinned upstream tree, including `README.rst` and `include/*.rst`.
- Use `[provider] type = "pi"`, `model = "gpt-5.6-sol"`, and `base_url = "openai-codex"`; do not substitute another provider or model.
- Keep `projects/devguide/upstream` read-only and keep `projects/devguide/state` as the authoritative committed translation data.
- Do not commit `projects/devguide/ko`, `projects/devguide/build`, `projects/devguide/dist`, or a Python virtual environment.
- Preserve code, commands, paths, URLs, GitHub usernames, personal names, version numbers, and table identifier columns byte-for-byte unless they are reader-facing prose.
- Treat every Sphinx warning as an error. The only pinned-upstream nitpick exception is the exact pair `("rst:role", "py:func")` from `getting-started/pull-request-lifecycle.rst`.
- Every commit created by this plan must contain exactly one trailer: `Assisted-by: Codex:gpt-5.6-sol`.
- Never claim the complete translation is finished from a sample: all status, state, output, evaluator, build, and link gates must pass.

## File Map

- Modify `crates/parser-rst/src/lib.rs`: detect directive tables, emit table-cell spans, and reconstruct translated directive cells safely.
- Create `crates/parser-rst/src/directive_table.rs`: parse inline `list-table` and `csv-table` syntax into stable absolute spans without depending on Sphinx at runtime.
- Modify `crates/core/src/config.rs`: add an explicit `headerless` matcher for table-selection rules.
- Modify `crates/core/src/select.rs`: apply a `headerless = true` rule only to a table whose parser reported no header row.
- Modify `README.md`: document the explicit headerless-table selector.
- Modify `.gitmodules`: register the full-history devguide upstream submodule.
- Create `projects/devguide/.gitignore`: ignore all derived and environment outputs.
- Create `projects/devguide/README.md`: document scope, license, commands, upstream updates, and authoritative state.
- Create `projects/devguide/glossary.toml`: fix recurring CPython development terminology.
- Create `projects/devguide/yeokja.toml`: define all RST sources, table selection, Codex, evaluation, derivation, and HTML build.
- Create `projects/devguide/scripts/prepare.py`: append deterministic Korean Sphinx settings and the one exact nitpick exception.
- Create `projects/devguide/scripts/test_prepare.py`: unit-test preparation collision handling and idempotence.
- Create `projects/devguide/scripts/audit.py`: verify source/state/output completeness and local HTML links/anchors.
- Create `projects/devguide/scripts/test_audit.py`: unit-test missing state, incomplete segments, missing HTML targets, and missing anchors.
- Create `projects/devguide/state/**/*.yeokja.json`: authoritative Codex translations for all source files.

---

### Task 1: Parse and reconstruct RST `list-table` directives

**Files:**
- Create: `crates/parser-rst/src/directive_table.rs`
- Modify: `crates/parser-rst/src/lib.rs`
- Test: `crates/parser-rst/src/lib.rs`

**Interfaces:**
- Consumes: the existing parser's source text, absolute directive range, `table_idx`, `make_segments`, `BlockRole::TableCell`, and `TranslationMap`.
- Produces: `directive_table::parse_list(source: &str, range: Range<usize>) -> Option<DirectiveTable>` and `directive_table_edits(document: &Document, translations: &TranslationMap) -> Vec<(Range<usize>, String)>`.
- `DirectiveTable` contains `title: Option<DirectiveField>` and row-major `cells: Vec<DirectiveCell>`; each cell carries normalized `text`, absolute `span`, `column`, `label_row`, and `header`.

- [ ] **Step 1: Add failing parsing and reconstruction tests**

Add tests with these exact fixtures to the existing `tests` module in `crates/parser-rst/src/lib.rs`:

```rust
#[test]
fn list_table_cells_are_segments_with_headers() {
    let source = ".. list-table::\n   :header-rows: 1\n\n   * - Avoid\n     - Instead\n   * - whitelist\n     - allowlist\n";
    let doc = RstParser.parse(source);
    let sources: Vec<&str> = doc
        .translatable_segments()
        .iter()
        .map(|segment| segment.source.as_str())
        .collect();
    assert_eq!(sources, vec!["Avoid", "Instead", "whitelist", "allowlist"]);

    let cells: Vec<&Block> = doc.sections.iter().flat_map(|s| &s.blocks)
        .filter(|block| matches!(block.role, BlockRole::TableCell { .. }))
        .collect();
    assert!(matches!(cells[0].role,
        BlockRole::TableCell { table: 0, column: 0, label_row: true, .. }));
    assert!(matches!(&cells[3].role,
        BlockRole::TableCell { table: 0, column: 1, label_row: false, header: Some(header) }
        if header == "Instead"));
}

#[test]
fn translated_list_table_keeps_directive_structure() {
    let source = ".. list-table::\n   :header-rows: 1\n\n   * - Avoid\n     - Instead\n   * - whitelist\n     - allowlist\n";
    let output = translate_all(
        &RstParser,
        source,
        &[("Avoid", "피할 표현"), ("Instead", "대신 사용할 표현"),
          ("whitelist", "허용 목록"), ("allowlist", "허용 목록")],
    );
    assert_eq!(output,
        ".. list-table::\n   :header-rows: 1\n\n   * - 피할 표현\n     - 대신 사용할 표현\n   * - 허용 목록\n     - 허용 목록\n");
}

#[test]
fn translated_wrapped_list_table_cell_collapses_without_touching_next_cell() {
    let source = ".. list-table::\n\n   * - `Usage <https://example.com>`__,\n       `Limitations <https://example.com/limits>`__\n     - maintainer\n";
    let output = translate_all(
        &RstParser,
        source,
        &[("`Usage <https://example.com>`__, `Limitations <https://example.com/limits>`__",
           "`사용법 <https://example.com>`__, `제한 사항 <https://example.com/limits>`__")],
    );
    assert!(output.contains("   * - `사용법 <https://example.com>`__, `제한 사항 <https://example.com/limits>`__\n     - maintainer"));
}
```

- [ ] **Step 2: Run the focused tests and verify the unsupported behavior**

Run:

```bash
cargo test -p yeokja-parser-rst list_table -- --nocapture
```

Expected: all three new tests fail because `list-table` is currently listed in `OPAQUE_DIRECTIVES` and offers no cells.

- [ ] **Step 3: Implement the list-table span parser**

Create `directive_table.rs` with these public data contracts:

```rust
use std::ops::Range;

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

pub fn parse_list(source: &str, range: Range<usize>) -> Option<DirectiveTable>;
```

Implement `parse_list` with the following exact grammar and failure behavior:

1. Require a first line matching `.. list-table::` at the directive indentation.
2. Parse an optional title after `::` as a `DirectiveField` span excluding surrounding whitespace.
3. Read `:header-rows: N`; absent means `0`. Reject non-integer or values larger than the row count.
4. A row begins only at directive indent + 3 with `* -`; later cells begin at directive indent + 5 with `-`. Deeper `*` or `-` lines belong to the current cell.
5. A cell span begins after its `-` marker and ends at the last nonblank continuation byte before the next cell or row. Normalize its text with the same whitespace joining used by ordinary RST blocks.
6. Empty cells produce no block but still advance the column. Every nonempty row must have the same column count; return `None` for malformed geometry so the caller preserves the directive byte-for-byte.
7. Mark rows `0..header_rows` as label rows and attach their text to later cells as `header`.

In `lib.rs`, add `mod directive_table;`, remove only `list-table` from `OPAQUE_DIRECTIVES`, detect it before generic directive handling, and emit:

- one untranslatable `BlockType::Table` anchor spanning the whole directive;
- an ordinary heading/paragraph block for a nonempty directive title;
- one spanned `BlockType::Table`/`BlockRole::TableCell` block per nonempty cell.

Add `directive_table_edits` beside `table_edits`. It must join translations for spanned table cells, remove those segment IDs from the map passed to `collect_splices`, and emit nonoverlapping cell-span edits. Geometry tables continue through `table_edits` unchanged.

- [ ] **Step 4: Run focused and crate-wide tests**

Run:

```bash
cargo test -p yeokja-parser-rst list_table -- --nocapture
cargo test -p yeokja-parser-rst
```

Expected: the new tests pass and all existing parser tests remain green.

- [ ] **Step 5: Commit the list-table support**

```bash
git add crates/parser-rst/src/directive_table.rs crates/parser-rst/src/lib.rs
git commit -m "feat(rst): translate list-table cells" \
  -m "Assisted-by: Codex:gpt-5.6-sol"
```

---

### Task 2: Parse inline RST `csv-table` directives safely

**Files:**
- Modify: `crates/parser-rst/src/directive_table.rs`
- Modify: `crates/parser-rst/src/lib.rs`
- Test: `crates/parser-rst/src/lib.rs`

**Interfaces:**
- Consumes: `DirectiveTable`, `DirectiveField`, `DirectiveCell`, and `directive_table_edits` from Task 1.
- Produces: `directive_table::parse_csv(source: &str, range: Range<usize>) -> Option<DirectiveTable>`.
- Guarantees: only quoted inline CSV fields are translatable; unquoted author/version identifiers and `:file:` CSV tables remain verbatim.

- [ ] **Step 1: Add failing CSV directive tests**

```rust
#[test]
fn inline_csv_table_offers_title_headers_and_quoted_prose_only() {
    let source = ".. csv-table:: **Current references**\n   :header: \"Title\", \"Brief\", \"Author\", \"Version\"\n\n    \"Guide\", \"Parser docs\", Louie Lu, 3.15\n";
    let doc = RstParser.parse(source);
    let sources: Vec<&str> = doc.translatable_segments().iter()
        .map(|segment| segment.source.as_str()).collect();
    assert_eq!(sources, vec![
        "**Current references**", "Title", "Brief", "Author", "Version",
        "Guide", "Parser docs",
    ]);
    assert!(!sources.contains(&"Louie Lu"));
    assert!(!sources.contains(&"3.15"));
}

#[test]
fn csv_translation_escapes_ascii_quotes_inside_quoted_field() {
    let source = ".. csv-table::\n   :header: \"Title\", \"Brief\"\n\n    \"Guide\", \"Parser docs\"\n";
    let output = translate_all(
        &RstParser,
        source,
        &[("Guide", "안내서"), ("Parser docs", "파서 \"문서\"")],
    );
    assert!(output.contains("\"안내서\", \"파서 \"\"문서\"\"\""));
}

#[test]
fn file_backed_csv_table_stays_opaque() {
    let source = ".. csv-table::\n   :header-rows: 1\n   :file: include/branches.csv\n";
    assert!(RstParser.parse(source).translatable_segments().is_empty());
}
```

- [ ] **Step 2: Run the tests and verify they fail for inline CSV**

Run: `cargo test -p yeokja-parser-rst csv_table -- --nocapture`

Expected: the first two tests fail because `csv-table` is opaque; the file-backed safety test already passes.

- [ ] **Step 3: Implement narrow inline CSV support**

Add this interface:

```rust
pub fn parse_csv(source: &str, range: Range<usize>) -> Option<DirectiveTable>;
```

Implementation rules:

1. Return `None` immediately if the option block contains `:file:` or `:url:`.
2. Parse the optional directive title as in `parse_list`.
3. Parse `:header:` as one logical CSV record and inline data as one record per indented nonblank line.
4. Support RFC 4180 quoted fields and unquoted fields, but create `DirectiveCell` blocks only for quoted fields. Preserve unquoted fields as identifiers.
5. Record spans inside the surrounding quotes and set `csv_quoted = true`; reject unterminated quotes, embedded newlines, and unequal row widths by returning `None` for whole-directive preservation.
6. Mark `:header:` cells as `label_row = true`; attach those labels to data cells by column.
7. In `directive_table_edits`, replace every `"` in a translated `csv_quoted` field with `""` before splicing it between the preserved outer quotes.

Remove only inline `csv-table` handling from the opaque path: unsupported/file-backed CSV directives must still become one opaque block.

- [ ] **Step 4: Verify CSV, list-table, and all RST parser tests**

```bash
cargo test -p yeokja-parser-rst csv_table -- --nocapture
cargo test -p yeokja-parser-rst list_table -- --nocapture
cargo test -p yeokja-parser-rst
```

Expected: all commands exit 0.

- [ ] **Step 5: Commit inline CSV support**

```bash
git add crates/parser-rst/src/directive_table.rs crates/parser-rst/src/lib.rs
git commit -m "feat(rst): translate inline csv-table prose" \
  -m "Assisted-by: Codex:gpt-5.6-sol"
```

---

### Task 3: Select columns in explicitly headerless tables

**Files:**
- Modify: `crates/core/src/config.rs`
- Modify: `crates/core/src/select.rs`
- Modify: `README.md`
- Test: `crates/core/src/config.rs`
- Test: `crates/core/src/select.rs`

**Interfaces:**
- Consumes: existing `TableRule`, `TableGroup.headers`, file globs, and column selectors.
- Produces: `TableRule.headerless: bool` with a serde default of `false`.
- Matching rule: `headerless = true` matches only `headers.is_empty()`; it is mutually exclusive with a nonempty `headers` array.

- [ ] **Step 1: Write failing config and selection tests**

Add these focused tests. The config test parses:

```toml
[[tables]]
files = "upstream/development-tools/clinic/howto.rst"
headers = []
headerless = true
skip = [0, 1]
```

and asserts `rule.headerless` is true:

```rust
#[test]
fn parse_explicit_headerless_table_rule() {
    let config = ProjectConfig::from_toml(r#"
[project]
source_lang = "en"
target_lang = "ko"

[provider]
type = "pi"
model = "gpt-5.6-sol"

[[tables]]
files = "upstream/development-tools/clinic/howto.rst"
headers = []
headerless = true
skip = [0, 1]
"#).unwrap();
    assert!(config.tables[0].headerless);
    assert!(config.tables[0].headers.is_empty());
}

#[test]
fn reject_headerless_rule_with_named_headers() {
    let error = ProjectConfig::from_toml(r#"
[project]
source_lang = "en"
target_lang = "ko"

[provider]
type = "pi"
model = "gpt-5.6-sol"

[[tables]]
headers = ["Name"]
headerless = true
skip = [0]
"#).unwrap_err();
    assert!(error.to_string().contains("sets both headerless = true and headers"));
}
```

In `select.rs`, add this helper and test beside the existing table tests:

```rust
fn unlabelled_cell(table: usize, column: usize, text: &str) -> Block {
    block(
        text,
        BlockRole::TableCell {
            table,
            column,
            label_row: false,
            header: None,
        },
    )
}

#[test]
fn headerless_rule_does_not_match_a_labelled_table() {
    let mut doc = instruction_table();
    doc.sections[0].blocks.extend([
        unlabelled_cell(2, 0, "'B'"),
        unlabelled_cell(2, 1, "unsigned_char"),
    ]);
    let rules = vec![TableRule {
        files: Some("development-tools/clinic/howto.rst".to_string()),
        headers: vec![],
        headerless: true,
        translate: vec![],
        skip: vec![ColumnRef::Index(0), ColumnRef::Index(1)],
    }];
    assert_eq!(
        apply_table_rules(
            &mut doc,
            &rules,
            Path::new("development-tools/clinic/howto.rst"),
        ),
        2,
    );
    assert_eq!(kept(&doc), vec!["allocate", "t t", "Allocate stack words"]);
}
```

- [ ] **Step 2: Run the focused tests and observe failure**

```bash
cargo test -p yeokja-core headerless -- --nocapture
```

Expected: compilation or assertions fail because `headerless` does not exist.

- [ ] **Step 3: Implement explicit headerless matching**

Add to `TableRule`:

```rust
/// Match a table that has no parser-reported label row. This must be explicit;
/// an accidentally empty `headers` list never broadens a rule on its own.
#[serde(default)]
pub headerless: bool,
```

Replace the start of `matches_headers` with:

```rust
if self.headerless {
    return self.headers.is_empty() && headers.is_empty();
}
if self.headers.is_empty() {
    return false;
}
```

Add `ConfigError::Invalid(String)` with display text `Invalid config: {0}`. In
`ProjectConfig::from_toml`, deserialize first, then enumerate table rules and return
`ConfigError::Invalid(format!("table rule {index} sets both headerless = true and headers"))`
when `headerless` and a nonempty `headers` array occur together. Update every test
constructor of `TableRule` to set `headerless: false`.

Document this syntax in the README after the existing table-column examples, explicitly warning that `files` should scope a headerless rule narrowly.

- [ ] **Step 4: Run core and workspace regression tests**

```bash
cargo test -p yeokja-core headerless -- --nocapture
cargo test -p yeokja-core
cargo test --workspace
```

Expected: all commands exit 0.

- [ ] **Step 5: Commit the selector**

```bash
git add crates/core/src/config.rs crates/core/src/select.rs README.md
git commit -m "feat(core): select headerless table columns" \
  -m "Assisted-by: Codex:gpt-5.6-sol"
```

---

### Task 4: Add the pinned devguide translation project

**Files:**
- Modify: `.gitmodules`
- Create: `projects/devguide/upstream` (gitlink)
- Create: `projects/devguide/.gitignore`
- Create: `projects/devguide/README.md`
- Create: `projects/devguide/glossary.toml`
- Create: `projects/devguide/yeokja.toml`
- Create: `projects/devguide/scripts/prepare.py`
- Create: `projects/devguide/scripts/test_prepare.py`

**Interfaces:**
- Consumes: RST/list/CSV support from Tasks 1–2 and headerless selection from Task 3.
- Produces: `target/debug/yeokja -C projects/devguide status upstream`, `translate upstream`, and `build html` workflows.
- `prepare.py` exposes `prepare(conf_path: Path) -> None` and a CLI accepting exactly one `conf.py` path.

- [ ] **Step 1: Add and pin the full-history upstream submodule**

```bash
git submodule add https://github.com/python/devguide.git projects/devguide/upstream
git -C projects/devguide/upstream checkout 261dc2116ca81985c5c0cfc59db5a251d2c8db96
git -C projects/devguide/upstream rev-parse HEAD
```

Expected final output: `261dc2116ca81985c5c0cfc59db5a251d2c8db96`. Do not add `shallow = true` to `.gitmodules`.

- [ ] **Step 2: Write failing preparation tests**

`test_prepare.py` must use `tempfile.TemporaryDirectory` and contain these executable tests:

```python
import tempfile
import unittest
from pathlib import Path

from projects.devguide.scripts.prepare import MANAGED_BLOCK, prepare


class PrepareTests(unittest.TestCase):
    def write_conf(self, root: str, text: str) -> Path:
        path = Path(root, "conf.py")
        path.write_text(text, encoding="utf-8")
        return path

    def test_appends_korean_settings_and_exact_nitpick(self):
        with tempfile.TemporaryDirectory() as root:
            path = self.write_conf(
                root,
                'project = "Python Developer\'s Guide"\nhtml_title = ""\n',
            )
            prepare(path)
            text = path.read_text(encoding="utf-8")
            self.assertEqual(text.count(MANAGED_BLOCK), 1)
            self.assertIn('language = "ko"', text)
            self.assertIn('("rst:role", "py:func")', text)

    def test_second_run_is_idempotent(self):
        with tempfile.TemporaryDirectory() as root:
            path = self.write_conf(
                root,
                'project = "Python Developer\'s Guide"\nhtml_title = ""\n',
            )
            prepare(path)
            first = path.read_bytes()
            prepare(path)
            self.assertEqual(path.read_bytes(), first)

    def test_existing_unmanaged_language_setting_fails(self):
        with tempfile.TemporaryDirectory() as root:
            path = self.write_conf(
                root,
                'project = "Python Developer\'s Guide"\nhtml_title = ""\nlanguage = "ja"\n',
            )
            with self.assertRaisesRegex(ValueError, "unmanaged language"):
                prepare(path)

    def test_missing_conf_fails(self):
        with tempfile.TemporaryDirectory() as root:
            with self.assertRaises(FileNotFoundError):
                prepare(Path(root, "conf.py"))
```

The first test starts with `project = "Python Developer's Guide"\nhtml_title = ""\n` and asserts the prepared file contains exactly once:

```python
# BEGIN YEOKJA KOREAN CONFIG
language = "ko"
html_title = "Python 개발자 가이드 (비공식 한국어 번역)"
nitpick_ignore = [*globals().get("nitpick_ignore", []), ("rst:role", "py:func")]
# END YEOKJA KOREAN CONFIG
```

- [ ] **Step 3: Run the test to verify it fails**

Run: `python3 -m unittest projects/devguide/scripts/test_prepare.py -v`

Expected: import/file failure because `prepare.py` does not exist.

- [ ] **Step 4: Implement deterministic Sphinx preparation**

Implement `prepare(conf_path: Path) -> None` with these rules:

- fail with `FileNotFoundError` if `conf.py` is missing;
- return without changes when both managed markers occur exactly once;
- fail with `ValueError` if only one marker occurs or if an unmanaged line matches `^\s*language\s*=`;
- require the pinned upstream anchors `project = "Python Developer's Guide"` and `html_title = ""` before appending the exact managed block above;
- write UTF-8 with the original content followed by one blank line and the managed block.

The CLI must print the exception and exit nonzero rather than partially writing a file.

- [ ] **Step 5: Create project configuration and glossary**

Create `yeokja.toml` with this core configuration:

```toml
[project]
source_lang = "en"
target_lang = "ko"
glossary = "glossary.toml"
state_dir = "state"

[[sources]]
path = "upstream"
pattern = "**/*.rst"
parser = "rst"
output = "ko/{path}"

[derive]
base = "upstream"

[[derive.overlay]]
path = "ko"
require_base = true

[[derive.step]]
kind = "generate"
command = '''python3 "$YEOKJA_ROOT/scripts/prepare.py" conf.py'''

[build.html]
command = '''
set -e
make html SPHINXOPTS='--fail-on-warning --keep-going'
mv _build/html site
'''
outputs = ["site"]

[provider]
type = "pi"
model = "gpt-5.6-sol"
base_url = "openai-codex"

[evaluation]
auto_evaluate = true
style_evaluate = false
max_retries = 3

[translation]
concurrency = 18
batch_segments = 32
```

Add these narrowly scoped rules after `[translation]`:

```toml
[[tables]]
files = "upstream/internals.rst"
headers = ["Title", "Brief", "Author", "Version"]
translate = ["Title", "Brief"]

[[tables]]
files = "upstream/core-team/experts.rst"
headers = ["Module", "Maintainers"]
skip = ["Module", "Maintainers"]

[[tables]]
files = "upstream/core-team/experts.rst"
headers = ["Tool", "Maintainers"]
skip = ["Tool", "Maintainers"]

[[tables]]
files = "upstream/core-team/experts.rst"
headers = ["Interest area", "Maintainers"]
translate = ["Interest area"]

[[tables]]
files = "upstream/core-team/experts.rst"
headers = []
headerless = true
skip = [0, 1]

[[tables]]
files = "upstream/core-team/join-team.rst"
headers = ["Service", "Add to", "Remove from", "Contact"]
skip = [0, 1, 2, 3]

[[tables]]
files = "upstream/developer-workflow/development-cycle.rst"
headers = ["Name", "Role", "GitHub Username"]
translate = ["Role"]

[[tables]]
files = "upstream/developer-workflow/development-cycle.rst"
headers = ["Name", "PEP", "Contact repo"]
translate = ["Name"]

[[tables]]
files = "upstream/developer-workflow/porting.rst"
headers = ["Platform", "Maintainers", "Information"]
translate = ["Information"]

[[tables]]
files = "upstream/developer-workflow/extension-modules.rst"
headers = ["Enabled", "Supported", "Status"]
translate = ["Status"]

[[tables]]
files = "upstream/development-tools/clinic/howto.rst"
headers = []
headerless = true
skip = [0, 1]

[[tables]]
files = "upstream/documentation/markup.rst"
headers = ["Element", "Markup", "See also"]
translate = ["Element"]

[[tables]]
files = "upstream/documentation/markup.rst"
headers = ["reStructuredText", "Rendered"]
skip = [0, 1]

[[tables]]
files = "upstream/documentation/style-guide.rst"
headers = ["Avoid", "Instead"]
translate = ["Avoid", "Instead"]

[[tables]]
files = "upstream/documentation/translations/translating.rst"
headers = ["Language", "Coordination team", "Links"]
translate = ["Language"]

[[tables]]
files = "upstream/index.rst"
headers = ["Documentation", "Code", "Triage"]
skip = [0, 1, 2]

[[tables]]
files = "upstream/testing/new-buildbot-worker.rst"
headers = ["Port", "Host", "Description"]
translate = ["Description"]
```

Create the glossary from the already reviewed Python terminology and append the
devguide-specific mappings:

```bash
cp projects/peps/glossary.toml projects/devguide/glossary.toml
```

Append these fixed mappings:

```toml
[terms."backport"]
translation = "백포트"
[terms."buildbot"]
translation = "빌드봇"
[terms."core developer"]
translation = "코어 개발자"
[terms."core team"]
translation = "코어 팀"
[terms."deprecation"]
translation = "지원 중단 예정"
[terms."issue tracker"]
translation = "이슈 추적기"
[terms."pull request"]
translation = "풀 리퀘스트"
[terms."regression"]
translation = "회귀"
[terms."release manager"]
translation = "릴리스 관리자"
[terms."steering council"]
translation = "운영 위원회"
[terms."triage"]
translation = "트리아지"
[terms."working tree"]
translation = "작업 트리"
```

- [ ] **Step 6: Add ignore rules and project documentation**

`.gitignore` must contain:

```gitignore
/build/
/dist/
/ko/
/venv/
/.venv/
__pycache__/
```

`README.md` must state the 64-file scope, pinned commit, CC0-1.0 source license, unofficial Codex translation notice, directory responsibilities, exact status/translate/evaluate/build/audit commands, full-history requirement, and upstream update procedure.

- [ ] **Step 7: Verify preparation, config discovery, and table selection**

```bash
python3 -m unittest projects/devguide/scripts/test_prepare.py -v
cargo build -p yeokja-cli
./target/debug/yeokja -C projects/devguide status upstream
./target/debug/yeokja -C projects/devguide inspect upstream
./target/debug/yeokja -C projects/devguide coverage upstream --min-lines 3
```

Expected:

- unit tests pass;
- status reports exactly 64 files and more than the pre-extension baseline of 5,428 segments;
- inspect reports matching rules for all inline CSV, list, simple, and grid tables;
- coverage contains only code, raw HTML, comments, references, external CSV files, and other intentional structure—not inline list/CSV table prose.

- [ ] **Step 8: Commit the project recipe**

```bash
git add .gitmodules projects/devguide
git commit -m "feat: add Python devguide translation project" \
  -m "Assisted-by: Codex:gpt-5.6-sol"
```

---

### Task 5: Add deterministic translation and HTML audits

**Files:**
- Create: `projects/devguide/scripts/audit.py`
- Create: `projects/devguide/scripts/test_audit.py`
- Modify: `projects/devguide/README.md`

**Interfaces:**
- Produces CLI modes `python3 scripts/audit.py translation` and `python3 scripts/audit.py html`.
- `translation` compares `upstream/**/*.rst` with `state/upstream/**/*.rst.yeokja.json` and `ko/**/*.rst`, then validates every state segment.
- `html` parses `dist/site/**/*.html`, resolves local `href` paths and fragments, and exits nonzero with sorted diagnostics.

- [ ] **Step 1: Write failing audit tests**

Use temporary trees with the exact interfaces
`audit_translation(source_root: Path, state_root: Path, output_root: Path) -> list[str]`
and `audit_html(site_root: Path) -> list[str]`. The test module must contain:

```python
import json
import tempfile
import unittest
from pathlib import Path

from projects.devguide.scripts.audit import audit_html, audit_translation


class TranslationAuditTests(unittest.TestCase):
    def make_tree(self, root: str) -> tuple[Path, Path, Path]:
        base = Path(root)
        source = base / "upstream"
        state = base / "state" / "upstream"
        output = base / "ko"
        for path in (source, state, output):
            path.mkdir(parents=True)
        return source, state, output

    def write_complete(self, source: Path, state: Path, output: Path) -> None:
        (source / "index.rst").write_text("Hello.\n", encoding="utf-8")
        (output / "index.rst").write_text("안녕하세요.\n", encoding="utf-8")
        payload = {
            "version": 1,
            "source_hash": 1,
            "segments": [{
                "id": "section:0/block:0/seg:0",
                "source": "Hello.",
                "source_hash": 1,
                "context_hash": 1,
                "translation": "안녕하세요.",
                "glossary_snapshot": {},
                "translated_at": "2026-08-30T00:00:00Z",
                "issues": [],
            }],
        }
        (state / "index.rst.yeokja.json").write_text(
            json.dumps(payload), encoding="utf-8"
        )

    def test_complete_tree_has_no_errors(self):
        with tempfile.TemporaryDirectory() as root:
            source, state, output = self.make_tree(root)
            self.write_complete(source, state, output)
            self.assertEqual(audit_translation(source, state, output), [])

    def test_reports_missing_state_and_output(self):
        with tempfile.TemporaryDirectory() as root:
            source, state, output = self.make_tree(root)
            (source / "index.rst").write_text("Hello.\n", encoding="utf-8")
            errors = audit_translation(source, state, output)
            self.assertIn("missing state: index.rst.yeokja.json", errors)
            self.assertIn("missing output: index.rst", errors)

    def test_reports_null_translation_and_issues(self):
        with tempfile.TemporaryDirectory() as root:
            source, state, output = self.make_tree(root)
            self.write_complete(source, state, output)
            path = state / "index.rst.yeokja.json"
            payload = json.loads(path.read_text(encoding="utf-8"))
            payload["segments"][0]["translation"] = None
            payload["segments"][0]["issues"] = [{"kind": "format"}]
            path.write_text(json.dumps(payload), encoding="utf-8")
            errors = audit_translation(source, state, output)
            self.assertTrue(any("missing translation" in error for error in errors))
            self.assertTrue(any("unresolved issues" in error for error in errors))

    def test_reports_orphan_state(self):
        with tempfile.TemporaryDirectory() as root:
            source, state, output = self.make_tree(root)
            self.write_complete(source, state, output)
            (state / "orphan.rst.yeokja.json").write_text(
                '{"version": 1, "source_hash": 1, "segments": []}',
                encoding="utf-8",
            )
            self.assertIn(
                "orphan state: orphan.rst.yeokja.json",
                audit_translation(source, state, output),
            )


class HtmlAuditTests(unittest.TestCase):
    def test_accepts_supported_local_and_external_links(self):
        with tempfile.TemporaryDirectory() as root:
            site = Path(root)
            (site / "guide").mkdir()
            (site / "_static").mkdir()
            (site / "_static" / "app.css").write_text("", encoding="utf-8")
            (site / "guide" / "index.html").write_text(
                '<h1 id="target">Guide</h1>', encoding="utf-8"
            )
            (site / "index.html").write_text(
                '<h1 id="intro">Index</h1>'
                '<a href="guide/">guide</a>'
                '<a href="guide/#target">target</a>'
                '<a href="#intro">intro</a>'
                '<a href="/_static/app.css">css</a>'
                '<a href="https://example.com/">external</a>'
                '<a href="mailto:docs@example.com">mail</a>',
                encoding="utf-8",
            )
            self.assertEqual(audit_html(site), [])

    def test_reports_missing_file_and_fragment(self):
        with tempfile.TemporaryDirectory() as root:
            site = Path(root)
            (site / "index.html").write_text(
                '<a href="missing.html">missing</a>'
                '<a href="#absent">fragment</a>',
                encoding="utf-8",
            )
            errors = audit_html(site)
            self.assertTrue(any("missing target" in error for error in errors))
            self.assertTrue(any("missing fragment" in error for error in errors))
```

- [ ] **Step 2: Run tests and verify missing implementation**

Run: `python3 -m unittest projects/devguide/scripts/test_audit.py -v`

Expected: import/file failure because `audit.py` does not exist.

- [ ] **Step 3: Implement translation-state auditing**

Use only Python standard-library modules. Map `upstream/<relative>.rst` to:

- `state/upstream/<relative>.rst.yeokja.json`;
- `ko/<relative>.rst`.

For every state JSON, require `version == 1`, a list-valued `segments`, a nonempty string `translation` for every segment, and an empty `issues` list. Sort all diagnostics by path and segment ID. Do not require a positive segment count because raw/include-only RST files legitimately contain no translatable prose.

- [ ] **Step 4: Implement local HTML path and anchor auditing**

Subclass `html.parser.HTMLParser` to collect every `id`, `name`, and `href`. Resolve links with `urllib.parse.urlsplit` and `unquote`:

- skip nonempty schemes or netlocs and `mailto:`;
- resolve `/path` from `site_root` and relative paths from the referring file;
- map a directory to `index.html`;
- require non-HTML assets to exist;
- for an HTML fragment, require the decoded fragment in the target page's collected IDs/names.

Reject any resolved path that escapes `site_root` after `Path.resolve()`.

- [ ] **Step 5: Run tests and document audit commands**

```bash
python3 -m unittest projects/devguide/scripts/test_audit.py -v
python3 -m unittest discover -s projects/devguide/scripts -p 'test_*.py' -v
```

Expected: all tests pass. Add both audit commands to the project README.

- [ ] **Step 6: Commit audits**

```bash
git add projects/devguide/scripts/audit.py projects/devguide/scripts/test_audit.py projects/devguide/README.md
git commit -m "test(devguide): audit translation and HTML completeness" \
  -m "Assisted-by: Codex:gpt-5.6-sol"
```

---

### Task 6: Translate all devguide RST with Codex

**Files:**
- Create: `projects/devguide/state/**/*.yeokja.json`
- Derived and ignored: `projects/devguide/ko/**/*.rst`

**Interfaces:**
- Consumes: the Task 4 Codex provider and table rules.
- Produces: complete authoritative state and a mirrored Korean RST tree.

- [ ] **Step 1: Record the clean pre-translation baseline**

```bash
./target/debug/yeokja -C projects/devguide status upstream
./target/debug/yeokja -C projects/devguide status upstream --check
```

Expected: the first command reports 64 files and pending segments; the `--check` command exits nonzero.

- [ ] **Step 2: Run the complete Codex translation**

```bash
./target/debug/yeokja -C projects/devguide translate upstream
```

Let the command finish or resume it after transient failures; never delete successful state to restart. The provider in the command path must log/use `pi`, `openai-codex`, and `gpt-5.6-sol` from the committed configuration.

- [ ] **Step 3: Verify status and mechanical evaluation**

```bash
./target/debug/yeokja -C projects/devguide status upstream --check
./target/debug/yeokja -C projects/devguide evaluate upstream --mechanical-only
./target/debug/yeokja -C projects/devguide orphans
```

Expected: zero pending/stale/glossary-stale/context-changed/failed segments, all deterministic evaluations pass, and no orphan state is reported.

- [ ] **Step 4: Run source/state/output completeness audit**

```bash
python3 projects/devguide/scripts/audit.py translation
```

Expected: exit 0 with 64 source files, 64 state files, 64 Korean outputs, zero incomplete segments, and zero orphans.

- [ ] **Step 5: Inspect high-risk translations**

Inspect state and reconstructed output for these exact high-risk areas:

- `internals.rst` inline CSV titles/briefs;
- `documentation/translations/translating.rst` coordinator list table;
- `developer-workflow/porting.rst` link-heavy information cells;
- `core-team/experts.rst` identifiers and names;
- `development-tools/clinic/howto.rst` headerless code table;
- `getting-started/setup-building.rst` shell commands and platform tabs;
- `documentation/markup.rst` literal RST examples.

Correct translation text only in `state/`, rerun `translate upstream` to regenerate `ko/`, and rerun Steps 3–4 after every correction batch.

- [ ] **Step 6: Commit the authoritative translations**

```bash
git add projects/devguide/state
git commit -m "docs(devguide): translate guide into Korean with Codex" \
  -m "Assisted-by: Codex:gpt-5.6-sol"
```

Confirm `git show --stat HEAD` includes only `state/` files and no ignored `ko/`, build, dist, or virtual-environment content.

---

### Task 7: Build the Korean site and fix translation-induced RST failures

**Files:**
- Modify on a Sphinx file/line diagnostic: the corresponding `projects/devguide/state/**/*.yeokja.json`
- Derived and ignored: `projects/devguide/build/tree`, `projects/devguide/dist/site`

**Interfaces:**
- Consumes: complete state, `prepare.py`, upstream Sphinx requirements, and yeokja assembly.
- Produces: `projects/devguide/dist/site/index.html` with no Sphinx warnings.

- [ ] **Step 1: Verify the full-history submodule before building**

```bash
test "$(git -C projects/devguide/upstream rev-parse HEAD)" = 261dc2116ca81985c5c0cfc59db5a251d2c8db96
test "$(git -C projects/devguide/upstream rev-list --count HEAD)" -gt 1
```

Expected: both commands exit 0. A count of 1 proves a shallow checkout and must be fixed before building.

- [ ] **Step 2: Assemble and inspect the Korean Sphinx configuration**

```bash
./target/debug/yeokja -C projects/devguide assemble
rg -n 'BEGIN YEOKJA KOREAN CONFIG|language = "ko"|html_title|nitpick_ignore' projects/devguide/build/tree/conf.py
```

Expected: each managed setting occurs exactly once and the exact nitpick pair is present.

- [ ] **Step 3: Run warning-as-error HTML build**

```bash
./target/debug/yeokja -C projects/devguide build html
```

Expected: Sphinx exits 0 with no warnings and `projects/devguide/dist/site/index.html` exists.

- [ ] **Step 4: Repair any translation-induced failure at its authoritative state**

For each Sphinx file/line diagnostic, locate the owning source segment in its mirrored state JSON, correct only its `translation`, and run:

```bash
./target/debug/yeokja -C projects/devguide translate upstream
./target/debug/yeokja -C projects/devguide evaluate upstream --mechanical-only
./target/debug/yeokja -C projects/devguide build html
```

Do not suppress new warning classes. The only configured exception remains `("rst:role", "py:func")`.

- [ ] **Step 5: Audit the built HTML and Korean rendering**

```bash
python3 projects/devguide/scripts/audit.py html
rg -n 'Python 개발자 가이드|시작|기여|개발' projects/devguide/dist/site/index.html
```

Expected: the HTML audit exits 0 and the index contains Korean reader-facing text.

- [ ] **Step 6: Commit build-driven translation corrections**

If Step 4 changed state:

```bash
git add projects/devguide/state
git commit -m "fix(devguide): repair Korean RST rendering" \
  -m "Assisted-by: Codex:gpt-5.6-sol"
```

If no state changed, do not create an empty commit.

---

### Task 8: Run the completion audit and record reproducible evidence

**Files:**
- Modify if commands changed during execution: `projects/devguide/README.md`

**Interfaces:**
- Consumes every artifact and command from Tasks 1–7.
- Produces evidence for every completion condition in the design spec.

- [ ] **Step 1: Run all software and script tests fresh**

```bash
cargo fmt --all -- --check
cargo test --workspace
cargo clippy --workspace -- -D warnings
python3 -m unittest discover -s projects/devguide/scripts -p 'test_*.py' -v
```

Expected: all commands exit 0 with no test failure or lint warning.

- [ ] **Step 2: Run all translation gates fresh**

```bash
./target/debug/yeokja -C projects/devguide status upstream --check
./target/debug/yeokja -C projects/devguide evaluate upstream --mechanical-only
./target/debug/yeokja -C projects/devguide orphans
python3 projects/devguide/scripts/audit.py translation
```

Expected: every segment translated/evaluated, zero stale/failed/orphan state, and exact 64-way source/state/output correspondence.

- [ ] **Step 3: Run build and HTML gates fresh**

```bash
./target/debug/yeokja -C projects/devguide build html
python3 projects/devguide/scripts/audit.py html
test -s projects/devguide/dist/site/index.html
```

Expected: all commands exit 0 and the site index is nonempty.

- [ ] **Step 4: Verify repository hygiene and commit trailers**

```bash
git status --short --branch --untracked-files=all
git diff --check
git submodule status projects/devguide/upstream
```

Expected: no source modifications inside `projects/devguide/upstream`, no generated `ko/build/dist/venv` paths, and the gitlink points at the pinned commit. Verify each implementation commit with:

```bash
for commit in $(git rev-list --reverse 78376f1..HEAD); do
  test "$(git show -s --format='%B' "$commit" | rg -c '^Assisted-by: Codex:gpt-5\.6-sol$')" -eq 1
done
```

- [ ] **Step 5: Commit documentation corrections if needed**

If real commands or prerequisites differed from the README, update it and commit:

```bash
git add projects/devguide/README.md
git commit -m "docs(devguide): finalize translation workflow" \
  -m "Assisted-by: Codex:gpt-5.6-sol"
```

Rerun `git diff --check` and `git status` after this commit. Do not mark the goal complete until the complete evidence from Steps 1–4 is fresh and green.
