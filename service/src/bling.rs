//! Bling API v3: OAuth refresh + product lookup by codigo + midia attach via URL.

use crate::config::Config;
use crate::error::{AppError, AppResult};
use base64::{Engine as _, engine::general_purpose::STANDARD as B64};
use parking_lot::Mutex;
use reqwest::Client;
use serde::Deserialize;
use serde_json::Value;
use std::sync::Arc;
use std::time::Instant;

#[derive(Clone)]
pub struct BlingClient {
    http: Client,
    cfg: Config,
    token: Arc<Mutex<CachedToken>>,
}

struct CachedToken {
    access_token: String,
    expires_at: Instant,
    /// Bling may rotate refresh_token; keep latest in memory for this process.
    refresh_token: String,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct BlingProduct {
    pub id: u64,
    pub nome: String,
    pub codigo: String,
}

impl BlingClient {
    pub fn new(http: Client, cfg: Config) -> Self {
        let refresh = cfg.bling_refresh_token.clone();
        Self {
            http,
            cfg,
            token: Arc::new(Mutex::new(CachedToken {
                access_token: String::new(),
                expires_at: Instant::now(),
                refresh_token: refresh,
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
        self.refresh().await
    }

    async fn refresh(&self) -> AppResult<String> {
        #[derive(Deserialize)]
        struct Tok {
            access_token: String,
            expires_in: Option<u64>,
            refresh_token: Option<String>,
        }

        let refresh_token = self.token.lock().refresh_token.clone();
        let basic = B64.encode(format!(
            "{}:{}",
            self.cfg.bling_client_id, self.cfg.bling_client_secret
        ));

        let resp = self
            .http
            .post(&self.cfg.bling_token_url)
            .header("Authorization", format!("Basic {basic}"))
            .header("Content-Type", "application/x-www-form-urlencoded")
            .header("Accept", "application/json")
            .body(format!(
                "grant_type=refresh_token&refresh_token={}",
                urlencoding::encode(&refresh_token)
            ))
            .send()
            .await?;

        let status = resp.status();
        let body = resp.text().await?;
        if !status.is_success() {
            return Err(AppError::msg(format!(
                "Bling token refresh failed ({status}): {body}"
            )));
        }
        let t: Tok = serde_json::from_str(&body)?;
        let expires = t.expires_in.unwrap_or(21600);
        let mut guard = self.token.lock();
        guard.access_token = t.access_token.clone();
        guard.expires_at =
            Instant::now() + std::time::Duration::from_secs(expires.saturating_sub(120));
        if let Some(rt) = t.refresh_token {
            if !rt.is_empty() {
                guard.refresh_token = rt;
            }
        }
        Ok(t.access_token)
    }

    pub async fn find_by_codigo(&self, codigo: &str) -> AppResult<Option<BlingProduct>> {
        let token = self.access_token().await?;
        let url = format!("{}/produtos", self.cfg.bling_api_base);
        let resp = self
            .http
            .get(&url)
            .bearer_auth(&token)
            .header("Accept", "application/json")
            .query(&[
                ("codigos[]", codigo),
                ("criterio", "5"),
                ("tipo", "T"),
                ("limite", "10"),
            ])
            .send()
            .await?;

        let status = resp.status();
        let body = resp.text().await?;
        if !status.is_success() {
            return Err(AppError::msg(format!(
                "Bling GET produtos failed ({status}): {body}"
            )));
        }
        let v: Value = serde_json::from_str(&body)?;
        let Some(arr) = v.get("data").and_then(|d| d.as_array()) else {
            return Ok(None);
        };
        for item in arr {
            let c = item
                .get("codigo")
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .to_string();
            if c.eq_ignore_ascii_case(codigo) {
                let id = item
                    .get("id")
                    .and_then(|x| x.as_u64())
                    .ok_or_else(|| AppError::msg("produto sem id"))?;
                let nome = item
                    .get("nome")
                    .and_then(|x| x.as_str())
                    .unwrap_or("")
                    .to_string();
                return Ok(Some(BlingProduct { id, nome, codigo: c }));
            }
        }
        // fallback: first match if Bling returns filtered already
        if let Some(item) = arr.first() {
            let id = item.get("id").and_then(|x| x.as_u64());
            if let Some(id) = id {
                return Ok(Some(BlingProduct {
                    id,
                    nome: item
                        .get("nome")
                        .and_then(|x| x.as_str())
                        .unwrap_or("")
                        .into(),
                    codigo: item
                        .get("codigo")
                        .and_then(|x| x.as_str())
                        .unwrap_or(codigo)
                        .into(),
                }));
            }
        }
        Ok(None)
    }

    /// Attach image via public URL using PATCH midia.imagens.imagensURL (OpenAPI writeOnly).
    pub async fn attach_image_url(&self, product_id: u64, image_url: &str) -> AppResult<()> {
        let token = self.access_token().await?;
        let url = format!("{}/produtos/{product_id}", self.cfg.bling_api_base);

        // video is required by ProdutosMidiaDTO schema
        let payload = serde_json::json!({
            "midia": {
                "video": { "url": "" },
                "imagens": {
                    "imagensURL": [
                        { "link": image_url }
                    ]
                }
            }
        });

        let resp = self
            .http
            .patch(&url)
            .bearer_auth(&token)
            .header("Accept", "application/json")
            .header("Content-Type", "application/json")
            .json(&payload)
            .send()
            .await?;

        let status = resp.status();
        let body = resp.text().await?;
        if !status.is_success() {
            // fallback: try PUT with same midia fragment (some contas reject PATCH)
            let resp2 = self
                .http
                .put(&url)
                .bearer_auth(&token)
                .header("Accept", "application/json")
                .header("Content-Type", "application/json")
                .json(&payload)
                .send()
                .await?;
            let status2 = resp2.status();
            let body2 = resp2.text().await?;
            if !status2.is_success() {
                return Err(AppError::msg(format!(
                    "Bling attach midia failed PATCH({status}): {body} | PUT({status2}): {body2}"
                )));
            }
        }
        Ok(())
    }
}
