mod api;
mod bling;
mod config;
mod drive;
mod error;
mod image_proc;
mod sync;

use api::{AppState, router};
use bling::BlingClient;
use config::Config;
use drive::DriveClient;
use sync::{MediaStore, SyncState};
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let cfg = Config::from_env().map_err(|e| anyhow::anyhow!(e))?;
    let http = reqwest::Client::builder()
        .user_agent("photo-transformer-service/0.1")
        .timeout(std::time::Duration::from_secs(120))
        .build()?;

    let state = AppState {
        drive: DriveClient::new(http.clone(), cfg.clone()),
        bling: BlingClient::new(http, cfg.clone()),
        media: MediaStore::default(),
        sync: SyncState::new(),
        cfg: cfg.clone(),
    };

    let app = router(state)
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http());

    let addr = format!("{}:{}", cfg.host, cfg.port);
    tracing::info!(%addr, public = %cfg.public_base_url, "listening");
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}
