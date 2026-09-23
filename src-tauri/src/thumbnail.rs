use std::fs;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use image_dds::{ddsfile::Dds, image_from_dds};
use tauri::{AppHandle, Manager};

use crate::safety;

/// 缩略图最长边。
const THUMB_SIZE: u32 = 512;

/// 缓存目录容量上限：超出后按 mtime 从旧到新删到 90%。
const CACHE_MAX_BYTES: u64 = 512 * 1024 * 1024;

/// 每进程只裁剪一次：缓存增长以天计，启动后首次渲染时做一遍足够。
static PRUNED: AtomicBool = AtomicBool::new(false);

/// 解码一张 8192² PNG 的瞬时峰值可达数百 MB；全局串行化保证同一时刻
/// 只有一张全图在解码内存里，排队请求由 spawn_blocking 线程池承载。
static DECODE_LOCK: Mutex<()> = Mutex::new(());

use crate::to_string_error;

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

/// 缓存容量裁剪：按 mtime 从旧到新删除，直到总量 ≤ max_bytes 的 90%。
/// 纯函数便于测试；目录不可读时静默返回（缓存属可重建数据）。
pub(crate) fn prune_cache(cache_dir: &Path, max_bytes: u64) {
    let Ok(entries) = fs::read_dir(cache_dir) else {
        return;
    };
    let mut files: Vec<(std::time::SystemTime, u64, PathBuf)> = Vec::new();
    let mut total: u64 = 0;
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(metadata) = entry.metadata() else {
            continue;
        };
        if !metadata.is_file() {
            continue;
        }
        let modified = metadata
            .modified()
            .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
        total += metadata.len();
        files.push((modified, metadata.len(), path));
    }
    if total <= max_bytes {
        return;
    }
    files.sort_by_key(|(modified, _, _)| *modified);
    let target = max_bytes * 9 / 10;
    for (_, size, path) in files {
        if total <= target {
            break;
        }
        if fs::remove_file(&path).is_ok() {
            total = total.saturating_sub(size);
        }
    }
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

    let image = if source
        .extension()
        .is_some_and(|ext| ext.to_string_lossy().eq_ignore_ascii_case("dds"))
    {
        let file = fs::File::open(source).map_err(to_string_error)?;
        let dds = Dds::read(&mut std::io::BufReader::new(file))
            .map_err(|error| format!("无法读取 DDS：{error}"))?;
        let mut level = 0;
        while level < 31
            && level + 1 < dds.get_num_mipmap_levels()
            && dds.header.width.max(dds.header.height) >> level > THUMB_SIZE
        {
            level += 1;
        }
        let width = (dds.header.width >> level).max(1);
        let height = (dds.header.height >> level).max(1);
        safety::memory_budget(width, height, 32)?;
        image::DynamicImage::ImageRgba8(
            image_from_dds(&dds, level).map_err(|error| format!("无法解码 DDS：{error}"))?,
        )
    } else {
        let (width, height) = image::image_dimensions(source)
            .map_err(|error| format!("无法读取图片尺寸：{error}"))?;
        safety::memory_budget(width, height, 32)?;
        image::open(source).map_err(|error| format!("无法解码图片：{error}"))?
    };
    let thumbnail = if image.width().max(image.height()) > THUMB_SIZE {
        image.thumbnail(THUMB_SIZE, THUMB_SIZE)
    } else {
        image
    };
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
        if !PRUNED.swap(true, Ordering::SeqCst) {
            prune_cache(&cache_dir, CACHE_MAX_BYTES);
        }
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

    fn filetime_set(path: &Path, time: std::time::SystemTime) {
        // Rust 1.75+ 提供 File::set_modified；失败时忽略（部分文件系统不支持），
        // 测试主要断言裁剪行为本身。
        if let Ok(file) = fs::OpenOptions::new().write(true).open(path) {
            let _ = file.set_modified(time);
        }
    }
    fn write_png(path: &Path, width: u32, height: u32, seed: u8) {
        let image =
            image::DynamicImage::ImageRgba8(image::RgbaImage::from_fn(width, height, |x, y| {
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
    fn prune_removes_oldest_entries_down_to_target() {
        let cache = tempfile::tempdir().unwrap();
        for index in 0..10u64 {
            let path = cache.path().join(format!("{index}.png"));
            fs::write(&path, vec![0u8; 1024]).unwrap();
            // 用文件时间戳拉开 mtime：编号越大越新。
            let time = std::time::SystemTime::UNIX_EPOCH
                + std::time::Duration::from_secs(1_700_000_000 + index);
            filetime_set(&path, time);
        }
        // 总量 10240，上限 4096 → 裁剪到 90%（3686）以下。
        prune_cache(cache.path(), 4096);
        let remaining: u64 = fs::read_dir(cache.path())
            .unwrap()
            .flatten()
            .map(|entry| entry.metadata().unwrap().len())
            .sum();
        assert!(remaining <= 4096, "总量应裁剪到上限内，实际 {remaining}");
        assert!(!cache.path().join("0.png").exists(), "最旧的条目应被删除");
        assert!(cache.path().join("9.png").exists(), "最新的条目应保留");

        // 未超限时不动任何文件。
        let small = tempfile::tempdir().unwrap();
        fs::write(small.path().join("only.png"), vec![0u8; 100]).unwrap();
        prune_cache(small.path(), 4096);
        assert!(small.path().join("only.png").exists());
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
        let broken_dds = source_dir.path().join("broken.dds");
        fs::write(&broken_dds, b"not a DDS").unwrap();
        assert!(ensure_thumbnail(cache.path(), &broken_dds).is_err());
    }

    #[test]
    fn dds_thumbnail_decodes_bc3_and_rgba8() {
        let source_dir = tempfile::tempdir().unwrap();
        let cache = tempfile::tempdir().unwrap();
        let image = image::RgbaImage::from_pixel(8, 8, image::Rgba([210, 75, 32, 128]));
        for (format, name) in [("DXT5", "color_c.dds"), ("8.8.8.8", "normal_n.dds")] {
            let source = source_dir.path().join(name);
            fs::write(&source, crate::encode_dds(&image, format).unwrap()).unwrap();
            let output = ensure_thumbnail(cache.path(), &source).unwrap();
            let decoded = image::open(output).unwrap().to_rgba8();
            assert_eq!(decoded.dimensions(), (8, 8));
            assert!(decoded.get_pixel(0, 0)[0] > 150);
            assert!(decoded.get_pixel(0, 0)[3] < 255);
        }
    }

    #[test]
    fn dds_thumbnail_uses_a_small_mip() {
        let source_dir = tempfile::tempdir().unwrap();
        let cache = tempfile::tempdir().unwrap();
        let source = source_dir.path().join("mipped_c.dds");
        let first = image::RgbaImage::from_pixel(1024, 1024, image::Rgba([255, 0, 0, 255]));
        let second = image::RgbaImage::from_pixel(512, 512, image::Rgba([0, 255, 0, 255]));
        let levels = [
            crate::MipmapLevel {
                width: 1024,
                height: 1024,
                payload: first.into_raw(),
            },
            crate::MipmapLevel {
                width: 512,
                height: 512,
                payload: second.into_raw(),
            },
        ];
        fs::write(&source, crate::build_dds(&levels, "8.8.8.8").unwrap()).unwrap();
        let output = ensure_thumbnail(cache.path(), &source).unwrap();
        let decoded = image::open(output).unwrap().to_rgba8();
        assert_eq!(decoded.dimensions(), (512, 512));
        assert_eq!(*decoded.get_pixel(0, 0), image::Rgba([0, 255, 0, 255]));
    }
}
