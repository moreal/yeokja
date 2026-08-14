//! Assembling the buildable tree a `[derive]` config describes.
//!
//! The tree is a symlink farm: real directories, linked files. The base
//! layer (an upstream submodule) goes down first, overlays replace links at
//! the same relative path, then patch/generate steps run inside the tree.
//! Everything lands in a temporary directory that is swapped into place only
//! when every step succeeded, so a failed assembly leaves the previous tree
//! untouched — cleanup is never repair, it is reassembly.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;
use yeokja_core::config::{DeriveConfig, DeriveStep, ProjectConfig};

#[derive(Debug, thiserror::Error)]
pub enum AssembleError {
    #[error("no [derive] section in yeokja.toml")]
    NotConfigured,
    #[error("base layer {0} does not exist")]
    MissingBase(PathBuf),
    #[error("overlay {0} does not exist")]
    MissingOverlay(PathBuf),
    #[error("IO error on {path}: {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("patch {file} failed:\n{output}")]
    PatchFailed { file: String, output: String },
    #[error("generate step failed ({command}):\n{output}")]
    GenerateFailed { command: String, output: String },
}

fn io_err(path: &Path) -> impl FnOnce(std::io::Error) -> AssembleError + '_ {
    move |source| AssembleError::Io {
        path: path.to_path_buf(),
        source,
    }
}

#[derive(Debug, Default)]
pub struct AssembleReport {
    pub files_linked: usize,
    pub files_overlaid: usize,
    /// Overlay files skipped because `require_base` found no base counterpart
    /// — usually translations whose source upstream deleted.
    pub skipped_orphans: Vec<PathBuf>,
    pub patches_applied: Vec<String>,
    pub steps_run: usize,
    pub target: PathBuf,
}

/// Assemble the tree for the project at `root`. Returns what was done; on
/// any error the previous tree (if any) is left in place.
pub fn assemble(root: &Path, config: &ProjectConfig) -> Result<AssembleReport, AssembleError> {
    let derive = config.derive.as_ref().ok_or(AssembleError::NotConfigured)?;
    let root = std::fs::canonicalize(root).map_err(io_err(root))?;

    let base = root.join(&derive.base);
    if !base.is_dir() {
        return Err(AssembleError::MissingBase(base));
    }
    let target = root.join(&derive.target);
    let tmp = tmp_path(&target);
    if tmp.exists() {
        std::fs::remove_dir_all(&tmp).map_err(io_err(&tmp))?;
    }

    let result = assemble_into(&root, &base, derive, &tmp);
    match result {
        Ok(mut report) => {
            swap_into_place(&tmp, &target)?;
            report.target = target;
            Ok(report)
        }
        Err(e) => {
            let _ = std::fs::remove_dir_all(&tmp);
            Err(e)
        }
    }
}

fn tmp_path(target: &Path) -> PathBuf {
    let name = target
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "tree".to_string());
    target.with_file_name(format!(".{name}.assembling"))
}

fn assemble_into(
    root: &Path,
    base: &Path,
    derive: &DeriveConfig,
    tmp: &Path,
) -> Result<AssembleReport, AssembleError> {
    let mut report = AssembleReport::default();

    let base_files = walk_files(base)?;
    for rel in &base_files {
        link_file(&base.join(rel), &tmp.join(rel))?;
        report.files_linked += 1;
    }
    let base_set: BTreeSet<&PathBuf> = base_files.iter().collect();

    for overlay in &derive.overlay {
        let overlay_root = root.join(&overlay.path);
        if !overlay_root.is_dir() {
            // An overlay that produces nothing yet (ko/ before the first
            // translation) is an empty layer, not an error — but a typo'd
            // path should not silently assemble a base-only tree.
            if overlay.require_base {
                tracing::warn!(overlay = %overlay_root.display(), "Overlay directory missing; treating as empty");
                continue;
            }
            return Err(AssembleError::MissingOverlay(overlay_root));
        }
        for rel in walk_files(&overlay_root)? {
            if overlay.require_base && !base_set.contains(&rel) {
                report.skipped_orphans.push(rel);
                continue;
            }
            let dest = tmp.join(&rel);
            if dest.exists() || dest.is_symlink() {
                std::fs::remove_file(&dest).map_err(io_err(&dest))?;
            }
            link_file(&overlay_root.join(&rel), &dest)?;
            report.files_overlaid += 1;
        }
    }

    for step in &derive.step {
        run_step(root, tmp, step, &mut report)?;
        report.steps_run += 1;
    }

    Ok(report)
}

fn run_step(
    root: &Path,
    tmp: &Path,
    step: &DeriveStep,
    report: &mut AssembleReport,
) -> Result<(), AssembleError> {
    match step {
        DeriveStep::Patch { file } => {
            let patch_path = root.join(file);
            let patch_text = std::fs::read_to_string(&patch_path).map_err(io_err(&patch_path))?;
            for rel in patch_targets(&patch_text) {
                materialize(&tmp.join(&rel))?;
            }
            let output = Command::new("git")
                .arg("apply")
                .arg(&patch_path)
                .current_dir(tmp)
                .output()
                .map_err(io_err(&patch_path))?;
            if !output.status.success() {
                return Err(AssembleError::PatchFailed {
                    file: file.clone(),
                    output: String::from_utf8_lossy(&output.stderr).into_owned(),
                });
            }
            report.patches_applied.push(file.clone());
        }
        DeriveStep::Generate { command } => {
            let output = Command::new("sh")
                .arg("-c")
                .arg(command)
                .current_dir(tmp)
                .env("YEOKJA_ROOT", root)
                .output()
                .map_err(io_err(tmp))?;
            if !output.status.success() {
                return Err(AssembleError::GenerateFailed {
                    command: command.clone(),
                    output: String::from_utf8_lossy(&output.stderr).into_owned(),
                });
            }
        }
    }
    Ok(())
}

/// The tree-relative files a unified diff touches, read from `+++ b/` (and,
/// for deletions, `--- a/`) headers.
fn patch_targets(patch: &str) -> Vec<PathBuf> {
    let mut targets = BTreeSet::new();
    for line in patch.lines() {
        let path = line
            .strip_prefix("+++ b/")
            .or_else(|| line.strip_prefix("--- a/"));
        if let Some(path) = path
            && path != "dev/null"
        {
            targets.insert(PathBuf::from(path.trim()));
        }
    }
    targets.into_iter().collect()
}

/// Replace a symlink with a real copy of what it points at, so a patch (or
/// any in-tree edit) can never write through into a layer.
fn materialize(path: &Path) -> Result<(), AssembleError> {
    if !path.is_symlink() {
        return Ok(());
    }
    let real = std::fs::canonicalize(path).map_err(io_err(path))?;
    std::fs::remove_file(path).map_err(io_err(path))?;
    std::fs::copy(&real, path).map_err(io_err(path))?;
    Ok(())
}

fn link_file(source: &Path, dest: &Path) -> Result<(), AssembleError> {
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent).map_err(io_err(parent))?;
    }
    #[cfg(unix)]
    std::os::unix::fs::symlink(source, dest).map_err(io_err(dest))?;
    #[cfg(not(unix))]
    std::fs::copy(source, dest).map(|_| ()).map_err(io_err(dest))?;
    Ok(())
}

/// Relative paths of every file under `root`, `.git` excluded, sorted.
fn walk_files(root: &Path) -> Result<Vec<PathBuf>, AssembleError> {
    let mut files = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).map_err(io_err(&dir))?.flatten() {
            let path = entry.path();
            if path.file_name().is_some_and(|n| n == ".git") {
                continue;
            }
            // Symlinked directories inside a layer are not followed; a cycle
            // would walk forever, and layers are expected to hold real trees.
            if path.is_dir() && !path.is_symlink() {
                stack.push(path);
            } else if path.is_file() || path.is_symlink() {
                files.push(path.strip_prefix(root).expect("walked under root").to_path_buf());
            }
        }
    }
    files.sort();
    Ok(files)
}

/// Copy `src` (file or directory) to `dest`, replacing what was there and
/// reading *through* symlinks: build outputs leave the tree as real files,
/// so a published dist never dangles into the tree or the layers.
pub fn copy_dereferenced(src: &Path, dest: &Path) -> Result<(), AssembleError> {
    if dest.exists() {
        if dest.is_dir() && !dest.is_symlink() {
            std::fs::remove_dir_all(dest).map_err(io_err(dest))?;
        } else {
            std::fs::remove_file(dest).map_err(io_err(dest))?;
        }
    }
    // `metadata` stats through links, so a symlinked directory in a build
    // output is descended into rather than mistaken for a file.
    let meta = std::fs::metadata(src).map_err(io_err(src))?;
    if meta.is_dir() {
        std::fs::create_dir_all(dest).map_err(io_err(dest))?;
        for entry in std::fs::read_dir(src).map_err(io_err(src))?.flatten() {
            copy_dereferenced(&entry.path(), &dest.join(entry.file_name()))?;
        }
    } else {
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent).map_err(io_err(parent))?;
        }
        std::fs::copy(src, dest).map_err(io_err(dest))?;
    }
    Ok(())
}

/// Retire the previous tree and move the finished one into place. Two
/// renames, so the window without a tree is as small as the filesystem
/// allows; a leftover `.old` from a crash is removed on the next run.
fn swap_into_place(tmp: &Path, target: &Path) -> Result<(), AssembleError> {
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent).map_err(io_err(parent))?;
    }
    let old = target.with_file_name(format!(
        ".{}.retired",
        target
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "tree".to_string())
    ));
    if old.exists() {
        std::fs::remove_dir_all(&old).map_err(io_err(&old))?;
    }
    if target.exists() {
        std::fs::rename(target, &old).map_err(io_err(target))?;
    }
    std::fs::rename(tmp, target).map_err(io_err(target))?;
    if old.exists() {
        std::fs::remove_dir_all(&old).map_err(io_err(&old))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(path: &Path, content: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, content).unwrap();
    }

    fn read(path: &Path) -> String {
        std::fs::read_to_string(path).unwrap()
    }

    fn project(toml_tail: &str) -> ProjectConfig {
        ProjectConfig::from_toml(&format!(
            r#"
[project]
source_lang = "en"
target_lang = "ko"

[provider]
type = "claude_code"
model = "test"

{toml_tail}
"#
        ))
        .unwrap()
    }

    #[test]
    fn base_is_linked_and_overlay_wins() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write(&root.join("upstream/book.adoc"), "include::chapters/a.adoc[]");
        write(&root.join("upstream/chapters/a.adoc"), "English");
        write(&root.join("ko/chapters/a.adoc"), "한국어");

        let config = project(
            r#"
[derive]
base = "upstream"

[[derive.overlay]]
path = "ko"
require_base = true
"#,
        );
        let report = assemble(root, &config).unwrap();
        assert_eq!(report.files_linked, 2);
        assert_eq!(report.files_overlaid, 1);
        let tree = root.join("build/tree");
        assert_eq!(read(&tree.join("book.adoc")), "include::chapters/a.adoc[]");
        assert_eq!(read(&tree.join("chapters/a.adoc")), "한국어");
        // The tree is links, not copies.
        assert!(tree.join("chapters/a.adoc").is_symlink());
    }

    #[test]
    fn require_base_skips_orphans_but_allows_new_asset_files() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write(&root.join("upstream/chapters/a.adoc"), "English");
        write(&root.join("ko/chapters/a.adoc"), "한국어");
        write(&root.join("ko/chapters/deleted.adoc"), "고아 번역");
        write(&root.join("assets/style/theme.yml"), "font: nanum");

        let config = project(
            r#"
[derive]
base = "upstream"

[[derive.overlay]]
path = "ko"
require_base = true

[[derive.overlay]]
path = "assets"
"#,
        );
        let report = assemble(root, &config).unwrap();
        assert_eq!(report.skipped_orphans, vec![PathBuf::from("chapters/deleted.adoc")]);
        let tree = root.join("build/tree");
        assert!(!tree.join("chapters/deleted.adoc").exists());
        assert_eq!(read(&tree.join("style/theme.yml")), "font: nanum");
    }

    #[test]
    fn patch_materializes_and_never_writes_through_to_the_base() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write(&root.join("upstream/book.adoc"), "line one\n");
        write(
            &root.join("patches/fix.patch"),
            "--- a/book.adoc\n+++ b/book.adoc\n@@ -1 +1,2 @@\n line one\n+line two\n",
        );

        let config = project(
            r#"
[derive]
base = "upstream"

[[derive.step]]
kind = "patch"
file = "patches/fix.patch"
"#,
        );
        let report = assemble(root, &config).unwrap();
        assert_eq!(report.patches_applied, vec!["patches/fix.patch"]);
        let tree = root.join("build/tree");
        assert_eq!(read(&tree.join("book.adoc")), "line one\nline two\n");
        assert!(!tree.join("book.adoc").is_symlink());
        assert_eq!(read(&root.join("upstream/book.adoc")), "line one\n");
    }

    #[test]
    fn generate_runs_in_the_tree_with_the_root_exposed() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write(&root.join("upstream/a.txt"), "x");

        let config = project(
            r#"
[derive]
base = "upstream"

[[derive.step]]
kind = "generate"
command = "printf '%s' \"$YEOKJA_ROOT\" > where.txt"
"#,
        );
        assemble(root, &config).unwrap();
        let recorded = read(&root.join("build/tree/where.txt"));
        assert_eq!(
            PathBuf::from(recorded),
            std::fs::canonicalize(root).unwrap()
        );
    }

    #[test]
    fn a_failing_step_leaves_the_previous_tree_untouched() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write(&root.join("upstream/a.txt"), "v1");

        let ok = project(
            r#"
[derive]
base = "upstream"
"#,
        );
        assemble(root, &ok).unwrap();

        write(&root.join("upstream/a.txt"), "v2");
        let failing = project(
            r#"
[derive]
base = "upstream"

[[derive.step]]
kind = "generate"
command = "exit 3"
"#,
        );
        let err = assemble(root, &failing).unwrap_err();
        assert!(matches!(err, AssembleError::GenerateFailed { .. }));
        // Old tree still stands (still linking v1's path, now reading v2 —
        // but crucially, present and complete) and no debris is left behind.
        assert!(root.join("build/tree/a.txt").exists());
        assert!(!root.join("build/.tree.assembling").exists());
    }

    #[test]
    fn reassembly_forgets_files_from_the_previous_run() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write(&root.join("upstream/a.txt"), "a");
        write(&root.join("upstream/b.txt"), "b");
        let config = project(
            r#"
[derive]
base = "upstream"
"#,
        );
        assemble(root, &config).unwrap();
        std::fs::remove_file(root.join("upstream/b.txt")).unwrap();
        assemble(root, &config).unwrap();
        assert!(!root.join("build/tree/b.txt").exists());
    }

    #[test]
    fn copied_outputs_are_real_files_not_links() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write(&root.join("layer/site/img/a.png"), "png-bytes");
        let linked = root.join("tree/site");
        std::fs::create_dir_all(&linked).unwrap();
        std::os::unix::fs::symlink(root.join("layer/site/img"), linked.join("img")).unwrap();
        std::fs::write(linked.join("index.html"), "<html>").unwrap();

        copy_dereferenced(&root.join("tree/site"), &root.join("dist/site")).unwrap();
        let out = root.join("dist/site");
        assert_eq!(read(&out.join("index.html")), "<html>");
        assert!(!out.join("img").is_symlink());
        assert_eq!(read(&out.join("img/a.png")), "png-bytes");
    }

    #[test]
    fn missing_non_mirror_overlay_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write(&root.join("upstream/a.txt"), "x");
        let config = project(
            r#"
[derive]
base = "upstream"

[[derive.overlay]]
path = "assets"
"#,
        );
        assert!(matches!(
            assemble(root, &config),
            Err(AssembleError::MissingOverlay(_))
        ));
    }
}
