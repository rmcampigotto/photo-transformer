use crate::bling::BlingClient;
use crate::config::Config;
use crate::drive::DriveClient;
use crate::error::{AppError, AppResult};
use crate::sync::{MediaStore, SyncState, run_sync};
use axum::{
    Json, Router,
    body::Body,
    extract::{Path, State},
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use serde::Deserialize;
use serde_json::json;
use std::sync::Arc;

#[derive(Clone)]
pub struct AppState {
    pub cfg: Config,
    pub drive: DriveClient,
    pub bling: BlingClient,
    pub media: MediaStore,
    pub sync: SyncState,
}

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/sync", post(start_sync))
        .route("/jobs", post(start_sync))
        .route("/status", get(status))
        .route("/media/{id}", get(serve_media))
        .with_state(Arc::new(state))
}

async fn health() -> impl IntoResponse {
    Json(json!({ "status": "ok", "service": "photo-transformer-service" }))
}

async fn status(State(st): State<Arc<AppState>>) -> impl IntoResponse {
    Json(st.sync.snapshot())
}

#[derive(Debug, Deserialize, Default)]
struct SyncBody {
    /// Optional override folder (defaults to GOOGLE_DRIVE_FOLDER_ID)
    folder_id: Option<String>,
}

async fn start_sync(
    State(st): State<Arc<AppState>>,
    headers: HeaderMap,
    body: Option<Json<SyncBody>>,
) -> AppResult<impl IntoResponse> {
    check_api_key(&st.cfg, &headers)?;
    let _body = body.map(|j| j.0).unwrap_or_default();
    // folder override: mutate a local cfg clone if provided
    let mut cfg = st.cfg.clone();
    if let Some(fid) = _body.folder_id {
        if !fid.is_empty() {
            cfg.drive_folder_id = fid;
        }
    }

    let cfg2 = cfg.clone();
    let drive = st.drive.clone();
    let bling = st.bling.clone();
    let media = st.media.clone();
    let sync = st.sync.clone();

    // run in background so HTTP returns quickly
    tokio::spawn(async move {
        if let Err(e) = run_sync(&cfg2, &drive, &bling, &media, &sync).await {
            tracing::error!(error = %e, "sync task error");
        }
    });

    Ok((
        StatusCode::ACCEPTED,
        Json(json!({
            "message": "sync started",
            "status_url": "/status"
        })),
    ))
}

async fn serve_media(
    State(st): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Response, AppError> {
    let key = id.trim_end_matches(".jpg").trim_end_matches(".jpeg");
    let bytes = st
        .media
        .get(key)
        .ok_or_else(|| AppError::msg("media not found or expired"))?;
    Ok(Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "image/jpeg")
        .header(header::CACHE_CONTROL, "public, max-age=3600")
        .body(Body::from(bytes))
        .unwrap())
}

fn check_api_key(cfg: &Config, headers: &HeaderMap) -> AppResult<()> {
    let Some(expected) = &cfg.sync_api_key else {
        return Ok(());
    };
    let provided = headers
        .get("x-api-key")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string())
        .or_else(|| {
            headers
                .get(header::AUTHORIZATION)
                .and_then(|v| v.to_str().ok())
                .and_then(|s| s.strip_prefix("Bearer ").map(|x| x.to_string()))
        });
    match provided {
        Some(p) if p == *expected => Ok(()),
        _ => Err(AppError::msg("unauthorized: missing or invalid API key")),
    }
}
