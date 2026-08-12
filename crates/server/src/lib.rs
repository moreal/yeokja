pub mod api;
pub mod state;

use anyhow::Result;
use std::sync::Arc;
use yeokja_core::project::ProjectContext;

/// Load the project from the current directory and serve the API.
pub async fn serve() -> Result<()> {
    let ctx = ProjectContext::load()?;
    let port = ctx.config.server.as_ref().map(|s| s.port).unwrap_or(3000);

    let state = Arc::new(state::AppState::new(ctx.config, ctx.glossary));
    let app = api::create_router(state);

    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{port}")).await?;
    tracing::info!(port, "Server running");
    axum::serve(listener, app).await?;
    Ok(())
}
