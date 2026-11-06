//! Icon model for the shared icon library.

/// Maximum icon file size, in megabytes as the pages say it.
pub const MAX_ICON_SIZE_MB: usize = 1;

pub const MAX_ICON_SIZE: usize = MAX_ICON_SIZE_MB * 1024 * 1024;

pub const MAX_ICON_DIMENSION: u32 = 128;

/// An uploaded icon in the shared library.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct Icon {
    pub id: i64,
    pub filename: String,
    pub original_name: String,
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
        };
        assert_eq!(icon.url(), "/uploads/icons/abc-123.png");
    }
}
