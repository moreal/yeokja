use anyhow::Result;
use clap::Parser;
use std::path::PathBuf;
use tracing_subscriber::EnvFilter;

#[derive(Parser)]
#[command(name = "yeokja-server", about = "Yeokja translation server")]
struct Cli {
    /// Working directory (defaults to current directory)
    #[arg(long = "working-directory", short = 'C')]
    working_directory: Option<PathBuf>,

    /// Increase log verbosity (-v: debug, -vv: trace)
    #[arg(short = 'v', long = "verbose", action = clap::ArgAction::Count)]
    verbose: u8,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    let default_filter = match cli.verbose {
        0 => "yeokja=info",
        1 => "yeokja=debug",
        _ => "yeokja=trace",
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

    yeokja_server::serve().await
}
