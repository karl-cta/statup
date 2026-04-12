//! Icon service - upload validation, image processing, and file I/O.

use std::path::Path;

use crate::db::DbPool;
use crate::error::AppError;
use crate::models::{Icon, MAX_ICON_DIMENSION, MAX_ICON_SIZE};
use crate::repositories::IconRepository;

/// Business logic for icon upload, validation, and lifecycle.
pub struct IconService;

impl IconService {
    /// Upload a new icon: validate, process, save to disk, create DB record.
    pub async fn upload(
        pool: &DbPool,
        upload_dir: &str,
        data: &[u8],
        original_name: &str,
        user_id: i64,
    ) -> Result<Icon, AppError> {
        if data.is_empty() {
            return Err(AppError::Validation("validation.file_empty".to_string()));
        }

        if data.len() > MAX_ICON_SIZE {
            return Err(AppError::Validation(
                "validation.file_too_large".to_string(),
            ));
        }

        let mime = detect_mime(data)?;
        let extension = mime_to_extension(mime);
        let filename = format!("{}.{extension}", uuid::Uuid::new_v4());

        let processed = process_image(data, mime)?;

        let icons_dir = format!("{upload_dir}/icons");
        let file_path = format!("{icons_dir}/{filename}");
        tokio::fs::write(&file_path, &processed)
            .await
            .map_err(|e| AppError::Internal(anyhow::anyhow!("failed to write icon file: {e}")))?;

        #[allow(clippy::cast_possible_wrap)]
        let size_bytes = processed.len() as i64;

        let icon =
            match IconRepository::create(pool, &filename, original_name, mime, size_bytes, user_id)
                .await
            {
                Ok(icon) => icon,
                Err(e) => {
                    // Clean up the file if DB insert fails
                    let _ = tokio::fs::remove_file(&file_path).await;
                    return Err(e.into());
                }
            };

        Ok(icon)
    }

    /// Delete an icon: remove from DB and disk.
    pub async fn delete(pool: &DbPool, upload_dir: &str, id: i64) -> Result<(), AppError> {
        let icon = IconRepository::find_by_id(pool, id)
            .await?
            .ok_or(AppError::NotFound)?;

        if IconRepository::is_referenced(pool, id).await? {
            return Err(AppError::Validation("validation.icon_in_use".to_string()));
        }

        IconRepository::delete(pool, id).await?;

        let file_path = format!("{upload_dir}/icons/{}", icon.filename);
        if Path::new(&file_path).exists() {
            let _ = tokio::fs::remove_file(&file_path).await;
        }

        Ok(())
    }
}

/// Detect MIME type from file magic bytes.
fn detect_mime(data: &[u8]) -> Result<&'static str, AppError> {
    if data.len() >= 4 && data[..4] == [0x89, 0x50, 0x4E, 0x47] {
        return Ok("image/png");
    }
    if data.len() >= 3 && data[..3] == [0xFF, 0xD8, 0xFF] {
        return Ok("image/jpeg");
    }
    if data.len() >= 12 && data[..4] == *b"RIFF" && data[8..12] == *b"WEBP" {
        return Ok("image/webp");
    }
    if is_svg(data) {
        return Ok("image/svg+xml");
    }
    Err(AppError::Validation(
        "validation.unsupported_file_type".to_string(),
    ))
}

/// Check if the data looks like an SVG file.
fn is_svg(data: &[u8]) -> bool {
    let text = std::str::from_utf8(data).unwrap_or("");
    let trimmed = text.trim_start();
    // Skip BOM if present
    let trimmed = trimmed.strip_prefix('\u{feff}').unwrap_or(trimmed);
    trimmed.starts_with("<svg") || trimmed.starts_with("<?xml")
}

/// Map MIME type to file extension.
fn mime_to_extension(mime: &str) -> &'static str {
    match mime {
        "image/png" => "png",
        "image/jpeg" => "jpg",
        "image/webp" => "webp",
        "image/svg+xml" => "svg",
        _ => "bin",
    }
}

/// Process image data: resize raster images, sanitize SVGs.
fn process_image(data: &[u8], mime: &str) -> Result<Vec<u8>, AppError> {
    if mime == "image/svg+xml" {
        return sanitize_svg(data);
    }
    resize_raster(data, mime)
}

/// Sanitize an SVG file using ammonia to strip dangerous elements.
fn sanitize_svg(data: &[u8]) -> Result<Vec<u8>, AppError> {
    let text = std::str::from_utf8(data)
        .map_err(|_| AppError::Validation("validation.invalid_svg".to_string()))?;

    let sanitized = ammonia::Builder::new()
        .add_tags([
            "svg",
            "path",
            "g",
            "circle",
            "rect",
            "line",
            "polyline",
            "polygon",
            "ellipse",
            "defs",
            "use",
            "symbol",
            "title",
            "desc",
            "linearGradient",
            "radialGradient",
            "stop",
            "clipPath",
            "mask",
            "text",
            "tspan",
        ])
        .add_generic_attributes([
            "id",
            "class",
            "style",
            "viewBox",
            "xmlns",
            "fill",
            "stroke",
            "stroke-width",
            "stroke-linecap",
            "stroke-linejoin",
            "d",
            "cx",
            "cy",
            "r",
            "rx",
            "ry",
            "x",
            "y",
            "x1",
            "y1",
            "x2",
            "y2",
            "width",
            "height",
            "transform",
            "opacity",
            "fill-opacity",
            "stroke-opacity",
            "points",
            "offset",
            "stop-color",
            "stop-opacity",
            "gradientUnits",
            "gradientTransform",
            "clip-path",
            "mask",
            "font-size",
            "font-family",
            "text-anchor",
            "dominant-baseline",
            "xlink:href",
            "href",
        ])
        .clean(text)
        .to_string();

    Ok(sanitized.into_bytes())
}

/// Resize a raster image to fit within `MAX_ICON_DIMENSION` x `MAX_ICON_DIMENSION`.
fn resize_raster(data: &[u8], mime: &str) -> Result<Vec<u8>, AppError> {
    let img = image::load_from_memory(data)
        .map_err(|_| AppError::Validation("validation.image_read_error".to_string()))?;

    let (w, h) = (img.width(), img.height());

    let img = if w > MAX_ICON_DIMENSION || h > MAX_ICON_DIMENSION {
        img.thumbnail(MAX_ICON_DIMENSION, MAX_ICON_DIMENSION)
    } else {
        img
    };

    let mut buf = std::io::Cursor::new(Vec::new());
    let format = match mime {
        "image/jpeg" => image::ImageFormat::Jpeg,
        "image/webp" => image::ImageFormat::WebP,
        _ => image::ImageFormat::Png,
    };

    img.write_to(&mut buf, format)
        .map_err(|e| AppError::Internal(anyhow::anyhow!("failed to encode image: {e}")))?;

    Ok(buf.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_png() {
        let data = [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];
        assert_eq!(detect_mime(&data).unwrap(), "image/png");
    }

    #[test]
    fn detect_jpeg() {
        let data = [0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10];
        assert_eq!(detect_mime(&data).unwrap(), "image/jpeg");
    }

    #[test]
    fn detect_webp() {
        let mut data = vec![0u8; 12];
        data[..4].copy_from_slice(b"RIFF");
        data[8..12].copy_from_slice(b"WEBP");
        assert_eq!(detect_mime(&data).unwrap(), "image/webp");
    }

    #[test]
    fn detect_svg() {
        let data = b"<svg xmlns=\"http://www.w3.org/2000/svg\"></svg>";
        assert_eq!(detect_mime(data).unwrap(), "image/svg+xml");
    }

    #[test]
    fn detect_svg_with_xml_declaration() {
        let data = b"<?xml version=\"1.0\"?><svg></svg>";
        assert_eq!(detect_mime(data).unwrap(), "image/svg+xml");
    }

    #[test]
    fn detect_unknown_returns_error() {
        let data = [0x00, 0x01, 0x02, 0x03];
        assert!(detect_mime(&data).is_err());
    }

    #[test]
    fn sanitize_svg_strips_script() {
        let input = b"<svg><script>alert('xss')</script><rect/></svg>";
        let result = sanitize_svg(input).unwrap();
        let text = std::str::from_utf8(&result).unwrap();
        assert!(!text.contains("<script>"));
        assert!(text.contains("<rect"));
    }

    #[test]
    fn mime_extensions() {
        assert_eq!(mime_to_extension("image/png"), "png");
        assert_eq!(mime_to_extension("image/jpeg"), "jpg");
        assert_eq!(mime_to_extension("image/webp"), "webp");
        assert_eq!(mime_to_extension("image/svg+xml"), "svg");
    }
}
