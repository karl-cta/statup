//! The instance's own logo: one uploaded image that takes the place of the
//! Statup mark in the masthead. It lives apart from the icon library, so
//! removing icons never takes it away.

use std::path::{Path, PathBuf};

use crate::db::DbPool;
use crate::error::AppError;
use crate::repositories::SettingsRepository;
use crate::services::{mime_to_extension, prepare_image};

/// Settings key of the logo's file name.
pub const LOGO_SETTING: &str = "instance_logo";

/// Twice the masthead's height and more, sharp on a dense screen.
const MAX_LOGO_DIMENSION: u32 = 256;

pub struct LogoService;

impl LogoService {
    /// Checks and stores `data` as the logo, then removes the one it
    /// replaces.
    pub async fn replace(pool: &DbPool, upload_dir: &str, data: Vec<u8>) -> Result<(), AppError> {
        if data.is_empty() {
            return Err(AppError::Validation("validation.file_empty".to_string()));
        }
        let (mime, image) = prepare_image(data, MAX_LOGO_DIMENSION).await?;
        let filename = format!("logo-{}.{}", uuid::Uuid::new_v4(), mime_to_extension(mime));
        let dir = logo_dir(upload_dir);
        tokio::fs::create_dir_all(&dir).await.map_err(|e| {
            AppError::Internal(anyhow::anyhow!("cannot create the logo folder: {e}"))
        })?;
        tokio::fs::write(dir.join(&filename), &image)
            .await
            .map_err(|e| AppError::Internal(anyhow::anyhow!("failed to write the logo: {e}")))?;
        let previous = SettingsRepository::get(pool, LOGO_SETTING).await?;
        SettingsRepository::set(pool, LOGO_SETTING, &filename).await?;
        crate::set_instance_logo(&filename);
        remove_file(upload_dir, previous).await;
        Ok(())
    }

    /// Brings the Statup mark back.
    pub async fn remove(pool: &DbPool, upload_dir: &str) -> Result<(), AppError> {
        let previous = SettingsRepository::get(pool, LOGO_SETTING).await?;
        SettingsRepository::set(pool, LOGO_SETTING, "").await?;
        crate::set_instance_logo("");
        remove_file(upload_dir, previous).await;
        Ok(())
    }
}

/// Where the logo lives under the upload directory.
pub fn logo_dir(upload_dir: &str) -> PathBuf {
    Path::new(upload_dir).join("brand")
}

/// A file left behind costs only disk space: a failure is not reported.
async fn remove_file(upload_dir: &str, filename: Option<String>) {
    if let Some(name) = filename.filter(|name| !name.is_empty()) {
        let _ = tokio::fs::remove_file(logo_dir(upload_dir).join(name)).await;
    }
}
