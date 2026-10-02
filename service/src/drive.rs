//! Google Drive list/download (service account JWT or OAuth refresh).

use crate::config::{Config, GoogleAuth};
use crate::error::{AppError, AppResult};
use crate::image_proc::is_image_mime;
use chrono::{Duration, Utc};
use jsonwebtoken::{Algorithm, EncodingKey, Header};
use parking_lot::Mutex;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;
use std::time::Instant;

const TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
const DRIVE_FILES: &str = "https://www.googleapis.com/drive/v3/files";
const SCOPE: &str = "https://www.googleapis.com/auth/drive";

#[derive(Clone)]
pub struct DriveClient {
    http: Client,
    cfg: Config,
    token: Arc<Mutex<CachedToken>>,
}

struct CachedToken {
    access_token: String,
    expires_at: Instant,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct DriveFile {
    pub id: String,
    pub name: String,
    pub mime_type: String,
}

impl DriveClient {
    pub fn new(http: Client, cfg: Config) -> Self {
        Self {
            http,
            cfg,
            token: Arc::new(Mutex::new(CachedToken {
                access_token: String::new(),
                expires_at: Instant::now(),
            })),
        }
    }

    async fn access_token(&self) -> AppResult<String> {
        {
            let guard = self.token.lock();
            if !guard.access_token.is_empty() && Instant::now() < guard.expires_at {
                return Ok(guard.access_token.clone());
            }
        }
        let (token, expires_in) = match &self.cfg.google_auth {
            GoogleAuth::ServiceAccount { json_path } => {
                self.service_account_token(json_path).await?
            }
            GoogleAuth::OAuth {
                client_id,
                client_secret,
                refresh_token,
            } => {
                self.oauth_refresh(client_id, client_secret, refresh_token)
                    .await?
            }
        };
        let mut guard = self.token.lock();
        guard.access_token = token.clone();
        guard.expires_at = Instant::now() + std::time::Duration::from_secs(expires_in.saturating_sub(60));
        Ok(token)
    }

    async fn oauth_refresh(
        &self,
        client_id: &str,
        client_secret: &str,
        refresh_token: &str,
    ) -> AppResult<(String, u64)> {
        #[derive(Deserialize)]
        struct Tok {
            access_token: String,
            expires_in: u64,
        }
        let resp = self
            .http
            .post(TOKEN_URL)
            .form(&[
                ("client_id", client_id),
                ("client_secret", client_secret),
                ("refresh_token", refresh_token),
                ("grant_type", "refresh_token"),
            ])
            .send()
            .await?
            .error_for_status()?;
        let t: Tok = resp.json().await?;
        Ok((t.access_token, t.expires_in))
    }

    async fn service_account_token(&self, path: &std::path::Path) -> AppResult<(String, u64)> {
        #[derive(Deserialize)]
        struct Sa {
            client_email: String,
            private_key: String,
            token_uri: Option<String>,
        }
        #[derive(Serialize)]
        struct Claims {
            iss: String,
            scope: String,
            aud: String,
            iat: i64,
            exp: i64,
        }
        #[derive(Deserialize)]
        struct Tok {
            access_token: String,
            expires_in: u64,
        }

        let raw = std::fs::read_to_string(path)
            .map_err(|e| AppError::msg(format!("read service account JSON: {e}")))?;
        let sa: Sa = serde_json::from_str(&raw)?;
        let now = Utc::now();
        let claims = Claims {
            iss: sa.client_email,
            scope: SCOPE.into(),
            aud: sa.token_uri.unwrap_or_else(|| TOKEN_URL.into()),
            iat: now.timestamp(),
            exp: (now + Duration::minutes(55)).timestamp(),
        };
        let key = EncodingKey::from_rsa_pem(sa.private_key.as_bytes())?;
        let assertion = jsonwebtoken::encode(&Header::new(Algorithm::RS256), &claims, &key)?;

        let resp = self
            .http
            .post(TOKEN_URL)
            .form(&[
                ("grant_type", "urn:ietf:params:oauth:grant-type:jwt-bearer"),
                ("assertion", &assertion),
            ])
            .send()
            .await?
            .error_for_status()?;
        let t: Tok = resp.json().await?;
        Ok((t.access_token, t.expires_in))
    }

    pub async fn list_images(&self, folder_id: &str) -> AppResult<Vec<DriveFile>> {
        let token = self.access_token().await?;
        let mut out = Vec::new();
        let mut page_token: Option<String> = None;
        let q = format!(
            "'{folder_id}' in parents and trashed=false and mimeType contains 'image/'"
        );

        loop {
            let mut req = self
                .http
                .get(DRIVE_FILES)
                .bearer_auth(&token)
                .query(&[
                    ("q", q.as_str()),
                    ("fields", "nextPageToken,files(id,name,mimeType)"),
                    ("pageSize", "100"),
                    (
                        "supportsAllDrives",
                        bool_str(self.cfg.supports_all_drives),
                    ),
                    (
                        "includeItemsFromAllDrives",
                        bool_str(self.cfg.supports_all_drives),
                    ),
                    ("corpora", if self.cfg.supports_all_drives { "allDrives" } else { "user" }),
                ]);
            if let Some(pt) = &page_token {
                req = req.query(&[("pageToken", pt.as_str())]);
            }
            let resp: Value = req.send().await?.error_for_status()?.json().await?;
            if let Some(files) = resp.get("files").and_then(|f| f.as_array()) {
                for f in files {
                    let mime = f
                        .get("mimeType")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    if !is_image_mime(&mime) {
                        continue;
                    }
                    out.push(DriveFile {
                        id: f.get("id").and_then(|v| v.as_str()).unwrap_or("").into(),
                        name: f.get("name").and_then(|v| v.as_str()).unwrap_or("").into(),
                        mime_type: mime,
                    });
                }
            }
            page_token = resp
                .get("nextPageToken")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            if page_token.is_none() {
                break;
            }
        }
        out.retain(|f| !f.id.is_empty());
        Ok(out)
    }

    pub async fn download(&self, file_id: &str) -> AppResult<Vec<u8>> {
        let token = self.access_token().await?;
        let bytes = self
            .http
            .get(format!("{DRIVE_FILES}/{file_id}"))
            .bearer_auth(&token)
            .query(&[
                ("alt", "media"),
                (
                    "supportsAllDrives",
                    bool_str(self.cfg.supports_all_drives),
                ),
            ])
            .send()
            .await?
            .error_for_status()?
            .bytes()
            .await?;
        Ok(bytes.to_vec())
    }

    /// Upload processed JPEG and (optionally) make it readable by link for Bling.
    pub async fn upload_jpeg(
        &self,
        parent_id: &str,
        name: &str,
        jpeg: &[u8],
        make_public: bool,
    ) -> AppResult<String> {
        let token = self.access_token().await?;
        let metadata = serde_json::json!({
            "name": name,
            "parents": [parent_id],
            "mimeType": "image/jpeg",
        });

        let boundary = format!("boundary_{}", uuid::Uuid::new_v4());
        let mut body = Vec::new();
        body.extend_from_slice(
            format!("--{boundary}\r\nContent-Type: application/json; charset=UTF-8\r\n\r\n")
                .as_bytes(),
        );
        body.extend_from_slice(metadata.to_string().as_bytes());
        body.extend_from_slice(
            format!("\r\n--{boundary}\r\nContent-Type: image/jpeg\r\n\r\n").as_bytes(),
        );
        body.extend_from_slice(jpeg);
        body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());

        let resp: Value = self
            .http
            .post("https://www.googleapis.com/upload/drive/v3/files")
            .bearer_auth(&token)
            .query(&[
                ("uploadType", "multipart"),
                (
                    "supportsAllDrives",
                    bool_str(self.cfg.supports_all_drives),
                ),
                ("fields", "id,webContentLink,webViewLink"),
            ])
            .header(
                "Content-Type",
                format!("multipart/related; boundary={boundary}"),
            )
            .body(body)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;

        let id = resp
            .get("id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| AppError::msg("Drive upload missing id"))?
            .to_string();

        if make_public {
            let _ = self
                .http
                .post(format!("{DRIVE_FILES}/{id}/permissions"))
                .bearer_auth(&token)
                .query(&[("supportsAllDrives", bool_str(self.cfg.supports_all_drives))])
                .json(&serde_json::json!({
                    "role": "reader",
                    "type": "anyone"
                }))
                .send()
                .await?
                .error_for_status()?;
        }

        Ok(id)
    }

    #[allow(dead_code)]
    pub fn public_uc_url(file_id: &str) -> String {
        format!("https://drive.google.com/uc?export=view&id={file_id}")
    }
}

fn bool_str(v: bool) -> &'static str {
    if v { "true" } else { "false" }
}
