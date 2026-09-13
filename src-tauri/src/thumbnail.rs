use std::fs;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use tauri::{AppHandle, Manager};

use crate::safety;

/// 缩略图最长边。
const THUMB_SIZE: u32 = 512;

/// 解码一张 8192² PNG 的瞬时峰值可达数百 MB；全局串行化保证同一时刻
/// 只有一张全图在解码内存里，排队请求由 spawn_blocking 线程池承载。
static DECODE_LOCK: Mutex<()> = Mutex::new(());

fn to_string_error(error: impl std::fmt::Display) -> String {
    error.to_string()
}

/// 缓存键 = canonicalize 后的源路径 + mtime + 文件长度。
/// 源文件重写（超分重跑覆盖同名输出）后 mtime/长度变化，键随之失效。
fn cache_key(source: &Path) -> Result<u64, String> {
    let canonical = fs::canonicalize(source).map_err(to_string_error)?;
    let metadata = fs::metadata(&canonical).map_err(to_string_error)?;
    let modified = metadata.modified().map_err(to_string_error)?;
    let mut hasher = std::hash::DefaultHasher::new();
    canonical.hash(&mut hasher);
    modified.hash(&mut hasher);
    metadata.len().hash(&mut hasher);
    Ok(hasher.finish())
}

/// 生成（或命中）512 内缩略图，返回缩略图磁盘绝对路径。
/// 纯函数：不依赖 AppHandle，命令层负责目录解析与 spawn_blocking。
pub(crate) fn ensure_thumbnail(cache_dir: &Path, source: &Path) -> Result<PathBuf, String> {
    if !source.is_file() {
        return Err(format!("文件不存在：{}", source.display()));
    }
    let key = cache_key(source)?;
    fs::create_dir_all(cache_dir).map_err(to_string_error)?;
    let target = cache_dir.join(format!("{key:x}.png"));
    if target.is_file() {
        return Ok(target);
    }

    let _guard = DECODE_LOCK.lock().map_err(to_string_error)?;
    // 拿到锁后复查：排队期间前一个请求可能已生成同一张缩略图。
    if target.is_file() {
        return Ok(target);
    }

    let (width, height) =
        image::image_dimensions(source).map_err(|error| format!("无法读取图片尺寸：{error}"))?;
    safety::memory_budget(width, height, 32)?;
    let image = image::open(source).map_err(|error| format!("无法解码图片：{error}"))?;
    let thumbnail = image.thumbnail(THUMB_SIZE, THUMB_SIZE);
    safety::atomic_write(&target, |writer| {
        thumbnail
            .write_to(writer, image::ImageFormat::Png)
            .map_err(to_string_error)
    })?;
    Ok(target)
}

#[tauri::command]
pub(crate) async fn gallery_thumbnail(app: AppHandle, path: String) -> Result<String, String> {
    let cache_dir = app
        .path()
        .app_data_dir()
        .map_err(|error| format!("无法定位应用数据目录：{error}"))?
        .join("thumbs");
    tauri::async_runtime::spawn_blocking(move || {
        ensure_thumbnail(&cache_dir, Path::new(&path)).map(|value| value.display().to_string())
    })
    .await
    .map_err(to_string_error)?
}

#[tauri::command]
pub(crate) async fn files_exist(paths: Vec<String>) -> Vec<bool> {
    let count = paths.len();
    tauri::async_runtime::spawn_blocking(move || {
        paths
            .iter()
            .map(|value| Path::new(value).is_file())
            .collect::<Vec<_>>()
    })
    .await
    .unwrap_or_else(|_| vec![false; count])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_png(path: &Path, width: u32, height: u32, seed: u8) {
        let image = image::DynamicImage::ImageRgba8(image::RgbaImage::from_fn(width, height, |x, y| {
            image::Rgba([x as u8 ^ seed, y as u8 ^ seed, seed, 255])
        }));
        image.save(path).unwrap();
    }

    #[test]
    fn thumbnail_fits_512_and_reuses_cache_entry() {
        let source_dir = tempfile::tempdir().unwrap();
        let cache = tempfile::tempdir().unwrap();
        let source = source_dir.path().join("hero.png");
        write_png(&source, 2048, 1024, 7);

        let first = ensure_thumbnail(cache.path(), &source).unwrap();
        let (width, height) = image::image_dimensions(&first).unwrap();
        assert_eq!((width, height), (512, 256));
        let cached = fs::metadata(&first).unwrap();
        assert_eq!(
            ensure_thumbnail(cache.path(), &source).unwrap(),
            first,
            "second call must hit the cache"
        );
        let again = fs::metadata(&first).unwrap();
        assert_eq!(cached.modified().unwrap(), again.modified().unwrap());
        assert_eq!(cached.len(), again.len());
        assert_eq!(fs::read_dir(cache.path()).unwrap().count(), 1);
    }

    #[test]
    fn rewritten_source_gets_a_new_cache_entry() {
        let source_dir = tempfile::tempdir().unwrap();
        let cache = tempfile::tempdir().unwrap();
        let source = source_dir.path().join("hero.png");
        write_png(&source, 2048, 1024, 7);
        let first = ensure_thumbnail(cache.path(), &source).unwrap();
        // 尺寸不同保证长度与 mtime 都变化，键必然失效。
        write_png(&source, 2048, 1023, 9);
        std::thread::sleep(std::time::Duration::from_millis(50));
        let second = ensure_thumbnail(cache.path(), &source).unwrap();
        assert_ne!(first, second);
        assert_eq!(fs::read_dir(cache.path()).unwrap().count(), 2);
    }

    #[test]
    fn missing_and_undecodable_sources_are_rejected() {
        let source_dir = tempfile::tempdir().unwrap();
        let cache = tempfile::tempdir().unwrap();
        let missing = source_dir.path().join("missing.png");
        let error = ensure_thumbnail(cache.path(), &missing).unwrap_err();
        assert!(error.contains("文件不存在"), "unexpected message: {error}");
        let garbage = source_dir.path().join("garbage.png");
        fs::write(&garbage, b"not a png").unwrap();
        assert!(ensure_thumbnail(cache.path(), &garbage).is_err());
    }
}
