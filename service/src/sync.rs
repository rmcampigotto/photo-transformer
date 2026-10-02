use crate::bling::BlingClient;
use crate::config::Config;
use crate::drive::DriveClient;
use crate::error::{AppError, AppResult};
use crate::image_proc::{contain_to_square_jpeg, sku_from_filename};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;
use uuid::Uuid;

#[derive(Clone, Default)]
pub struct MediaStore {
    inner: Arc<Mutex<HashMap<String, StoredMedia>>>,
}

#[derive(Clone)]
struct StoredMedia {
    bytes: Vec<u8>,
    created: Instant,
}

impl MediaStore {
    pub fn put(&self, bytes: Vec<u8>) -> String {
        let id = Uuid::new_v4().to_string();
        let mut g = self.inner.lock();
        // simple cleanup: drop entries older than 2h
        g.retain(|_, v| v.created.elapsed().as_secs() < 7200);
        g.insert(
            id.clone(),
            StoredMedia {
                bytes,
                created: Instant::now(),
            },
        );
        id
    }

    pub fn get(&self, id: &str) -> Option<Vec<u8>> {
        self.inner.lock().get(id).map(|m| m.bytes.clone())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SyncStatus {
    pub running: bool,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
    pub last_error: Option<String>,
    pub processed: usize,
    pub attached: usize,
    pub skipped: usize,
    pub failed: usize,
    pub items: Vec<SyncItemResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncItemResult {
    pub drive_name: String,
    pub sku: String,
    pub status: String,
    pub detail: Option<String>,
    pub product_id: Option<u64>,
    pub image_url: Option<String>,
}

#[derive(Clone)]
pub struct SyncState {
    pub status: Arc<Mutex<SyncStatus>>,
}

impl SyncState {
    pub fn new() -> Self {
        Self {
            status: Arc::new(Mutex::new(SyncStatus::default())),
        }
    }

    pub fn snapshot(&self) -> SyncStatus {
        self.status.lock().clone()
    }
}

pub async fn run_sync(
    cfg: &Config,
    drive: &DriveClient,
    bling: &BlingClient,
    media: &MediaStore,
    state: &SyncState,
) -> AppResult<SyncStatus> {
    {
        let mut st = state.status.lock();
        if st.running {
            return Err(AppError::msg("sync already running"));
        }
        *st = SyncStatus {
            running: true,
            started_at: Some(chrono::Local::now().to_rfc3339()),
            ..SyncStatus::default()
        };
        st.running = true;
    }

    let result = do_sync(cfg, drive, bling, media, state).await;

    let mut st = state.status.lock();
    st.running = false;
    st.finished_at = Some(chrono::Local::now().to_rfc3339());
    if let Err(ref e) = result {
        st.last_error = Some(e.to_string());
    }
    Ok(st.clone())
}

async fn do_sync(
    cfg: &Config,
    drive: &DriveClient,
    bling: &BlingClient,
    media: &MediaStore,
    state: &SyncState,
) -> AppResult<()> {
    let mut files = drive.list_images(&cfg.drive_folder_id).await?;
    if cfg.sync_max_files > 0 && files.len() > cfg.sync_max_files {
        files.truncate(cfg.sync_max_files);
    }
    tracing::info!(count = files.len(), "Drive images listed");

    for file in files {
        let sku = sku_from_filename(&file.name);
        let mut item = SyncItemResult {
            drive_name: file.name.clone(),
            sku: sku.clone(),
            status: "pending".into(),
            detail: None,
            product_id: None,
            image_url: None,
        };

        let outcome = process_one(cfg, drive, bling, media, &file.id, &file.name, &sku).await;
        match outcome {
            Ok((product_id, url)) => {
                item.status = "ok".into();
                item.product_id = Some(product_id);
                item.image_url = Some(url);
                let mut st = state.status.lock();
                st.processed += 1;
                st.attached += 1;
                st.items.push(item);
            }
            Err(e) => {
                let msg = e.to_string();
                let skipped = msg.contains("produto não encontrado");
                item.status = if skipped { "skipped" } else { "failed" }.into();
                item.detail = Some(msg);
                let mut st = state.status.lock();
                st.processed += 1;
                if skipped {
                    st.skipped += 1;
                } else {
                    st.failed += 1;
                }
                st.items.push(item);
            }
        }
    }
    Ok(())
}

async fn process_one(
    cfg: &Config,
    drive: &DriveClient,
    bling: &BlingClient,
    media: &MediaStore,
    file_id: &str,
    file_name: &str,
    sku: &str,
) -> AppResult<(u64, String)> {
    tracing::info!(file_name, sku, "processing");
    let raw = drive.download(file_id).await?;
    let jpeg = contain_to_square_jpeg(&raw, cfg.jpeg_quality)?;

    let media_id = media.put(jpeg.clone());
    let public_url = format!("{}/media/{media_id}.jpg", cfg.public_base_url);

    // Optional mirror to Drive output folder
    if let Some(out_folder) = &cfg.drive_output_folder_id {
        let out_name = format!("{sku}-1200x1200.jpg");
        match drive
            .upload_jpeg(out_folder, &out_name, &jpeg, true)
            .await
        {
            Ok(id) => {
                tracing::info!(drive_id = %id, "uploaded processed JPEG to Drive");
            }
            Err(e) => tracing::warn!(error = %e, "Drive output upload failed (continuing with PUBLIC_BASE_URL)"),
        }
    }

    let product = bling
        .find_by_codigo(sku)
        .await?
        .ok_or_else(|| AppError::msg(format!("produto não encontrado para codigo/SKU '{sku}'")))?;

    bling.attach_image_url(product.id, &public_url).await?;
    tracing::info!(
        product_id = product.id,
        sku,
        url = %public_url,
        "attached midia to Bling"
    );
    let _ = file_name;
    Ok((product.id, public_url))
}
