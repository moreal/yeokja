use anyhow::Result;

/// Start the API server in-process (same as running yeokja-server).
pub async fn run() -> Result<()> {
    yeokja_server::serve().await
}
