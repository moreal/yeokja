mod commands;
mod tui;

use clap::{Parser, Subcommand};
use std::path::PathBuf;
use tracing_subscriber::EnvFilter;

#[derive(Parser)]
#[command(name = "yeokja", about = "Document translation tool")]
struct Cli {
    /// Working directory (defaults to current directory)
    #[arg(long = "working-directory", short = 'C', global = true)]
    working_directory: Option<PathBuf>,

    /// Increase log verbosity (-v: debug, -vv: trace)
    #[arg(short = 'v', long = "verbose", action = clap::ArgAction::Count, global = true)]
    verbose: u8,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Translate documents
    Translate {
        /// Path to translate
        path: String,
        /// Show TUI progress view
        #[arg(long)]
        tui: bool,
    },
    /// Show translation status
    Status {
        /// Path to check
        path: String,
        /// Exit nonzero if any segment still needs translation (CI gate)
        #[arg(long)]
        check: bool,
    },
    /// List tables and how selection rules treat them
    Inspect {
        /// Path to inspect
        path: String,
    },
    /// Show which parts of the source the parser passed over
    Coverage {
        /// Path to measure
        path: String,
        /// Smallest run of lines worth reporting
        #[arg(long, default_value_t = 5)]
        min_lines: usize,
    },
    /// Manage glossary
    Glossary {
        #[command(subcommand)]
        action: GlossaryAction,
    },
    /// Evaluate existing translations
    Evaluate {
        /// Path to evaluate
        path: String,
    },
    /// Assemble the buildable tree from base, overlays, and steps
    Assemble,
    /// Assemble, run the build command, and copy outputs to dist
    Build,
    /// List state files whose source is gone; optionally delete them
    Orphans {
        /// Delete unmatched orphans and their derived outputs
        #[arg(long)]
        delete: bool,
    },
    /// Start server mode
    Serve,
}

#[derive(Subcommand)]
enum GlossaryAction {
    /// List all glossary terms
    List,
    /// Set a glossary term
    Set {
        /// Term in source language
        term: String,
        /// Translation
        translation: String,
    },
    /// Remove a glossary term
    Remove {
        /// Term in source language
        term: String,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    // The TUI owns the terminal; suppress log output so it does not corrupt the view.
    let tui_mode = matches!(cli.command, Commands::Translate { tui: true, .. });
    let default_filter = if tui_mode {
        "off"
    } else {
        match cli.verbose {
            0 => "yeokja=info",
            1 => "yeokja=debug",
            _ => "yeokja=trace",
        }
    };

    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new(default_filter))
        )
        .init();

    if let Some(dir) = &cli.working_directory {
        std::env::set_current_dir(dir)
            .map_err(|e| anyhow::anyhow!("Failed to change working directory to {}: {}", dir.display(), e))?;
    }

    match cli.command {
        Commands::Translate { path, tui: use_tui } => {
            if use_tui {
                translate_with_tui(&path).await?;
            } else {
                commands::translate::run(&path, None).await?;
            }
        }
        Commands::Inspect { path } => {
            commands::inspect::run(&path)?;
        }
        Commands::Coverage { path, min_lines } => {
            commands::coverage::run(&path, min_lines)?;
        }
        Commands::Status { path, check } => {
            commands::status::run(&path, check)?;
        }
        Commands::Glossary { action } => match action {
            GlossaryAction::List => commands::glossary::list()?,
            GlossaryAction::Set { term, translation } => {
                commands::glossary::set(&term, &translation)?;
            }
            GlossaryAction::Remove { term } => {
                commands::glossary::remove(&term)?;
            }
        },
        Commands::Evaluate { path } => {
            commands::evaluate::run(&path).await?;
        }
        Commands::Assemble => {
            commands::assemble::run()?;
        }
        Commands::Build => {
            commands::build::run()?;
        }
        Commands::Orphans { delete } => {
            commands::orphans::run(delete)?;
        }
        Commands::Serve => {
            commands::serve::run().await?;
        }
    }

    Ok(())
}

/// Run the translation with a live ratatui progress view.
/// The translation runs as a tokio task; the TUI loop runs on a blocking
/// thread and reads shared progress state fed by the event consumer.
async fn translate_with_tui(path: &str) -> anyhow::Result<()> {
    let progress = tui::create_shared_progress();
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();

    let consumer = tokio::spawn(tui::consume_progress_events(rx, progress.clone()));

    let path_owned = path.to_string();
    let translate_task =
        tokio::spawn(async move { commands::translate::run(&path_owned, Some(tx)).await });

    let tui_progress = progress.clone();
    let cancelled = tokio::task::spawn_blocking(move || tui::run_tui(tui_progress)).await??;

    if cancelled {
        translate_task.abort();
        println!("Translation cancelled. Completed blocks were saved and will be reused.");
    }

    match translate_task.await {
        Ok(result) => {
            let outcome = result?;
            if !cancelled {
                println!(
                    "Translated {} segment(s) across {} file(s); {} file(s) failed.",
                    outcome.segments_translated, outcome.files_processed, outcome.files_failed
                );
            }
        }
        Err(e) if e.is_cancelled() => {}
        Err(e) => return Err(e.into()),
    }

    let _ = consumer.await;
    Ok(())
}
