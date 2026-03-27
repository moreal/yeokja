use anyhow::Result;
use clap::Parser;
use std::path::PathBuf;
use std::sync::Arc;
use tracing_subscriber::EnvFilter;
use tokio::sync::RwLock;
use yeokja_core::project::ProjectContext;
use yeokja_server::api::create_router;
use yeokja_server::state::AppState;

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

    let ctx = ProjectContext::load()?;

    let port = ctx.config.server.as_ref().map(|s| s.port).unwrap_or(3000);

    let state = Arc::new(AppState {
        config: ctx.config,
        glossary: Arc::new(RwLock::new(ctx.glossary)),
    });

    let app = create_router(state);

    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{}", port)).await?;
    tracing::info!(port, "Server running");
    axum::serve(listener, app).await?;

    Ok(())
}
