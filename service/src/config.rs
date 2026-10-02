use crate::error::{AppError, AppResult};
use std::env;
use std::path::PathBuf;

#[derive(Clone, Debug)]
pub struct Config {
    pub host: String,
    pub port: u16,
    pub sync_api_key: Option<String>,
    pub public_base_url: String,
    pub drive_folder_id: String,
    pub supports_all_drives: bool,
    pub drive_output_folder_id: Option<String>,
    pub google_auth: GoogleAuth,
    pub bling_client_id: String,
    pub bling_client_secret: String,
    pub bling_refresh_token: String,
    pub bling_api_base: String,
    pub bling_token_url: String,
    pub jpeg_quality: u8,
    #[allow(dead_code)]
    pub bling_replace_media: bool,
    pub sync_max_files: usize,
}

#[derive(Clone, Debug)]
pub enum GoogleAuth {
    ServiceAccount { json_path: PathBuf },
    OAuth {
        client_id: String,
        client_secret: String,
        refresh_token: String,
    },
}

impl Config {
    pub fn from_env() -> AppResult<Self> {
        let _ = dotenvy::dotenv();

        let drive_folder_id = required("GOOGLE_DRIVE_FOLDER_ID")?;
        let public_base_url = env::var("PUBLIC_BASE_URL")
            .unwrap_or_else(|_| "http://127.0.0.1:8080".into())
            .trim_end_matches('/')
            .to_string();

        let google_auth = if let Ok(path) = env::var("GOOGLE_SERVICE_ACCOUNT_JSON") {
            if !path.trim().is_empty() {
                GoogleAuth::ServiceAccount {
                    json_path: PathBuf::from(path),
                }
            } else {
                oauth_from_env()?
            }
        } else {
            oauth_from_env()?
        };

        let jpeg_quality: u8 = env::var("JPEG_QUALITY")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(90)
            .clamp(50, 100);

        let sync_max_files = env::var("SYNC_MAX_FILES")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(0);

        Ok(Self {
            host: env::var("HOST").unwrap_or_else(|_| "0.0.0.0".into()),
            port: env::var("PORT")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(8080),
            sync_api_key: env::var("SYNC_API_KEY").ok().filter(|s| !s.is_empty()),
            public_base_url,
            drive_folder_id,
            supports_all_drives: env_bool("GOOGLE_SUPPORTS_ALL_DRIVES", true),
            drive_output_folder_id: env::var("GOOGLE_DRIVE_OUTPUT_FOLDER_ID")
                .ok()
                .filter(|s| !s.is_empty()),
            google_auth,
            bling_client_id: required("BLING_CLIENT_ID")?,
            bling_client_secret: required("BLING_CLIENT_SECRET")?,
            bling_refresh_token: required("BLING_REFRESH_TOKEN")?,
            bling_api_base: env::var("BLING_API_BASE")
                .unwrap_or_else(|_| "https://api.bling.com.br/Api/v3".into())
                .trim_end_matches('/')
                .to_string(),
            bling_token_url: env::var("BLING_TOKEN_URL").unwrap_or_else(|_| {
                "https://api.bling.com.br/Api/v3/oauth/token".into()
            }),
            jpeg_quality,
            bling_replace_media: env_bool("BLING_REPLACE_MEDIA", true),
            sync_max_files,
        })
    }
}

fn required(key: &str) -> AppResult<String> {
    env::var(key)
        .map_err(|_| AppError::msg(format!("env var {key} is required")))
        .map(|s| s.trim().to_string())
        .and_then(|s| {
            if s.is_empty() {
                Err(AppError::msg(format!("env var {key} is empty")))
            } else {
                Ok(s)
            }
        })
}

fn oauth_from_env() -> AppResult<GoogleAuth> {
    Ok(GoogleAuth::OAuth {
        client_id: required("GOOGLE_CLIENT_ID")?,
        client_secret: required("GOOGLE_CLIENT_SECRET")?,
        refresh_token: required("GOOGLE_REFRESH_TOKEN")?,
    })
}

fn env_bool(key: &str, default: bool) -> bool {
    match env::var(key) {
        Ok(v) => matches!(
            v.trim().to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "on"
        ),
        Err(_) => default,
    }
}
