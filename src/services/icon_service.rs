//! Icon service: upload checks, image processing and file storage.

use std::borrow::Cow;
use std::io::Cursor;
use std::path::{Path, PathBuf};

use image::{ImageFormat, ImageReader, Limits};
use tokio::sync::Semaphore;

use crate::db::DbPool;
use crate::error::AppError;
use crate::models::{Icon, MAX_ICON_DIMENSION, MAX_ICON_SIZE};
use crate::repositories::IconRepository;

/// Largest raster accepted before resizing, in pixels per side.
const MAX_DECODE_DIMENSION: u32 = 4096;

/// Largest buffer a decoder may allocate: a small file can declare a huge
/// canvas.
const MAX_DECODE_ALLOC: u64 = 64 * 1024 * 1024;

/// Decoding holds up to 64 MiB on a blocking thread: two at a time.
static IMAGE_PERMITS: Semaphore = Semaphore::const_new(2);

/// SVG elements kept by the sanitizer. No script, style, link, image,
/// filter, animation or foreign content.
const SVG_ELEMENTS: &[&str] = &[
    "svg",
    "g",
    "defs",
    "symbol",
    "use",
    "title",
    "desc",
    "path",
    "circle",
    "ellipse",
    "line",
    "polyline",
    "polygon",
    "rect",
    "text",
    "tspan",
    "linearGradient",
    "radialGradient",
    "stop",
    "clipPath",
    "mask",
];

/// Presentation and geometry attributes kept on every element. `style` is
/// kept because exporters write fills and strokes there: an uploaded file is
/// served with `default-src 'none'`, and an SVG shown as an image loads
/// nothing, so a style can neither run nor fetch anything.
const SVG_ATTRIBUTES: &[&str] = &[
    "id",
    "style",
    "transform",
    "opacity",
    "display",
    "visibility",
    "color",
    "fill",
    "fill-opacity",
    "fill-rule",
    "clip-rule",
    "clip-path",
    "mask",
    "stroke",
    "stroke-width",
    "stroke-linecap",
    "stroke-linejoin",
    "stroke-miterlimit",
    "stroke-dasharray",
    "stroke-dashoffset",
    "stroke-opacity",
    "vector-effect",
    "x",
    "y",
    "x1",
    "y1",
    "x2",
    "y2",
    "cx",
    "cy",
    "r",
    "rx",
    "ry",
    "fx",
    "fy",
    "dx",
    "dy",
    "width",
    "height",
    "d",
    "points",
    "pathLength",
    "viewBox",
    "preserveAspectRatio",
    "offset",
    "stop-color",
    "stop-opacity",
    "gradientUnits",
    "gradientTransform",
    "spreadMethod",
    "clipPathUnits",
    "maskUnits",
    "maskContentUnits",
    "font-family",
    "font-size",
    "font-weight",
    "font-style",
    "text-anchor",
    "dominant-baseline",
    "letter-spacing",
];

/// Elements whose `href` (or `xlink:href`) may point at another element of
/// the same file, and only there.
const SVG_REFERENCING_ELEMENTS: &[&str] = &["use", "linearGradient", "radialGradient"];

const SVG_NAMESPACE: &str = "http://www.w3.org/2000/svg";
const XLINK_NAMESPACE: &str = "http://www.w3.org/1999/xlink";

/// Business logic for icon upload, validation, and lifecycle.
pub struct IconService;

impl IconService {
    /// Uploaded icons that can be drawn: one whose file is gone is not
    /// offered.
    pub async fn choosable(pool: &DbPool, upload_dir: &str) -> Result<Vec<Icon>, AppError> {
        let mut icons = Vec::new();
        for icon in IconRepository::list_all(pool).await? {
            if file_exists(&icon_path(upload_dir, &icon.filename)).await {
                icons.push(icon);
            }
        }
        Ok(icons)
    }

    /// Upload a new icon: validate, process, save to disk, create DB record.
    pub async fn upload(
        pool: &DbPool,
        upload_dir: &str,
        data: Vec<u8>,
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

        let (mime, processed) = prepare_image(data, MAX_ICON_DIMENSION).await?;
        let filename = format!("{}.{}", uuid::Uuid::new_v4(), mime_to_extension(mime));
        let file_path = icon_path(upload_dir, &filename);
        tokio::fs::write(&file_path, &processed)
            .await
            .map_err(|e| AppError::Internal(anyhow::anyhow!("failed to write icon file: {e}")))?;

        let size_bytes = i64::try_from(processed.len()).unwrap_or(i64::MAX);
        match IconRepository::create(pool, &filename, original_name, mime, size_bytes, user_id)
            .await
        {
            Ok(icon) => Ok(icon),
            Err(e) => {
                let _ = tokio::fs::remove_file(&file_path).await;
                Err(e.into())
            }
        }
    }

    /// Delete an icon from the database and the disk, and return it.
    ///
    /// A row whose file is gone shows a broken image wherever it is used;
    /// removing it clears those references (ON DELETE SET NULL), which is
    /// the only way to clean up. A row with a file stays protected.
    pub async fn delete(pool: &DbPool, upload_dir: &str, id: i64) -> Result<Icon, AppError> {
        let icon = IconRepository::find_by_id(pool, id)
            .await?
            .ok_or(AppError::NotFound)?;

        let file_path = icon_path(upload_dir, &icon.filename);
        let file_exists = file_exists(&file_path).await;
        if file_exists && IconRepository::is_referenced(pool, id).await? {
            return Err(AppError::Validation("validation.icon_in_use".to_string()));
        }

        IconRepository::delete(pool, id).await?;
        if file_exists {
            let _ = tokio::fs::remove_file(&file_path).await;
        }
        Ok(icon)
    }
}

/// Where an icon file lives under the upload directory.
pub fn icon_path(upload_dir: &str, filename: &str) -> PathBuf {
    Path::new(upload_dir).join("icons").join(filename)
}

/// Whether a file exists, without blocking the async runtime.
pub async fn file_exists(path: &Path) -> bool {
    tokio::fs::try_exists(path).await.unwrap_or(false)
}

/// Detect MIME type from file magic bytes.
fn detect_mime(data: &[u8]) -> Result<&'static str, AppError> {
    if data.starts_with(&[0x89, 0x50, 0x4E, 0x47]) {
        return Ok("image/png");
    }
    if data.starts_with(&[0xFF, 0xD8, 0xFF]) {
        return Ok("image/jpeg");
    }
    if data.len() >= 12 && data.starts_with(b"RIFF") && &data[8..12] == b"WEBP" {
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
    let trimmed = trimmed.strip_prefix('\u{feff}').unwrap_or(trimmed);
    trimmed.starts_with("<svg") || trimmed.starts_with("<?xml")
}

/// Map MIME type to file extension.
/// Checks an uploaded image and makes it safe to serve: a raster image is
/// scaled to fit `max_dimension`, an SVG is sanitized. Returns its type.
pub async fn prepare_image(
    data: Vec<u8>,
    max_dimension: u32,
) -> Result<(&'static str, Vec<u8>), AppError> {
    let mime = detect_mime(&data)?;
    let processed = process_off_runtime(data, mime, max_dimension).await?;
    Ok((mime, processed))
}

pub fn mime_to_extension(mime: &str) -> &'static str {
    match mime {
        "image/png" => "png",
        "image/jpeg" => "jpg",
        "image/webp" => "webp",
        "image/svg+xml" => "svg",
        _ => "bin",
    }
}

/// Resizing and sanitizing are CPU work: they run on the blocking pool.
async fn process_off_runtime(
    data: Vec<u8>,
    mime: &'static str,
    max_dimension: u32,
) -> Result<Vec<u8>, AppError> {
    let _permit = IMAGE_PERMITS
        .acquire()
        .await
        .map_err(|e| AppError::Internal(anyhow::anyhow!("image queue closed: {e}")))?;
    tokio::task::spawn_blocking(move || process_image(&data, mime, max_dimension))
        .await
        .map_err(|e| AppError::Internal(anyhow::anyhow!("image task failed: {e}")))?
}

/// Process image data: resize raster images, sanitize SVGs.
fn process_image(data: &[u8], mime: &str, max_dimension: u32) -> Result<Vec<u8>, AppError> {
    match mime {
        "image/svg+xml" => sanitize_svg(data),
        "image/jpeg" => resize_raster(data, ImageFormat::Jpeg, max_dimension),
        "image/webp" => resize_raster(data, ImageFormat::WebP, max_dimension),
        _ => resize_raster(data, ImageFormat::Png, max_dimension),
    }
}

/// Keeps a strict SVG subset (see [`SVG_ELEMENTS`]) and returns a document
/// an XML parser accepts: browsers parse a file served as `image/svg+xml`
/// as XML and show nothing when it is not well formed.
fn sanitize_svg(data: &[u8]) -> Result<Vec<u8>, AppError> {
    let invalid = || AppError::Validation("validation.invalid_svg".to_string());
    let text = std::str::from_utf8(data).map_err(|_| invalid())?;
    let cleaned = svg_sanitizer().clean(text).to_string();
    let xml = to_xml_text(cleaned.trim());
    if !xml.starts_with("<svg") || !xml.ends_with("</svg>") {
        return Err(invalid());
    }
    Ok(xml.into_bytes())
}

fn svg_sanitizer() -> ammonia::Builder<'static> {
    let mut builder = ammonia::Builder::empty();
    builder
        .tags(SVG_ELEMENTS.iter().copied().collect())
        .generic_attributes(SVG_ATTRIBUTES.iter().copied().collect())
        .tag_attributes(
            SVG_REFERENCING_ELEMENTS
                .iter()
                .map(|element| (*element, ["href"].into_iter().collect()))
                .chain([("svg", ["xmlns"].into_iter().collect())])
                .collect(),
        )
        .url_schemes(std::collections::HashSet::new())
        .link_rel(None)
        .set_tag_attribute_value("svg", "xmlns", SVG_NAMESPACE)
        .set_tag_attribute_value("svg", "xmlns:xlink", XLINK_NAMESPACE)
        .attribute_filter(keep_svg_attribute);
    builder
}

/// Drops what an XML parser would refuse in an attribute (`<`, control
/// characters), and any `href` that leaves the file.
fn keep_svg_attribute<'a>(_element: &str, attribute: &str, value: &'a str) -> Option<Cow<'a, str>> {
    if value.contains('<') || value.chars().any(is_forbidden_in_xml) {
        return None;
    }
    if attribute == "href" && !value.starts_with('#') {
        return None;
    }
    Some(Cow::Borrowed(value))
}

fn is_forbidden_in_xml(c: char) -> bool {
    c.is_control() && !matches!(c, '\t' | '\n' | '\r')
}

/// HTML serialization differs from XML in two ways that matter here: it
/// writes `&nbsp;`, an entity XML does not define, and it keeps control
/// characters.
fn to_xml_text(html: &str) -> String {
    html.replace("&nbsp;", "&#160;")
        .chars()
        .filter(|c| !is_forbidden_in_xml(*c))
        .collect()
}

/// Resize a raster image to fit within `max_dimension` on each side.
fn resize_raster(
    data: &[u8],
    format: ImageFormat,
    max_dimension: u32,
) -> Result<Vec<u8>, AppError> {
    let mut reader = ImageReader::with_format(Cursor::new(data), format);
    reader.limits(decode_limits());
    let img = reader
        .decode()
        .map_err(|_| AppError::Validation("validation.image_read_error".to_string()))?;

    let img = if img.width() > max_dimension || img.height() > max_dimension {
        img.thumbnail(max_dimension, max_dimension)
    } else {
        img
    };

    let mut buf = Cursor::new(Vec::new());
    img.write_to(&mut buf, format)
        .map_err(|e| AppError::Internal(anyhow::anyhow!("failed to encode image: {e}")))?;
    Ok(buf.into_inner())
}

fn decode_limits() -> Limits {
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_DECODE_DIMENSION);
    limits.max_image_height = Some(MAX_DECODE_DIMENSION);
    limits.max_alloc = Some(MAX_DECODE_ALLOC);
    limits
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Exported by a design tool: declaration, comment, doctype, internal
    /// stylesheet, xlink references, editor namespaces and a no-break space.
    const DESIGN_TOOL_SVG: &str = r##"<?xml version="1.0" encoding="utf-8"?>
<!-- Generator: Adobe Illustrator 27.0.0, SVG Export Plug-In . SVG Version: 6.00 Build 0)  -->
<!DOCTYPE svg PUBLIC "-//W3C//DTD SVG 1.1//EN" "http://www.w3.org/Graphics/SVG/1.1/DTD/svg11.dtd">
<svg version="1.1" id="Layer_1" xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink"
     xmlns:inkscape="http://www.inkscape.org/namespaces/inkscape" x="0px" y="0px"
     viewBox="0 0 64 64" style="enable-background:new 0 0 64 64;" xml:space="preserve" onload="alert(1)">
<style type="text/css">
	.st0{fill:#1C1B18;}
</style>
<sodipodi:namedview id="base" pagecolor="#ffffff"/>
<defs>
	<linearGradient id="SVGID_1_" gradientUnits="userSpaceOnUse" x1="0" y1="0" x2="64" y2="64">
		<stop offset="0" style="stop-color:#8BAE66"/>
		<stop offset="1" style="stop-color:#1C1B18"/>
	</linearGradient>
	<linearGradient id="SVGID_2_" xlink:href="#SVGID_1_"/>
	<rect id="SVGID_3_" width="64" height="64"/>
</defs>
<clipPath id="SVGID_4_">
	<use xlink:href="#SVGID_3_" style="overflow:visible;"/>
</clipPath>
<g inkscape:label="Layer 1" clip-path="url(#SVGID_4_)">
	<path class="st0" fill="url(#SVGID_2_)" d="M32,4C16.5,4,4,16.5,4,32s12.5,28,28,28s28-12.5,28-28S47.5,4,32,4z"/>
	<text x="8" y="40" font-size="12">A&#160;&amp;&#160;B</text>
</g>
<script>alert('xss')</script>
</svg>
"##;

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
        assert!(!text.contains("script"));
        assert!(text.contains("<rect"));
    }

    #[test]
    fn a_design_tool_export_stays_a_well_formed_svg() {
        let result = sanitize_svg(DESIGN_TOOL_SVG.as_bytes()).unwrap();
        let text = std::str::from_utf8(&result).unwrap();

        if let Err(problem) = xml_check::well_formed(text) {
            panic!("not well formed XML ({problem}):\n{text}");
        }
        assert!(text.starts_with("<svg"));
        assert!(text.contains(r#"xmlns="http://www.w3.org/2000/svg""#));
        assert!(text.contains(r#"xmlns:xlink="http://www.w3.org/1999/xlink""#));
        assert!(text.contains(r##"<use xlink:href="#SVGID_3_""##));
        assert!(text.contains(r##"xlink:href="#SVGID_1_""##));
        assert!(text.contains(r#"viewBox="0 0 64 64""#));
        assert!(text.contains("&#160;"));
        for gone in [
            "<style",
            "script",
            "onload",
            "inkscape",
            "sodipodi",
            "class=",
            "xml:space",
        ] {
            assert!(!text.contains(gone), "{gone} should be removed:\n{text}");
        }
    }

    #[test]
    fn references_that_leave_the_file_are_removed() {
        let input = br##"<svg xmlns="http://www.w3.org/2000/svg">
            <use href="https://evil.example/x.svg#a"/>
            <use href="data:image/svg+xml;base64,PHN2Zz4="/>
            <use href="other.svg#a"/>
            <use href=" javascript:alert(1)"/>
            <use href="#local"/>
            <a href="https://evil.example"><rect/></a>
            <image href="https://evil.example/p.png"/>
            <foreignObject><div onclick="x()">hi</div></foreignObject>
            <path d="M0 0" id="a&lt;b"/>
        </svg>"##;
        let result = sanitize_svg(input).unwrap();
        let text = std::str::from_utf8(&result).unwrap();

        assert!(xml_check::well_formed(text).is_ok(), "{text}");
        assert_eq!(text.matches("href=").count(), 1, "{text}");
        assert!(text.contains(r##"href="#local""##));
        for gone in [
            "evil",
            "data:",
            "other.svg",
            "javascript",
            "<image",
            "<a",
            "<div",
            "hi<",
            "a<b",
        ] {
            assert!(!text.contains(gone), "{gone} should be removed:\n{text}");
        }
        assert!(
            text.contains("<rect"),
            "the content of a removed link is kept"
        );
    }

    #[test]
    fn a_file_without_an_svg_root_is_refused() {
        let input = b"<?xml version=\"1.0\"?><html><body><p>Hello</p></body></html>";
        assert!(matches!(
            sanitize_svg(input),
            Err(AppError::Validation(key)) if key == "validation.invalid_svg"
        ));
    }

    #[test]
    fn a_canvas_past_the_decode_limit_is_refused() {
        let mut png = Vec::new();
        image::RgbaImage::new(MAX_DECODE_DIMENSION + 1, 1)
            .write_to(&mut Cursor::new(&mut png), ImageFormat::Png)
            .unwrap();
        assert!(
            png.len() < MAX_ICON_SIZE,
            "a small file can declare a wide canvas"
        );

        assert!(matches!(
            resize_raster(&png, ImageFormat::Png, MAX_ICON_DIMENSION),
            Err(AppError::Validation(key)) if key == "validation.image_read_error"
        ));
    }

    #[test]
    fn a_large_raster_is_resized() {
        let mut png = Vec::new();
        image::RgbaImage::new(512, 256)
            .write_to(&mut Cursor::new(&mut png), ImageFormat::Png)
            .unwrap();

        let resized = resize_raster(&png, ImageFormat::Png, MAX_ICON_DIMENSION).unwrap();
        let img = image::load_from_memory(&resized).unwrap();
        assert_eq!(
            (img.width(), img.height()),
            (MAX_ICON_DIMENSION, MAX_ICON_DIMENSION / 2)
        );
    }

    #[test]
    fn mime_extensions() {
        assert_eq!(mime_to_extension("image/png"), "png");
        assert_eq!(mime_to_extension("image/jpeg"), "jpg");
        assert_eq!(mime_to_extension("image/webp"), "webp");
        assert_eq!(mime_to_extension("image/svg+xml"), "svg");
    }

    /// A small XML well-formedness check, enough for the sanitizer output:
    /// one root, balanced elements, quoted and unique attributes without
    /// `<`, known entities, and declared namespace prefixes. The crate has
    /// no XML parser dependency.
    mod xml_check {
        use super::super::is_forbidden_in_xml;

        /// An open element and the prefixes it declares.
        type Open = (String, Vec<String>);

        pub fn well_formed(text: &str) -> Result<(), String> {
            let mut stack: Vec<Open> = Vec::new();
            let mut roots = 0;
            let mut rest = text;
            while let Some(start) = rest.find('<') {
                check_text(&rest[..start])?;
                if stack.is_empty() && !rest[..start].trim().is_empty() {
                    return Err("text outside the root".into());
                }
                let tag_len = tag_length(&rest[start + 1..]).ok_or("unclosed tag")?;
                let tag = &rest[start + 1..start + 1 + tag_len];
                rest = &rest[start + 2 + tag_len..];
                if let Some(name) = tag.strip_prefix('/') {
                    let (open, _) = stack.pop().ok_or("closing tag without opening")?;
                    if open != name.trim() {
                        return Err(format!("</{name}> closes <{open}>"));
                    }
                    continue;
                }
                if stack.is_empty() {
                    roots += 1;
                }
                let element = parse_tag(tag.trim_end_matches('/'), &stack)?;
                if !tag.ends_with('/') {
                    stack.push(element);
                }
            }
            check_text(rest)?;
            if !stack.is_empty() || roots != 1 || !rest.trim().is_empty() {
                return Err("not exactly one closed root element".into());
            }
            Ok(())
        }

        /// Length of a tag up to its `>`, skipping quoted attribute values.
        fn tag_length(text: &str) -> Option<usize> {
            let mut quoted = false;
            text.char_indices().find_map(|(i, c)| {
                if c == '"' {
                    quoted = !quoted;
                }
                (c == '>' && !quoted).then_some(i)
            })
        }

        fn parse_tag(tag: &str, stack: &[Open]) -> Result<Open, String> {
            let (name, mut rest) = tag.split_once(char::is_whitespace).unwrap_or((tag, ""));
            if name.is_empty() || name.starts_with(['!', '?']) {
                return Err(format!("unexpected markup <{tag}>"));
            }
            let mut attributes = Vec::new();
            while !rest.trim_start().is_empty() {
                let (attribute, value, after) = split_attribute(rest.trim_start())?;
                check_text(value)?;
                if attributes.contains(&attribute) {
                    return Err(format!("duplicate attribute {attribute}"));
                }
                attributes.push(attribute);
                rest = after;
            }
            let declared: Vec<String> = attributes
                .iter()
                .filter_map(|a| a.strip_prefix("xmlns:"))
                .map(String::from)
                .collect();
            let in_scope: Vec<&str> = stack
                .iter()
                .flat_map(|(_, prefixes)| prefixes.iter().map(String::as_str))
                .chain(declared.iter().map(String::as_str))
                .chain(["xml"])
                .collect();
            let mut qualified: Vec<&str> = attributes
                .iter()
                .map(String::as_str)
                .filter(|a| !a.starts_with("xmlns"))
                .collect();
            qualified.push(name);
            for used in qualified {
                if let Some((prefix, _)) = used.split_once(':')
                    && !in_scope.contains(&prefix)
                {
                    return Err(format!("undeclared prefix in {used}"));
                }
            }
            Ok((name.to_string(), declared))
        }

        fn split_attribute(text: &str) -> Result<(String, &str, &str), String> {
            let (name, after) = text.split_once('=').ok_or("attribute without value")?;
            let after = after.strip_prefix('"').ok_or("unquoted attribute")?;
            let close = after.find('"').ok_or("unterminated attribute")?;
            let value = &after[..close];
            if value.contains('<') {
                return Err("< in an attribute value".into());
            }
            Ok((name.trim().to_string(), value, &after[close + 1..]))
        }

        fn check_text(text: &str) -> Result<(), String> {
            if text.chars().any(is_forbidden_in_xml) {
                return Err("control character".into());
            }
            let mut rest = text;
            while let Some(amp) = rest.find('&') {
                let end = rest[amp..].find(';').ok_or("unterminated entity")? + amp;
                let entity = &rest[amp + 1..end];
                if !is_xml_entity(entity) {
                    return Err(format!("entity &{entity}; is not defined in XML"));
                }
                rest = &rest[end + 1..];
            }
            Ok(())
        }

        fn is_xml_entity(entity: &str) -> bool {
            matches!(entity, "amp" | "lt" | "gt" | "quot" | "apos")
                || entity
                    .strip_prefix("#x")
                    .is_some_and(|hex| u32::from_str_radix(hex, 16).is_ok())
                || entity
                    .strip_prefix('#')
                    .is_some_and(|dec| dec.parse::<u32>().is_ok())
        }

        #[test]
        fn the_checker_itself_catches_what_browsers_refuse() {
            assert!(well_formed(r#"<svg a="1"><g/></svg>"#).is_ok());
            assert!(well_formed("<svg><g></svg>").is_err());
            assert!(well_formed(r#"<svg a="1" a="2"></svg>"#).is_err());
            assert!(well_formed(r##"<svg><use xlink:href="#a"/></svg>"##).is_err());
            assert!(well_formed("<svg>&nbsp;</svg>").is_err());
            assert!(well_formed(r#"<svg id="a<b"></svg>"#).is_err());
            assert!(well_formed("<svg></svg><svg></svg>").is_err());
        }
    }
}
