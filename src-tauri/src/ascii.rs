use base64::Engine;
use std::{fs, path::Path};

fn export_bytes(format: &str, content: &str) -> Result<Vec<u8>, String> {
    if content.len() > 32 * 1024 * 1024 {
        return Err("导出内容过大。".into());
    }
    match format {
        "txt" => {
            if content.len() > 100_000
                || !content
                    .bytes()
                    .all(|b| b == b'\n' || (32..=126).contains(&b))
            {
                return Err("ASCII 文本内容无效。".into());
            }
            Ok(content.as_bytes().to_vec())
        }
        "png" => {
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(content)
                .map_err(|_| "PNG 编码无效。")?;
            let decoder = image::codecs::png::PngDecoder::new(std::io::Cursor::new(&bytes))
                .map_err(|_| "PNG 内容无效。")?;
            use image::ImageDecoder;
            let (width, height) = decoder.dimensions();
            if width == 0
                || height == 0
                || width > 4096
                || height > 8192
                || u64::from(width) * u64::from(height) > 20_000_000
            {
                return Err("PNG 尺寸超出限制。".into());
            }
            image::DynamicImage::from_decoder(decoder).map_err(|_| "PNG 内容损坏。")?;
            Ok(bytes)
        }
        _ => Err("仅支持 TXT 和 PNG。".into()),
    }
}

#[tauri::command]
pub async fn ascii_export(path: String, format: String, content: String) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || write_export(path, format, content))
        .await
        .map_err(|e| e.to_string())?
}

fn write_export(path: String, format: String, content: String) -> Result<String, String> {
    let target = Path::new(&path);
    if target
        .extension()
        .and_then(|v| v.to_str())
        .map(|v| v.to_ascii_lowercase())
        != Some(format.clone())
    {
        return Err("文件扩展名与导出格式不一致。".into());
    }
    let bytes = export_bytes(&format, &content)?;
    fs::write(target, bytes).map_err(|e| format!("保存失败：{e}"))?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn text_preserves_spaces_and_lines() {
        assert_eq!(export_bytes("txt", "  @ \n .  ").unwrap(), b"  @ \n .  ");
        assert!(export_bytes("txt", "中文").is_err());
        assert!(export_bytes("txt", "\x1b").is_err());
        assert!(export_bytes("txt", &"a".repeat(100_001)).is_err());
    }
    #[test]
    fn rejects_invalid_format_and_png() {
        assert!(export_bytes("html", "").is_err());
        assert!(export_bytes("png", "invalid").is_err());
        assert!(export_bytes("png", "aGVsbG8=").is_err());
    }
    #[test]
    fn exports_real_files_and_rejects_invalid_replacements() {
        let dir = tempfile::tempdir().unwrap();
        let txt = dir
            .path()
            .join("image_ascii.txt")
            .to_string_lossy()
            .to_string();
        write_export(txt.clone(), "txt".into(), " @\n. ".into()).unwrap();
        assert_eq!(fs::read_to_string(&txt).unwrap(), " @\n. ");
        assert!(write_export(txt.clone(), "png".into(), "invalid".into()).is_err());
        assert_eq!(fs::read_to_string(&txt).unwrap(), " @\n. ");
        let mut png = std::io::Cursor::new(Vec::new());
        image::DynamicImage::new_rgb8(12, 18)
            .write_to(&mut png, image::ImageFormat::Png)
            .unwrap();
        let path = dir
            .path()
            .join("image_ascii.png")
            .to_string_lossy()
            .to_string();
        let content = base64::engine::general_purpose::STANDARD.encode(png.get_ref());
        write_export(path.clone(), "png".into(), content).unwrap();
        assert_eq!(image::image_dimensions(&path).unwrap(), (12, 18));
        assert!(write_export(
            dir.path()
                .join("missing/image.txt")
                .to_string_lossy()
                .to_string(),
            "txt".into(),
            "x".into()
        )
        .is_err());
    }
}
