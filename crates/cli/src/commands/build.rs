use anyhow::{bail, Context, Result};
use std::path::Path;
use std::process::Command;
use yeokja_core::project::ProjectContext;

/// Assemble the tree, run the build command inside it, and copy the declared
/// outputs into dist. The tree is reassembled every time — it is disposable
/// by design, so a build never runs against a stale or hand-edited tree.
pub fn run() -> Result<()> {
    let ctx = ProjectContext::load()?;
    let Some(build) = ctx.config.build.clone() else {
        bail!("no [build] section in yeokja.toml");
    };

    let report = yeokja_assemble::assemble(Path::new("."), &ctx.config)?;
    let tree = report.target.clone();
    println!(
        "Assembled {} ({} linked, {} overlaid)",
        tree.display(),
        report.files_linked,
        report.files_overlaid
    );

    let root = std::fs::canonicalize(".")?;
    println!("Running: {}", build.command);
    let status = Command::new("sh")
        .arg("-c")
        .arg(&build.command)
        .current_dir(&tree)
        .env("YEOKJA_ROOT", &root)
        .status()
        .context("failed to launch build command")?;
    if !status.success() {
        bail!(
            "build command failed with {status}; the tree is left at {} for inspection",
            tree.display()
        );
    }

    let dist = root.join(&build.dist);
    for output in &build.outputs {
        let src = tree.join(output);
        if !src.exists() {
            bail!(
                "declared output {output} was not produced (looked at {})",
                src.display()
            );
        }
        let dest = dist.join(output);
        yeokja_assemble::copy_dereferenced(&src, &dest)?;
        println!("Output: {}", dest.display());
    }
    Ok(())
}
