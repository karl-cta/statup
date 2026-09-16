//! Icon model for the shared icon library.

use chrono::{DateTime, Utc};

/// Maximum icon file size: 256 KB.
pub const MAX_ICON_SIZE: usize = 256 * 1024;

/// Maximum icon dimension (width or height) after resize.
pub const MAX_ICON_DIMENSION: u32 = 128;

/// An uploaded icon in the shared library.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct Icon {
    pub id: i64,
    pub filename: String,
    pub original_name: String,
    pub mime_type: String,
    pub size_bytes: i64,
    pub uploaded_by: i64,
    pub created_at: DateTime<Utc>,
}

impl Icon {
    /// URL path to serve this icon.
    pub fn url(&self) -> String {
        format!("/uploads/icons/{}", self.filename)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn icon_url_returns_correct_path() {
        let icon = Icon {
            id: 1,
            filename: "abc-123.png".to_string(),
            original_name: "logo.png".to_string(),
            mime_type: "image/png".to_string(),
            size_bytes: 1024,
            uploaded_by: 1,
            created_at: Utc::now(),
        };
        assert_eq!(icon.url(), "/uploads/icons/abc-123.png");
    }
}
