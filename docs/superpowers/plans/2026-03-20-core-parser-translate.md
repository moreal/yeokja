# Core + Parser-Markdown + Translate Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the foundational library layer — data models, Markdown parsing into segments, glossary management, change detection with reconciliation, LLM translation with rate limiting, and state file persistence.

**Architecture:** Cargo workspace with three crates: `yeokja-core` (models, traits, glossary, change detection, state file I/O), `yeokja-parser-markdown` (Markdown → Document parser using pulldown-cmark + sentence splitting), `yeokja-translate` (TranslationProvider trait + OpenAI-compatible provider as first implementation). All async via tokio.

**Tech Stack:** Rust, serde/serde_json, xxhash-rust, pulldown-cmark, unicode-segmentation, reqwest, tokio, toml

**Spec:** `docs/superpowers/specs/2026-03-20-yeokja-design.md`

---

## File Structure

```
Cargo.toml                                    # workspace definition
crates/
  core/
    Cargo.toml                                # yeokja-core
    src/
      lib.rs                                  # re-exports
      model.rs                                # Document, Section, Block, Segment, SegmentId, BlockType
      parser.rs                               # DocumentParser trait, TranslationMap type alias
      glossary.rs                             # Glossary struct, loading from TOML, term matching
      state.rs                                # StateFile struct, load/save (atomic write), SegmentState
      reconcile.rs                            # reconcile() — match old state to new segments
      change.rs                               # SegmentStatus enum, compute_status() per segment
      hash.rs                                 # xxhash helpers: content_hash, context_hash
      config.rs                               # Project config (yeokja.toml) deserialization
  parser-markdown/
    Cargo.toml                                # yeokja-parser-markdown
    src/
      lib.rs                                  # MarkdownParser implementing DocumentParser
      sentence.rs                             # sentence splitting logic
  translate/
    Cargo.toml                                # yeokja-translate
    src/
      lib.rs                                  # re-exports
      provider.rs                             # TranslationProvider trait, TranslateRequest, TranslateResponse
      prompt.rs                               # prompt building and response parsing ([1], [2] format)
      rate_limit.rs                           # RateLimiter — adaptive rate control
      openai_compatible.rs                    # OpenAICompatibleProvider implementation
```

---

### Task 1: Workspace and Crate Scaffolding

**Files:**
- Create: `Cargo.toml` (workspace)
- Create: `crates/core/Cargo.toml`
- Create: `crates/core/src/lib.rs`
- Create: `crates/parser-markdown/Cargo.toml`
- Create: `crates/parser-markdown/src/lib.rs`
- Create: `crates/translate/Cargo.toml`
- Create: `crates/translate/src/lib.rs`

- [ ] **Step 1: Create workspace Cargo.toml**

```toml
[workspace]
resolver = "2"
members = [
    "crates/core",
    "crates/parser-markdown",
    "crates/translate",
]
```

- [ ] **Step 2: Create crates/core/Cargo.toml**

```toml
[package]
name = "yeokja-core"
version = "0.1.0"
edition = "2024"

[dependencies]
serde = { version = "1", features = ["derive"] }
serde_json = "1"
xxhash-rust = { version = "0.8", features = ["xxh64"] }
toml = "0.8"
chrono = { version = "0.4", features = ["serde"] }
thiserror = "2"
```

- [ ] **Step 3: Create crates/core/src/lib.rs**

```rust
pub mod model;
pub mod parser;
pub mod hash;
pub mod glossary;
pub mod state;
pub mod reconcile;
pub mod change;
pub mod config;
```

- [ ] **Step 4: Create crates/parser-markdown/Cargo.toml**

```toml
[package]
name = "yeokja-parser-markdown"
version = "0.1.0"
edition = "2024"

[dependencies]
yeokja-core = { path = "../core" }
pulldown-cmark = "0.12"
unicode-segmentation = "1"
```

- [ ] **Step 5: Create crates/parser-markdown/src/lib.rs** (empty struct placeholder)

```rust
pub struct MarkdownParser;
```

- [ ] **Step 6: Create crates/translate/Cargo.toml**

```toml
[package]
name = "yeokja-translate"
version = "0.1.0"
edition = "2024"

[dependencies]
yeokja-core = { path = "../core" }
async-trait = "0.1"
reqwest = { version = "0.12", features = ["json"] }
tokio = { version = "1", features = ["full"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
thiserror = "2"
```

- [ ] **Step 7: Create crates/translate/src/lib.rs** (empty placeholder)

```rust
pub mod provider;
pub mod prompt;
pub mod rate_limit;
pub mod openai_compatible;
```

- [ ] **Step 8: Verify workspace compiles**

Run: `cargo check`
Expected: compiles with no errors

- [ ] **Step 9: Commit**

```bash
git add Cargo.toml crates/
git commit -m "feat: scaffold cargo workspace with core, parser-markdown, translate crates"
```

---

### Task 2: Core Data Models

**Files:**
- Create: `crates/core/src/model.rs`
- Create: `crates/core/src/hash.rs`
- Create: `crates/core/src/parser.rs`
- Test: `crates/core/src/hash.rs` (inline tests)

- [ ] **Step 1: Write hash module with tests**

```rust
// crates/core/src/hash.rs
use xxhash_rust::xxh64::xxh64;

const SEED: u64 = 0;
const SENTINEL: u64 = 0;

pub fn content_hash(content: &str) -> u64 {
    xxh64(content.as_bytes(), SEED)
}

pub fn context_hash(prev_source_hash: Option<u64>, next_source_hash: Option<u64>) -> u64 {
    let prev = prev_source_hash.unwrap_or(SENTINEL);
    let next = next_source_hash.unwrap_or(SENTINEL);
    let mut buf = [0u8; 16];
    buf[..8].copy_from_slice(&prev.to_le_bytes());
    buf[8..].copy_from_slice(&next.to_le_bytes());
    xxh64(&buf, SEED)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn content_hash_deterministic() {
        let h1 = content_hash("hello world");
        let h2 = content_hash("hello world");
        assert_eq!(h1, h2);
    }

    #[test]
    fn content_hash_differs_for_different_input() {
        let h1 = content_hash("hello");
        let h2 = content_hash("world");
        assert_ne!(h1, h2);
    }

    #[test]
    fn context_hash_uses_sentinel_at_boundaries() {
        let h_start = context_hash(None, Some(42));
        let h_end = context_hash(Some(42), None);
        let h_mid = context_hash(Some(1), Some(42));
        assert_ne!(h_start, h_end);
        assert_ne!(h_start, h_mid);
    }

    #[test]
    fn context_hash_deterministic() {
        let h1 = context_hash(Some(1), Some(2));
        let h2 = context_hash(Some(1), Some(2));
        assert_eq!(h1, h2);
    }
}
```

- [ ] **Step 2: Run hash tests**

Run: `cargo test -p yeokja-core hash`
Expected: 4 tests pass

- [ ] **Step 3: Write model module**

```rust
// crates/core/src/model.rs
use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SegmentId(pub String);

impl SegmentId {
    pub fn new(section: usize, block: usize, seg: usize) -> Self {
        Self(format!("section:{section}/block:{block}/seg:{seg}"))
    }

    /// Extract the ordinal position as (section, block, seg) for distance calculation.
    pub fn position(&self) -> Option<(usize, usize, usize)> {
        let parts: Vec<&str> = self.0.split('/').collect();
        if parts.len() != 3 {
            return None;
        }
        let section = parts[0].strip_prefix("section:")?.parse().ok()?;
        let block = parts[1].strip_prefix("block:")?.parse().ok()?;
        let seg = parts[2].strip_prefix("seg:")?.parse().ok()?;
        Some((section, block, seg))
    }

    /// Flat ordinal index for distance comparison.
    /// Uses a large multiplier to avoid collisions between sections/blocks.
    pub fn flat_index(&self) -> usize {
        self.position()
            .map(|(s, b, seg)| s * 1_000_000 + b * 1_000 + seg)
            .unwrap_or(0)
    }
}

impl fmt::Display for SegmentId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BlockType {
    Heading,
    Paragraph,
    ListItem,
    CodeBlock,
    BlockQuote,
    ThematicBreak,
    Table,
    HtmlBlock,
}

impl BlockType {
    pub fn is_translatable(&self) -> bool {
        !matches!(self, BlockType::CodeBlock | BlockType::ThematicBreak | BlockType::HtmlBlock)
    }
}

#[derive(Debug, Clone)]
pub struct Segment {
    pub id: SegmentId,
    pub source: String,
    pub source_hash: u64,
    pub block_type: BlockType,
}

#[derive(Debug, Clone)]
pub struct Block {
    pub block_type: BlockType,
    pub segments: Vec<Segment>,
    pub raw_content: String,
    /// Heading level (1-6), only set when block_type is Heading.
    pub heading_level: Option<u8>,
}

#[derive(Debug, Clone)]
pub struct Section {
    pub blocks: Vec<Block>,
}

#[derive(Debug, Clone)]
pub struct Document {
    pub sections: Vec<Section>,
}

impl Document {
    pub fn all_segments(&self) -> Vec<&Segment> {
        self.sections
            .iter()
            .flat_map(|s| s.blocks.iter())
            .flat_map(|b| b.segments.iter())
            .collect()
    }

    pub fn translatable_segments(&self) -> Vec<&Segment> {
        self.all_segments()
            .into_iter()
            .filter(|s| s.block_type.is_translatable())
            .collect()
    }
}
```

- [ ] **Step 4: Write parser trait module**

```rust
// crates/core/src/parser.rs
use crate::model::{Document, SegmentId};
use std::collections::HashMap;

pub type TranslationMap = HashMap<SegmentId, String>;

pub trait DocumentParser {
    /// Parse source text into a structured Document.
    fn parse(&self, source: &str) -> Document;

    /// Reconstruct the document with translations substituted.
    /// Missing translations fall back to original source text.
    fn reconstruct(&self, document: &Document, translations: &TranslationMap) -> String;
}
```

- [ ] **Step 5: Verify compilation**

Run: `cargo check -p yeokja-core`
Expected: compiles

- [ ] **Step 6: Commit**

```bash
git add crates/core/src/
git commit -m "feat(core): add data models, hash functions, parser trait"
```

---

### Task 3: Glossary

**Files:**
- Create: `crates/core/src/glossary.rs`
- Test: inline tests in `glossary.rs`

- [ ] **Step 1: Write failing tests**

```rust
// crates/core/src/glossary.rs
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GlossaryEntry {
    pub translation: String,
    #[serde(default)]
    pub note: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GlossaryFile {
    #[serde(default)]
    pub terms: HashMap<String, GlossaryEntry>,
}

#[derive(Debug, Clone)]
pub struct Glossary {
    terms: HashMap<String, String>,
}

impl Glossary {
    pub fn load(path: &Path) -> Result<Self, GlossaryError> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| GlossaryError::Io(e))?;
        Self::from_toml(&content)
    }

    pub fn from_toml(content: &str) -> Result<Self, GlossaryError> {
        let file: GlossaryFile = toml::from_str(content)
            .map_err(|e| GlossaryError::Parse(e.to_string()))?;
        let terms = file.terms.into_iter()
            .map(|(k, v)| (k, v.translation))
            .collect();
        Ok(Self { terms })
    }

    pub fn empty() -> Self {
        Self { terms: HashMap::new() }
    }

    pub fn terms(&self) -> &HashMap<String, String> {
        &self.terms
    }

    /// Find glossary terms that appear in the given text (word boundary match).
    /// Uses char-based boundary checking to handle non-ASCII (Korean, CJK) text correctly.
    pub fn find_matching_terms(&self, text: &str) -> HashMap<String, String> {
        let text_lower = text.to_lowercase();
        let text_chars: Vec<char> = text_lower.chars().collect();
        self.terms
            .iter()
            .filter(|(term, _)| {
                let term_lower = term.to_lowercase();
                text_lower
                    .match_indices(&term_lower)
                    .any(|(byte_pos, matched)| {
                        // Find char index for boundary check
                        let char_pos = text_lower[..byte_pos].chars().count();
                        let char_end = char_pos + matched.chars().count();
                        let before_ok = char_pos == 0
                            || !text_chars[char_pos - 1].is_alphanumeric();
                        let after_ok = char_end >= text_chars.len()
                            || !text_chars[char_end].is_alphanumeric();
                        before_ok && after_ok
                    })
            })
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect()
    }

    /// Check if a glossary snapshot is stale compared to current glossary.
    pub fn is_snapshot_stale(&self, snapshot: &HashMap<String, String>, source_text: &str) -> bool {
        let current_matches = self.find_matching_terms(source_text);
        // Check if any term in current matches differs from snapshot
        for (term, translation) in &current_matches {
            match snapshot.get(term) {
                Some(snap_translation) if snap_translation == translation => {}
                _ => return true, // new term or changed translation
            }
        }
        false
    }
}

#[derive(Debug, thiserror::Error)]
pub enum GlossaryError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Parse error: {0}")]
    Parse(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_glossary() -> Glossary {
        let toml = r#"
[terms.repository]
translation = "저장소"
note = "Git 저장소"

[terms.commit]
translation = "커밋"
"#;
        Glossary::from_toml(toml).unwrap()
    }

    #[test]
    fn load_glossary_from_toml() {
        let g = test_glossary();
        assert_eq!(g.terms().get("repository").unwrap(), "저장소");
        assert_eq!(g.terms().get("commit").unwrap(), "커밋");
    }

    #[test]
    fn find_matching_terms_word_boundary() {
        let g = test_glossary();
        let matches = g.find_matching_terms("The repository stores commits.");
        assert_eq!(matches.get("repository").unwrap(), "저장소");
        assert_eq!(matches.get("commit"), None); // "commits" != "commit" at boundary
    }

    #[test]
    fn find_matching_terms_no_partial() {
        let g = test_glossary();
        let matches = g.find_matching_terms("repositoryName is set");
        assert!(matches.is_empty()); // "repositoryName" fails boundary check
    }

    #[test]
    fn snapshot_not_stale_when_matching() {
        let g = test_glossary();
        let mut snapshot = HashMap::new();
        snapshot.insert("repository".to_string(), "저장소".to_string());
        assert!(!g.is_snapshot_stale(&snapshot, "The repository is ready."));
    }

    #[test]
    fn snapshot_stale_when_translation_changed() {
        let g = test_glossary();
        let mut snapshot = HashMap::new();
        snapshot.insert("repository".to_string(), "레포지토리".to_string());
        assert!(g.is_snapshot_stale(&snapshot, "The repository is ready."));
    }

    #[test]
    fn snapshot_stale_when_new_term_added() {
        let g = test_glossary();
        let snapshot = HashMap::new(); // no terms recorded
        assert!(g.is_snapshot_stale(&snapshot, "The repository is ready."));
    }

    #[test]
    fn snapshot_not_stale_when_term_not_in_text() {
        let g = test_glossary();
        let snapshot = HashMap::new();
        assert!(!g.is_snapshot_stale(&snapshot, "Hello world."));
    }
}
```

- [ ] **Step 2: Run tests**

Run: `cargo test -p yeokja-core glossary`
Expected: 7 tests pass

- [ ] **Step 3: Commit**

```bash
git add crates/core/src/glossary.rs
git commit -m "feat(core): add glossary loading, term matching, snapshot staleness check"
```

---

### Task 4: State File I/O

**Files:**
- Create: `crates/core/src/state.rs`
- Test: inline tests in `state.rs`

- [ ] **Step 1: Write state module with tests**

```rust
// crates/core/src/state.rs
use crate::model::SegmentId;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

const CURRENT_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateFile {
    pub version: u32,
    pub source_hash: u64,
    pub segments: Vec<SegmentState>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SegmentState {
    pub id: SegmentId,
    pub source: String,
    pub source_hash: u64,
    pub context_hash: u64,
    pub translation: Option<String>,
    #[serde(default)]
    pub glossary_snapshot: HashMap<String, String>,
    pub translated_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub issues: Vec<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum StateError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
}

impl StateFile {
    pub fn new(source_hash: u64) -> Self {
        Self {
            version: CURRENT_VERSION,
            source_hash,
            segments: Vec::new(),
        }
    }

    pub fn load(path: &Path) -> Result<Self, StateError> {
        let content = std::fs::read_to_string(path)?;
        let state: Self = serde_json::from_str(&content)?;
        Ok(state)
    }

    /// Atomic write: write to temp file, then rename.
    pub fn save(&self, path: &Path) -> Result<(), StateError> {
        let json = serde_json::to_string_pretty(self)?;
        let tmp_path = path.with_extension("json.tmp");
        std::fs::write(&tmp_path, &json)?;
        std::fs::rename(&tmp_path, path)?;
        Ok(())
    }

    pub fn state_file_path(source_path: &Path) -> std::path::PathBuf {
        let file_name = source_path.file_name().unwrap().to_string_lossy();
        source_path.with_file_name(format!("{file_name}.yeokja.json"))
    }

    pub fn find_by_hash(&self, source_hash: u64) -> Vec<&SegmentState> {
        self.segments.iter().filter(|s| s.source_hash == source_hash).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn state_file_path_generation() {
        let path = Path::new("/book/chapter1.md");
        let state_path = StateFile::state_file_path(path);
        assert_eq!(state_path, Path::new("/book/chapter1.md.yeokja.json"));
    }

    #[test]
    fn save_and_load_roundtrip() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test.md.yeokja.json");

        let mut state = StateFile::new(12345);
        state.segments.push(SegmentState {
            id: SegmentId::new(0, 0, 0),
            source: "Hello.".to_string(),
            source_hash: 111,
            context_hash: 222,
            translation: Some("안녕하세요.".to_string()),
            glossary_snapshot: HashMap::new(),
            translated_at: Some(Utc::now()),
            issues: Vec::new(),
        });

        state.save(&path).unwrap();
        let loaded = StateFile::load(&path).unwrap();

        assert_eq!(loaded.version, 1);
        assert_eq!(loaded.source_hash, 12345);
        assert_eq!(loaded.segments.len(), 1);
        assert_eq!(loaded.segments[0].source, "Hello.");
        assert_eq!(loaded.segments[0].translation.as_deref(), Some("안녕하세요."));
    }

    #[test]
    fn atomic_write_leaves_no_tmp_on_success() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test.md.yeokja.json");
        let tmp_path = path.with_extension("json.tmp");

        let state = StateFile::new(1);
        state.save(&path).unwrap();

        assert!(path.exists());
        assert!(!tmp_path.exists());
    }
}
```

- [ ] **Step 2: Add tempfile dev-dependency to core Cargo.toml**

Add under `[dev-dependencies]`:
```toml
[dev-dependencies]
tempfile = "3"
```

- [ ] **Step 3: Run tests**

Run: `cargo test -p yeokja-core state`
Expected: 3 tests pass

- [ ] **Step 4: Commit**

```bash
git add crates/core/
git commit -m "feat(core): add state file I/O with atomic writes"
```

---

### Task 5: Reconciliation

**Files:**
- Create: `crates/core/src/reconcile.rs`
- Test: inline tests

- [ ] **Step 1: Write reconcile module with tests**

```rust
// crates/core/src/reconcile.rs
use crate::hash::{content_hash, context_hash};
use crate::model::{Document, Segment, SegmentId};
use crate::state::{SegmentState, StateFile};
use std::collections::HashMap;

/// Result of reconciling new parsed segments with existing state.
pub struct ReconcileResult {
    pub segments: Vec<SegmentState>,
}

/// Reconcile new parsed document against existing state file.
/// Uses source_hash as primary key, positional greedy nearest-match as tiebreaker.
pub fn reconcile(document: &Document, existing: &StateFile) -> ReconcileResult {
    let new_segments = document.translatable_segments();

    // Build context hashes for new segments
    let new_with_context: Vec<(&Segment, u64)> = {
        let hashes: Vec<u64> = new_segments.iter().map(|s| s.source_hash).collect();
        new_segments
            .iter()
            .enumerate()
            .map(|(i, seg)| {
                let prev = if i > 0 { Some(hashes[i - 1]) } else { None };
                let next = hashes.get(i + 1).copied();
                let ctx = context_hash(prev, next);
                (*seg, ctx)
            })
            .collect()
    };

    // Group existing segments by source_hash for lookup
    let mut existing_by_hash: HashMap<u64, Vec<&SegmentState>> = HashMap::new();
    for seg in &existing.segments {
        existing_by_hash.entry(seg.source_hash).or_default().push(seg);
    }

    // Track which existing segments have been matched
    let mut matched_existing: std::collections::HashSet<String> = std::collections::HashSet::new();

    let mut result_segments = Vec::new();

    for (new_seg, ctx_hash) in &new_with_context {
        let matched = existing_by_hash
            .get(&new_seg.source_hash)
            .and_then(|candidates| {
                // Greedy nearest-match: find closest unmatched candidate by flat index
                let new_idx = new_seg.id.flat_index();
                candidates
                    .iter()
                    .filter(|c| !matched_existing.contains(&c.id.0))
                    .min_by_key(|c| {
                        let old_idx = c.id.flat_index();
                        (new_idx as isize - old_idx as isize).unsigned_abs()
                    })
            });

        match matched {
            Some(old) => {
                matched_existing.insert(old.id.0.clone());
                result_segments.push(SegmentState {
                    id: new_seg.id.clone(),
                    source: new_seg.source.clone(),
                    source_hash: new_seg.source_hash,
                    context_hash: *ctx_hash,
                    translation: old.translation.clone(),
                    glossary_snapshot: old.glossary_snapshot.clone(),
                    translated_at: old.translated_at,
                    issues: old.issues.clone(),
                });
            }
            None => {
                result_segments.push(SegmentState {
                    id: new_seg.id.clone(),
                    source: new_seg.source.clone(),
                    source_hash: new_seg.source_hash,
                    context_hash: *ctx_hash,
                    translation: None,
                    glossary_snapshot: HashMap::new(),
                    translated_at: None,
                    issues: Vec::new(),
                });
            }
        }
    }

    ReconcileResult { segments: result_segments }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::*;
    use chrono::Utc;

    fn make_segment(section: usize, block: usize, seg: usize, text: &str) -> Segment {
        Segment {
            id: SegmentId::new(section, block, seg),
            source: text.to_string(),
            source_hash: content_hash(text),
            block_type: BlockType::Paragraph,
        }
    }

    fn make_document(segments: Vec<Segment>) -> Document {
        Document {
            sections: vec![Section {
                blocks: vec![Block {
                    block_type: BlockType::Paragraph,
                    segments,
                    raw_content: String::new(),
                    heading_level: None,
                }],
            }],
        }
    }

    fn make_state(entries: Vec<(&str, &str)>) -> StateFile {
        let mut state = StateFile::new(0);
        for (i, (source, translation)) in entries.iter().enumerate() {
            state.segments.push(SegmentState {
                id: SegmentId::new(0, 0, i),
                source: source.to_string(),
                source_hash: content_hash(source),
                context_hash: 0,
                translation: Some(translation.to_string()),
                glossary_snapshot: HashMap::new(),
                translated_at: Some(Utc::now()),
                issues: Vec::new(),
            });
        }
        state
    }

    #[test]
    fn new_segments_get_no_translation() {
        let doc = make_document(vec![
            make_segment(0, 0, 0, "Hello."),
        ]);
        let state = StateFile::new(0);
        let result = reconcile(&doc, &state);

        assert_eq!(result.segments.len(), 1);
        assert!(result.segments[0].translation.is_none());
    }

    #[test]
    fn unchanged_segments_keep_translation() {
        let doc = make_document(vec![
            make_segment(0, 0, 0, "Hello."),
        ]);
        let state = make_state(vec![("Hello.", "안녕하세요.")]);
        let result = reconcile(&doc, &state);

        assert_eq!(result.segments[0].translation.as_deref(), Some("안녕하세요."));
    }

    #[test]
    fn moved_segment_keeps_translation() {
        // "World." was at position 0, now at position 1
        let doc = make_document(vec![
            make_segment(0, 0, 0, "New sentence."),
            make_segment(0, 0, 1, "World."),
        ]);
        let state = make_state(vec![("World.", "세계.")]);
        let result = reconcile(&doc, &state);

        assert_eq!(result.segments.len(), 2);
        assert!(result.segments[0].translation.is_none()); // new
        assert_eq!(result.segments[1].translation.as_deref(), Some("세계.")); // moved
    }

    #[test]
    fn deleted_segments_are_dropped() {
        let doc = make_document(vec![
            make_segment(0, 0, 0, "Hello."),
        ]);
        let state = make_state(vec![("Hello.", "안녕."), ("Goodbye.", "잘가.")]);
        let result = reconcile(&doc, &state);

        assert_eq!(result.segments.len(), 1);
        assert_eq!(result.segments[0].source, "Hello.");
    }

    #[test]
    fn duplicate_segments_matched_by_position() {
        let doc = make_document(vec![
            make_segment(0, 0, 0, "Same."),
            make_segment(0, 0, 1, "Same."),
        ]);
        let state = make_state(vec![("Same.", "같다1."), ("Same.", "같다2.")]);
        let result = reconcile(&doc, &state);

        assert_eq!(result.segments.len(), 2);
        // Both should have translations, matched by nearest position
        assert!(result.segments[0].translation.is_some());
        assert!(result.segments[1].translation.is_some());
        // They should have different translations (greedy match)
        assert_ne!(result.segments[0].translation, result.segments[1].translation);
    }
}
```

- [ ] **Step 2: Run tests**

Run: `cargo test -p yeokja-core reconcile`
Expected: 5 tests pass

- [ ] **Step 3: Commit**

```bash
git add crates/core/src/reconcile.rs
git commit -m "feat(core): add segment reconciliation with greedy nearest-match"
```

---

### Task 6: Change Detection

**Files:**
- Create: `crates/core/src/change.rs`
- Test: inline tests

- [ ] **Step 1: Write change detection module with tests**

```rust
// crates/core/src/change.rs
use crate::glossary::Glossary;
use crate::state::SegmentState;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SegmentStatus {
    /// Translation exists and is up-to-date.
    Translated,
    /// No translation yet.
    Pending,
    /// Source text changed since last translation.
    Stale,
    /// Context (adjacent segments) changed but source is the same.
    ContextChanged,
    /// Glossary terms changed since last translation.
    GlossaryStale,
}

/// Compute the status of a segment given its current state and the current context/glossary.
pub fn compute_status(
    segment: &SegmentState,
    current_source_hash: u64,
    current_context_hash: u64,
    glossary: &Glossary,
) -> SegmentStatus {
    if segment.translation.is_none() {
        return SegmentStatus::Pending;
    }

    if segment.source_hash != current_source_hash {
        return SegmentStatus::Stale;
    }

    if glossary.is_snapshot_stale(&segment.glossary_snapshot, &segment.source) {
        return SegmentStatus::GlossaryStale;
    }

    if segment.context_hash != current_context_hash {
        return SegmentStatus::ContextChanged;
    }

    SegmentStatus::Translated
}

impl SegmentStatus {
    /// Whether this status means the segment needs (re-)translation.
    pub fn needs_translation(&self) -> bool {
        matches!(
            self,
            SegmentStatus::Pending
                | SegmentStatus::Stale
                | SegmentStatus::GlossaryStale
                | SegmentStatus::ContextChanged
        )
    }

    /// Whether this is a high-priority translation target.
    /// ContextChanged is low priority and excluded here.
    pub fn is_high_priority(&self) -> bool {
        matches!(
            self,
            SegmentStatus::Pending | SegmentStatus::Stale | SegmentStatus::GlossaryStale
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::SegmentId;
    use std::collections::HashMap;

    fn make_segment_state(source_hash: u64, context_hash: u64, translated: bool) -> SegmentState {
        SegmentState {
            id: SegmentId::new(0, 0, 0),
            source: "test".to_string(),
            source_hash,
            context_hash,
            translation: if translated { Some("번역".to_string()) } else { None },
            glossary_snapshot: HashMap::new(),
            translated_at: None,
            issues: Vec::new(),
        }
    }

    #[test]
    fn pending_when_no_translation() {
        let seg = make_segment_state(100, 200, false);
        let status = compute_status(&seg, 100, 200, &Glossary::empty());
        assert_eq!(status, SegmentStatus::Pending);
    }

    #[test]
    fn translated_when_all_match() {
        let seg = make_segment_state(100, 200, true);
        let status = compute_status(&seg, 100, 200, &Glossary::empty());
        assert_eq!(status, SegmentStatus::Translated);
    }

    #[test]
    fn stale_when_source_hash_differs() {
        let seg = make_segment_state(100, 200, true);
        let status = compute_status(&seg, 999, 200, &Glossary::empty());
        assert_eq!(status, SegmentStatus::Stale);
    }

    #[test]
    fn context_changed_when_context_hash_differs() {
        let seg = make_segment_state(100, 200, true);
        let status = compute_status(&seg, 100, 999, &Glossary::empty());
        assert_eq!(status, SegmentStatus::ContextChanged);
    }

    #[test]
    fn glossary_stale_when_snapshot_outdated() {
        let glossary = Glossary::from_toml(r#"
[terms.test]
translation = "테스트"
"#).unwrap();
        let mut seg = make_segment_state(100, 200, true);
        seg.source = "test something".to_string();
        seg.glossary_snapshot.insert("test".to_string(), "이전번역".to_string());

        let status = compute_status(&seg, 100, 200, &glossary);
        assert_eq!(status, SegmentStatus::GlossaryStale);
    }

    #[test]
    fn needs_translation_for_actionable_statuses() {
        assert!(SegmentStatus::Pending.needs_translation());
        assert!(SegmentStatus::Stale.needs_translation());
        assert!(SegmentStatus::GlossaryStale.needs_translation());
        assert!(SegmentStatus::ContextChanged.needs_translation());
        assert!(!SegmentStatus::Translated.needs_translation());
    }

    #[test]
    fn high_priority_excludes_context_changed() {
        assert!(SegmentStatus::Pending.is_high_priority());
        assert!(SegmentStatus::Stale.is_high_priority());
        assert!(SegmentStatus::GlossaryStale.is_high_priority());
        assert!(!SegmentStatus::ContextChanged.is_high_priority());
        assert!(!SegmentStatus::Translated.is_high_priority());
    }
}
```

- [ ] **Step 2: Run tests**

Run: `cargo test -p yeokja-core change`
Expected: 6 tests pass

- [ ] **Step 3: Commit**

```bash
git add crates/core/src/change.rs
git commit -m "feat(core): add change detection with status computation"
```

---

### Task 7: Configuration

**Files:**
- Create: `crates/core/src/config.rs`
- Test: inline tests

- [ ] **Step 1: Write config module with tests**

```rust
// crates/core/src/config.rs
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectConfig {
    pub project: ProjectSettings,
    #[serde(default)]
    pub sources: Vec<SourceConfig>,
    pub provider: ProviderConfig,
    #[serde(default)]
    pub server: Option<ServerConfig>,
    #[serde(default)]
    pub evaluation: Option<EvaluationConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectSettings {
    pub source_lang: String,
    pub target_lang: String,
    #[serde(default = "default_glossary_path")]
    pub glossary: String,
}

fn default_glossary_path() -> String {
    "glossary.toml".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceConfig {
    pub path: String,
    pub pattern: String,
    pub parser: String,
    pub output: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderConfig {
    #[serde(rename = "type")]
    pub provider_type: String,
    pub model: String,
    #[serde(default)]
    pub api_key_env: Option<String>,
    #[serde(default)]
    pub base_url: Option<String>,
    /// Custom prompt template. If None, uses the default built-in template.
    #[serde(default)]
    pub prompt_template: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    #[serde(default = "default_port")]
    pub port: u16,
}

fn default_port() -> u16 {
    3000
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvaluationConfig {
    #[serde(default = "default_max_retries")]
    pub max_retries: u32,
    #[serde(default = "default_auto_evaluate")]
    pub auto_evaluate: bool,
}

fn default_max_retries() -> u32 {
    3
}

fn default_auto_evaluate() -> bool {
    true
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Parse error: {0}")]
    Parse(#[from] toml::de::Error),
}

impl ProjectConfig {
    pub fn load(path: &Path) -> Result<Self, ConfigError> {
        let content = std::fs::read_to_string(path)?;
        Self::from_toml(&content)
    }

    pub fn from_toml(content: &str) -> Result<Self, ConfigError> {
        Ok(toml::from_str(content)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_full_config() {
        let toml = r#"
[project]
source_lang = "en"
target_lang = "ko"
glossary = "glossary.toml"

[[sources]]
path = "book/"
pattern = "**/*.md"
parser = "markdown"
output = "{dir}/{stem}.ko{ext}"

[provider]
type = "anthropic"
model = "claude-sonnet-4-20250514"
api_key_env = "ANTHROPIC_API_KEY"

[server]
port = 8080

[evaluation]
max_retries = 5
auto_evaluate = false
"#;
        let config = ProjectConfig::from_toml(toml).unwrap();
        assert_eq!(config.project.source_lang, "en");
        assert_eq!(config.project.target_lang, "ko");
        assert_eq!(config.sources.len(), 1);
        assert_eq!(config.sources[0].parser, "markdown");
        assert_eq!(config.provider.provider_type, "anthropic");
        assert_eq!(config.server.unwrap().port, 8080);
        assert_eq!(config.evaluation.unwrap().max_retries, 5);
    }

    #[test]
    fn parse_minimal_config() {
        let toml = r#"
[project]
source_lang = "en"
target_lang = "ko"

[provider]
type = "openai_compatible"
model = "gpt-4o"
"#;
        let config = ProjectConfig::from_toml(toml).unwrap();
        assert_eq!(config.project.glossary, "glossary.toml"); // default
        assert!(config.sources.is_empty());
        assert!(config.server.is_none());
    }
}
```

- [ ] **Step 2: Run tests**

Run: `cargo test -p yeokja-core config`
Expected: 2 tests pass

- [ ] **Step 3: Commit**

```bash
git add crates/core/src/config.rs
git commit -m "feat(core): add project configuration deserialization"
```

---

### Task 8: Markdown Parser

**Files:**
- Create: `crates/parser-markdown/src/sentence.rs`
- Modify: `crates/parser-markdown/src/lib.rs`
- Test: inline tests in both files

- [ ] **Step 1: Write sentence splitter with tests**

```rust
// crates/parser-markdown/src/sentence.rs

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
```

- [ ] **Step 2: Run sentence tests**

Run: `cargo test -p yeokja-parser-markdown sentence`
Expected: 6 tests pass

- [ ] **Step 3: Write MarkdownParser implementation with tests**

```rust
// crates/parser-markdown/src/lib.rs
mod sentence;

use pulldown_cmark::{Event, HeadingLevel, Parser, Tag, TagEnd};
use yeokja_core::hash::content_hash;
use yeokja_core::model::*;
use yeokja_core::parser::{DocumentParser, TranslationMap};

pub struct MarkdownParser;

impl DocumentParser for MarkdownParser {
    fn parse(&self, source: &str) -> Document {
        let parser = Parser::new(source);
        let mut sections: Vec<Section> = vec![Section { blocks: Vec::new() }];
        let mut current_block_type: Option<BlockType> = None;
        let mut current_text = String::new();
        let mut current_heading_level: Option<u8> = None;
        let mut in_code_block = false;
        let mut section_idx: usize = 0;
        let mut block_idx: usize = 0;

        for event in parser {
            match event {
                Event::Start(Tag::Heading { level, .. }) => {
                    flush_block(
                        &mut sections,
                        &mut current_block_type,
                        &mut current_text,
                        &mut current_heading_level,
                        &mut section_idx,
                        &mut block_idx,
                    );
                    // Start new section on h1/h2
                    if matches!(level, HeadingLevel::H1 | HeadingLevel::H2) && !sections.last().unwrap().blocks.is_empty() {
                        sections.push(Section { blocks: Vec::new() });
                        section_idx += 1;
                        block_idx = 0;
                    }
                    current_block_type = Some(BlockType::Heading);
                    current_heading_level = Some(match level {
                        HeadingLevel::H1 => 1,
                        HeadingLevel::H2 => 2,
                        HeadingLevel::H3 => 3,
                        HeadingLevel::H4 => 4,
                        HeadingLevel::H5 => 5,
                        HeadingLevel::H6 => 6,
                    });
                }
                Event::End(TagEnd::Heading(_)) => {
                    flush_block(
                        &mut sections,
                        &mut current_block_type,
                        &mut current_text,
                        &mut current_heading_level,
                        &mut section_idx,
                        &mut block_idx,
                    );
                }
                Event::Start(Tag::Paragraph) => {
                    if !in_code_block {
                        current_block_type = Some(BlockType::Paragraph);
                    }
                }
                Event::End(TagEnd::Paragraph) => {
                    if !in_code_block {
                        flush_block(
                            &mut sections,
                            &mut current_block_type,
                            &mut current_text,
                            &mut current_heading_level,
                            &mut section_idx,
                            &mut block_idx,
                        );
                    }
                }
                Event::Start(Tag::Item) => {
                    current_block_type = Some(BlockType::ListItem);
                }
                Event::End(TagEnd::Item) => {
                    flush_block(
                        &mut sections,
                        &mut current_block_type,
                        &mut current_text,
                        &mut current_heading_level,
                        &mut section_idx,
                        &mut block_idx,
                    );
                }
                Event::Start(Tag::CodeBlock(_)) => {
                    in_code_block = true;
                    current_block_type = Some(BlockType::CodeBlock);
                }
                Event::End(TagEnd::CodeBlock) => {
                    in_code_block = false;
                    flush_block(
                        &mut sections,
                        &mut current_block_type,
                        &mut current_text,
                        &mut current_heading_level,
                        &mut section_idx,
                        &mut block_idx,
                    );
                }
                Event::Start(Tag::BlockQuote(_)) => {
                    current_block_type = Some(BlockType::BlockQuote);
                }
                Event::End(TagEnd::BlockQuote(_)) => {
                    flush_block(
                        &mut sections,
                        &mut current_block_type,
                        &mut current_text,
                        &mut current_heading_level,
                        &mut section_idx,
                        &mut block_idx,
                    );
                }
                Event::Text(text) | Event::Code(text) => {
                    current_text.push_str(&text);
                }
                Event::SoftBreak | Event::HardBreak => {
                    current_text.push(' ');
                }
                Event::Rule => {
                    flush_block(
                        &mut sections,
                        &mut current_block_type,
                        &mut current_text,
                        &mut current_heading_level,
                        &mut section_idx,
                        &mut block_idx,
                    );
                    // ThematicBreak is not translatable, add as empty block
                    let section = sections.last_mut().unwrap();
                    section.blocks.push(Block {
                        block_type: BlockType::ThematicBreak,
                        segments: Vec::new(),
                        raw_content: "---".to_string(),
                        heading_level: None,
                    });
                    block_idx += 1;
                }
                _ => {}
            }
        }

        // Flush any remaining content
        flush_block(
            &mut sections,
            &mut current_block_type,
            &mut current_text,
            &mut current_heading_level,
            &mut section_idx,
            &mut block_idx,
        );

        // Remove empty sections
        sections.retain(|s| !s.blocks.is_empty());

        Document { sections }
    }

    fn reconstruct(&self, document: &Document, translations: &TranslationMap) -> String {
        // For the initial implementation, reconstruct by walking segments
        // and substituting translations. This is a simplified version
        // that works for basic cases; a full implementation would preserve
        // the original markdown structure more precisely.
        let mut output = String::new();

        for section in &document.sections {
            for block in &section.blocks {
                match block.block_type {
                    BlockType::CodeBlock => {
                        output.push_str("```\n");
                        output.push_str(&block.raw_content);
                        output.push_str("\n```\n\n");
                    }
                    BlockType::ThematicBreak => {
                        output.push_str("---\n\n");
                    }
                    BlockType::Heading => {
                        let level = block.heading_level.unwrap_or(1);
                        let prefix = "#".repeat(level as usize);
                        output.push_str(&format!("{prefix} "));
                        for seg in &block.segments {
                            let text = translations
                                .get(&seg.id)
                                .unwrap_or(&seg.source);
                            output.push_str(text);
                        }
                        output.push_str("\n\n");
                    }
                    _ => {
                        for (i, seg) in block.segments.iter().enumerate() {
                            let text = translations
                                .get(&seg.id)
                                .unwrap_or(&seg.source);
                            if i > 0 {
                                output.push(' ');
                            }
                            output.push_str(text);
                        }
                        output.push_str("\n\n");
                    }
                }
            }
        }

        output.trim_end().to_string()
    }
}

fn flush_block(
    sections: &mut Vec<Section>,
    current_block_type: &mut Option<BlockType>,
    current_text: &mut String,
    current_heading_level: &mut Option<u8>,
    section_idx: &mut usize,
    block_idx: &mut usize,
) {
    if let Some(block_type) = current_block_type.take() {
        let text = std::mem::take(current_text);
        let heading_level = current_heading_level.take();
        if text.trim().is_empty() {
            return;
        }

        let raw_content = text.clone();
        let segments = if block_type.is_translatable() {
            sentence::split_sentences(&text)
                .into_iter()
                .enumerate()
                .map(|(seg_i, sent)| {
                    let hash = content_hash(&sent);
                    Segment {
                        id: SegmentId::new(*section_idx, *block_idx, seg_i),
                        source: sent,
                        source_hash: hash,
                        block_type,
                    }
                })
                .collect()
        } else {
            Vec::new()
        };

        let section = sections.last_mut().unwrap();
        section.blocks.push(Block {
            block_type,
            segments,
            raw_content,
            heading_level,
        });
        *block_idx += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_simple_paragraph() {
        let parser = MarkdownParser;
        let doc = parser.parse("Hello world. Goodbye world.");
        assert_eq!(doc.sections.len(), 1);
        let segments = doc.translatable_segments();
        assert_eq!(segments.len(), 2);
        assert_eq!(segments[0].source, "Hello world.");
        assert_eq!(segments[1].source, "Goodbye world.");
    }

    #[test]
    fn parse_heading_starts_new_section() {
        let parser = MarkdownParser;
        let doc = parser.parse("# Chapter 1\n\nSome text.\n\n## Chapter 2\n\nMore text.");
        assert!(doc.sections.len() >= 2);
    }

    #[test]
    fn code_blocks_not_translatable() {
        let parser = MarkdownParser;
        let doc = parser.parse("Some text.\n\n```\nfn main() {}\n```\n\nMore text.");
        let translatable = doc.translatable_segments();
        for seg in &translatable {
            assert_ne!(seg.block_type, BlockType::CodeBlock);
        }
    }

    #[test]
    fn reconstruct_with_translations() {
        let parser = MarkdownParser;
        let doc = parser.parse("Hello world.");
        let segments = doc.translatable_segments();
        let mut translations = TranslationMap::new();
        translations.insert(segments[0].id.clone(), "안녕하세요.".to_string());

        let output = parser.reconstruct(&doc, &translations);
        assert!(output.contains("안녕하세요."));
    }

    #[test]
    fn reconstruct_falls_back_to_original() {
        let parser = MarkdownParser;
        let doc = parser.parse("Hello world.");
        let translations = TranslationMap::new(); // empty

        let output = parser.reconstruct(&doc, &translations);
        assert!(output.contains("Hello world."));
    }
}
```

- [ ] **Step 4: Run all parser-markdown tests**

Run: `cargo test -p yeokja-parser-markdown`
Expected: 11 tests pass (6 sentence + 5 parser)

- [ ] **Step 5: Commit**

```bash
git add crates/parser-markdown/
git commit -m "feat(parser-markdown): add Markdown parser with sentence splitting"
```

---

### Task 9: Translation Provider Trait and Prompt Builder

**Files:**
- Create: `crates/translate/src/provider.rs`
- Create: `crates/translate/src/prompt.rs`
- Test: inline tests in `prompt.rs`

- [ ] **Step 1: Write provider trait**

```rust
// crates/translate/src/provider.rs
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct TranslateRequest {
    /// Segments to translate, keyed by index (1-based, matching prompt format).
    pub segments: Vec<(usize, String)>,
    /// Full text of the containing block for context.
    pub block_context: String,
    /// Glossary terms relevant to these segments.
    pub glossary: HashMap<String, String>,
    pub source_lang: String,
    pub target_lang: String,
    /// Optional feedback from previous evaluation failures (for retry loop).
    pub feedback: Option<String>,
}

#[derive(Debug, Clone)]
pub struct TranslateResponse {
    /// Segment index → translated text.
    pub translations: HashMap<usize, String>,
    pub usage: Option<TokenUsage>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
}

#[derive(Debug, thiserror::Error)]
pub enum TranslateError {
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("API error: {status} {message}")]
    Api { status: u16, message: String },
    #[error("Rate limited")]
    RateLimited { retry_after: Option<u64> },
    #[error("Parse error: {0}")]
    Parse(String),
}

#[async_trait]
pub trait TranslationProvider: Send + Sync {
    async fn translate(&self, request: TranslateRequest) -> Result<TranslateResponse, TranslateError>;
}
```

- [ ] **Step 2: Write prompt builder with tests**

```rust
// crates/translate/src/prompt.rs
use crate::provider::TranslateRequest;
use std::collections::HashMap;

/// Build the translation prompt from a TranslateRequest.
pub fn build_prompt(request: &TranslateRequest) -> String {
    let mut prompt = String::new();

    prompt.push_str(&format!(
        "Translate the following sentences from {} to {}.\n",
        request.source_lang, request.target_lang
    ));
    prompt.push_str("Respond with each numbered translation in the same [N] format.\n");

    if !request.glossary.is_empty() {
        prompt.push_str("\nGlossary (use these translations for the given terms):\n");
        let mut terms: Vec<_> = request.glossary.iter().collect();
        terms.sort_by_key(|(k, _)| k.as_str());
        for (term, translation) in terms {
            prompt.push_str(&format!("- {term} → {translation}\n"));
        }
    }

    if let Some(feedback) = &request.feedback {
        prompt.push_str(&format!("\nPrevious translation had these issues, please fix them:\n{feedback}\n"));
    }

    prompt.push_str(&format!("\nContext (full paragraph):\n{}\n", request.block_context));

    prompt.push_str("\nSentences to translate:\n");
    for (idx, text) in &request.segments {
        prompt.push_str(&format!("[{idx}] {text}\n"));
    }

    prompt
}

/// Parse a translation response in [N] format.
pub fn parse_response(response: &str) -> Result<HashMap<usize, String>, String> {
    let mut translations = HashMap::new();

    for line in response.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        if let Some(rest) = line.strip_prefix('[') {
            if let Some(bracket_end) = rest.find(']') {
                let idx_str = &rest[..bracket_end];
                if let Ok(idx) = idx_str.parse::<usize>() {
                    let translation = rest[bracket_end + 1..].trim().to_string();
                    if !translation.is_empty() {
                        translations.insert(idx, translation);
                    }
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
            feedback: None,
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
```

- [ ] **Step 3: Run tests**

Run: `cargo test -p yeokja-translate prompt`
Expected: 7 tests pass

- [ ] **Step 4: Commit**

```bash
git add crates/translate/src/
git commit -m "feat(translate): add TranslationProvider trait, prompt builder, response parser"
```

---

### Task 10: Rate Limiter

**Files:**
- Create: `crates/translate/src/rate_limit.rs`
- Test: inline tests

- [ ] **Step 1: Write rate limiter with tests**

```rust
// crates/translate/src/rate_limit.rs
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use tokio::time::Instant;

/// Adaptive rate limiter that adjusts based on API response headers and errors.
pub struct RateLimiter {
    state: Arc<Mutex<RateLimitState>>,
}

struct RateLimitState {
    /// Minimum interval between requests.
    min_interval: Duration,
    /// Last request timestamp.
    last_request: Option<Instant>,
    /// Current backoff multiplier (increases on rate limit errors).
    backoff_multiplier: f64,
    /// Retry-after deadline (from 429 response header).
    retry_after_deadline: Option<Instant>,
}

impl RateLimiter {
    pub fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(RateLimitState {
                min_interval: Duration::from_millis(100),
                last_request: None,
                backoff_multiplier: 1.0,
                retry_after_deadline: None,
            })),
        }
    }

    /// Wait until it's safe to make a request.
    pub async fn acquire(&self) {
        let wait_duration = {
            let state = self.state.lock().await;
            let now = Instant::now();

            // Check retry-after deadline
            if let Some(deadline) = state.retry_after_deadline {
                if now < deadline {
                    Some(deadline - now)
                } else {
                    state.last_request.and_then(|last| {
                        let interval = state.min_interval.mul_f64(state.backoff_multiplier);
                        let next = last + interval;
                        (next > now).then(|| next - now)
                    })
                }
            } else {
                state.last_request.and_then(|last| {
                    let interval = state.min_interval.mul_f64(state.backoff_multiplier);
                    let next = last + interval;
                    (next > now).then(|| next - now)
                })
            }
        };

        if let Some(duration) = wait_duration {
            tokio::time::sleep(duration).await;
        }

        let mut state = self.state.lock().await;
        state.last_request = Some(Instant::now());
    }

    /// Report a successful request. Gradually reduce backoff.
    pub async fn report_success(&self) {
        let mut state = self.state.lock().await;
        state.backoff_multiplier = (state.backoff_multiplier * 0.9).max(1.0);
        state.retry_after_deadline = None;
    }

    /// Report a rate limit (429) error.
    pub async fn report_rate_limited(&self, retry_after_secs: Option<u64>) {
        let mut state = self.state.lock().await;
        state.backoff_multiplier = (state.backoff_multiplier * 2.0).min(64.0);
        if let Some(secs) = retry_after_secs {
            state.retry_after_deadline = Some(Instant::now() + Duration::from_secs(secs));
        }
    }

    /// Update rate based on remaining quota from response headers.
    pub async fn update_from_remaining(&self, remaining: u64) {
        let mut state = self.state.lock().await;
        if remaining == 0 {
            state.backoff_multiplier = (state.backoff_multiplier * 2.0).min(64.0);
        } else if remaining > 100 {
            state.backoff_multiplier = (state.backoff_multiplier * 0.8).max(1.0);
        }
    }
}

impl Default for RateLimiter {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn acquire_does_not_block_on_first_call() {
        let limiter = RateLimiter::new();
        let start = Instant::now();
        limiter.acquire().await;
        assert!(start.elapsed() < Duration::from_millis(50));
    }

    #[tokio::test]
    async fn backoff_increases_on_rate_limit() {
        let limiter = RateLimiter::new();
        limiter.report_rate_limited(None).await;
        let state = limiter.state.lock().await;
        assert!(state.backoff_multiplier > 1.0);
    }

    #[tokio::test]
    async fn backoff_decreases_on_success() {
        let limiter = RateLimiter::new();
        limiter.report_rate_limited(None).await;
        limiter.report_success().await;
        let state = limiter.state.lock().await;
        assert!(state.backoff_multiplier < 2.0);
    }

    #[tokio::test]
    async fn backoff_capped_at_max() {
        let limiter = RateLimiter::new();
        for _ in 0..20 {
            limiter.report_rate_limited(None).await;
        }
        let state = limiter.state.lock().await;
        assert!(state.backoff_multiplier <= 64.0);
    }
}
```

- [ ] **Step 2: Run tests**

Run: `cargo test -p yeokja-translate rate_limit`
Expected: 4 tests pass

- [ ] **Step 3: Commit**

```bash
git add crates/translate/src/rate_limit.rs
git commit -m "feat(translate): add adaptive rate limiter with exponential backoff"
```

---

### Task 11: OpenAI-Compatible Provider

**Files:**
- Create: `crates/translate/src/openai_compatible.rs`
- Test: inline test (basic construction, prompt integration)

- [ ] **Step 1: Write OpenAI-compatible provider**

```rust
// crates/translate/src/openai_compatible.rs
use crate::prompt::{build_prompt, parse_response};
use crate::provider::{TokenUsage, TranslateError, TranslateRequest, TranslateResponse, TranslationProvider};
use crate::rate_limit::RateLimiter;
use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};

pub struct OpenAICompatibleProvider {
    client: Client,
    base_url: String,
    api_key: String,
    model: String,
    rate_limiter: RateLimiter,
}

#[derive(Serialize)]
struct ChatRequest {
    model: String,
    messages: Vec<ChatMessage>,
}

#[derive(Serialize, Deserialize)]
struct ChatMessage {
    role: String,
    content: String,
}

#[derive(Deserialize)]
struct ChatResponse {
    choices: Vec<ChatChoice>,
    usage: Option<ChatUsage>,
}

#[derive(Deserialize)]
struct ChatChoice {
    message: ChatMessage,
}

#[derive(Deserialize)]
struct ChatUsage {
    prompt_tokens: u64,
    completion_tokens: u64,
}

impl OpenAICompatibleProvider {
    pub fn new(base_url: String, api_key: String, model: String) -> Self {
        Self {
            client: Client::new(),
            base_url,
            api_key,
            model,
            rate_limiter: RateLimiter::new(),
        }
    }
}

#[async_trait]
impl TranslationProvider for OpenAICompatibleProvider {
    async fn translate(&self, request: TranslateRequest) -> Result<TranslateResponse, TranslateError> {
        self.rate_limiter.acquire().await;

        let prompt = build_prompt(&request);
        let chat_request = ChatRequest {
            model: self.model.clone(),
            messages: vec![ChatMessage {
                role: "user".to_string(),
                content: prompt,
            }],
        };

        let url = format!("{}/chat/completions", self.base_url.trim_end_matches('/'));
        let response = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .json(&chat_request)
            .send()
            .await?;

        // Check for rate limit headers
        if let Some(remaining) = response
            .headers()
            .get("x-ratelimit-remaining")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse::<u64>().ok())
        {
            self.rate_limiter.update_from_remaining(remaining).await;
        }

        let status = response.status().as_u16();
        if status == 429 {
            let retry_after = response
                .headers()
                .get("retry-after")
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.parse::<u64>().ok());
            self.rate_limiter.report_rate_limited(retry_after).await;
            return Err(TranslateError::RateLimited { retry_after });
        }

        if !response.status().is_success() {
            let message = response.text().await.unwrap_or_default();
            return Err(TranslateError::Api { status, message });
        }

        self.rate_limiter.report_success().await;

        let chat_response: ChatResponse = response.json().await?;
        let reply = chat_response
            .choices
            .first()
            .map(|c| c.message.content.as_str())
            .unwrap_or("");

        let translations = parse_response(reply)
            .map_err(|e| TranslateError::Parse(e))?;

        Ok(TranslateResponse {
            translations,
            usage: chat_response.usage.map(|u| TokenUsage {
                input_tokens: u.prompt_tokens,
                output_tokens: u.completion_tokens,
            }),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_construction() {
        let provider = OpenAICompatibleProvider::new(
            "https://api.openai.com/v1".to_string(),
            "test-key".to_string(),
            "gpt-4o".to_string(),
        );
        assert_eq!(provider.model, "gpt-4o");
    }
}
```

- [ ] **Step 2: Run tests**

Run: `cargo test -p yeokja-translate openai`
Expected: 1 test passes

- [ ] **Step 3: Commit**

```bash
git add crates/translate/src/openai_compatible.rs
git commit -m "feat(translate): add OpenAI-compatible translation provider"
```

---

### Task 12: Integration Test — Full Pipeline

**Files:**
- Create: `crates/core/tests/pipeline_test.rs`

This test validates the full flow: parse → reconcile → detect changes → (mock translate) → save state → reconstruct, without making actual API calls.

- [ ] **Step 1: Write integration test**

```rust
// crates/core/tests/pipeline_test.rs
use std::collections::HashMap;
use yeokja_core::change::{compute_status, SegmentStatus};
use yeokja_core::glossary::Glossary;
use yeokja_core::hash::{content_hash, context_hash};
use yeokja_core::model::SegmentId;
use yeokja_core::parser::{DocumentParser, TranslationMap};
use yeokja_core::reconcile::reconcile;
use yeokja_core::state::{SegmentState, StateFile};
use chrono::Utc;

// We can't import yeokja_parser_markdown from core's tests, so we'll build
// a Document manually for this test.
use yeokja_core::model::*;

fn make_document(texts: &[&str]) -> Document {
    let segments: Vec<Segment> = texts
        .iter()
        .enumerate()
        .map(|(i, text)| Segment {
            id: SegmentId::new(0, 0, i),
            source: text.to_string(),
            source_hash: content_hash(text),
            block_type: BlockType::Paragraph,
        })
        .collect();

    Document {
        sections: vec![Section {
            blocks: vec![Block {
                block_type: BlockType::Paragraph,
                segments,
                raw_content: texts.join(" "),
                heading_level: None,
            }],
        }],
    }
}

#[test]
fn full_pipeline_new_document() {
    // 1. Parse a new document
    let doc = make_document(&["Hello world.", "Goodbye world."]);
    let state = StateFile::new(0);
    let glossary = Glossary::empty();

    // 2. Reconcile — all segments should be new
    let result = reconcile(&doc, &state);
    assert_eq!(result.segments.len(), 2);

    // 3. Check status — all should be Pending
    let segs = doc.translatable_segments();
    let hashes: Vec<u64> = segs.iter().map(|s| s.source_hash).collect();
    for (i, seg_state) in result.segments.iter().enumerate() {
        let prev = if i > 0 { Some(hashes[i - 1]) } else { None };
        let next = hashes.get(i + 1).copied();
        let ctx = context_hash(prev, next);
        let status = compute_status(seg_state, seg_state.source_hash, ctx, &glossary);
        assert_eq!(status, SegmentStatus::Pending);
    }
}

#[test]
fn full_pipeline_incremental_update() {
    let glossary = Glossary::empty();

    // 1. First run: translate everything
    let doc1 = make_document(&["Hello.", "World."]);
    let state1 = StateFile::new(0);
    let result1 = reconcile(&doc1, &state1);

    // Simulate translation
    let mut state2 = StateFile::new(content_hash("Hello. World."));
    let segs1 = doc1.translatable_segments();
    let hashes1: Vec<u64> = segs1.iter().map(|s| s.source_hash).collect();
    for (i, seg) in result1.segments.iter().enumerate() {
        let prev = if i > 0 { Some(hashes1[i - 1]) } else { None };
        let next = hashes1.get(i + 1).copied();
        let ctx = context_hash(prev, next);
        state2.segments.push(SegmentState {
            id: seg.id.clone(),
            source: seg.source.clone(),
            source_hash: seg.source_hash,
            context_hash: ctx,
            translation: Some(format!("translated_{}", seg.source)),
            glossary_snapshot: HashMap::new(),
            translated_at: Some(Utc::now()),
            issues: Vec::new(),
        });
    }

    // 2. Second run: change first sentence only
    let doc2 = make_document(&["Hi.", "World."]);
    let result2 = reconcile(&doc2, &state2);

    let segs2 = doc2.translatable_segments();
    let hashes2: Vec<u64> = segs2.iter().map(|s| s.source_hash).collect();

    // First segment: new (no match), should be Pending
    let ctx0 = context_hash(None, Some(hashes2[1]));
    let status0 = compute_status(&result2.segments[0], segs2[0].source_hash, ctx0, &glossary);
    assert_eq!(status0, SegmentStatus::Pending);

    // Second segment: unchanged, should be Translated (or ContextChanged since prev changed)
    let ctx1 = context_hash(Some(hashes2[0]), None);
    let status1 = compute_status(&result2.segments[1], segs2[1].source_hash, ctx1, &glossary);
    // Context changed because the previous segment changed
    assert_eq!(status1, SegmentStatus::ContextChanged);
    // Needs translation but at low priority
    assert!(status1.needs_translation());
    assert!(!status1.is_high_priority());
}

#[test]
fn full_pipeline_glossary_stale() {
    // 1. Translate with empty glossary
    let doc = make_document(&["The repository is ready."]);
    let state = StateFile::new(0);
    let result = reconcile(&doc, &state);

    let segs = doc.translatable_segments();
    let ctx = context_hash(None, None);
    let mut state2 = StateFile::new(0);
    state2.segments.push(SegmentState {
        id: result.segments[0].id.clone(),
        source: "The repository is ready.".to_string(),
        source_hash: result.segments[0].source_hash,
        context_hash: ctx,
        translation: Some("레포지토리가 준비되었습니다.".to_string()),
        glossary_snapshot: HashMap::new(), // no glossary terms recorded
        translated_at: Some(Utc::now()),
        issues: Vec::new(),
    });

    // 2. Now add a glossary term
    let glossary = Glossary::from_toml(r#"
[terms.repository]
translation = "저장소"
"#).unwrap();

    let result2 = reconcile(&doc, &state2);
    let status = compute_status(&result2.segments[0], segs[0].source_hash, ctx, &glossary);
    assert_eq!(status, SegmentStatus::GlossaryStale);
    assert!(status.needs_translation());
}
```

- [ ] **Step 2: Run integration test**

Run: `cargo test -p yeokja-core --test pipeline_test`
Expected: 3 tests pass

- [ ] **Step 3: Commit**

```bash
git add crates/core/tests/
git commit -m "test: add full pipeline integration tests"
```

---

### Task 13: Final Workspace Verification

- [ ] **Step 1: Run all tests across workspace**

Run: `cargo test --workspace`
Expected: All tests pass (hash: 4, glossary: 7, state: 3, reconcile: 5, change: 7, config: 2, sentence: 6, parser: 5, prompt: 7, rate_limit: 4, openai: 1, pipeline: 3 = ~54 tests)

- [ ] **Step 2: Run clippy**

Run: `cargo clippy --workspace -- -D warnings`
Expected: No warnings

- [ ] **Step 3: Fix any clippy warnings and commit**

```bash
git add -A
git commit -m "chore: fix clippy warnings"
```
