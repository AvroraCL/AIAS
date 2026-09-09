use std::{collections::HashSet, io::{BufWriter, Write}, path::Path, sync::{Mutex, MutexGuard}};

static TASK: Mutex<()> = Mutex::new(());
pub(crate) fn task_guard() -> Result<MutexGuard<'static, ()>, String> {
    match TASK.try_lock() {
        Ok(guard) => Ok(guard),
        Err(std::sync::TryLockError::Poisoned(error)) => Ok(error.into_inner()),
        Err(std::sync::TryLockError::WouldBlock) => Err("另一个图片任务正在运行，请等待完成后重试。".into()),
    }
}

pub(crate) fn unique_stems(files: &[String]) -> Result<(), String> {
    let mut seen = HashSet::new();
    for file in files {
        let stem = Path::new(file).file_stem().and_then(|s| s.to_str()).ok_or("图片文件名无效")?;
        if !seen.insert(stem.to_lowercase()) {
            return Err(format!("输入存在重名图片「{stem}」，会覆盖同一个输出文件。请先重命名或分批导出。"));
        }
    }
    Ok(())
}

// Same-directory staging keeps a failed encode/write from truncating an old result.
pub(crate) fn atomic_write(path: &Path, write: impl FnOnce(&mut BufWriter<&mut std::fs::File>) -> Result<(), String>) -> Result<(), String> {
    let parent = path.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new("."));
    let mut temp = tempfile::NamedTempFile::new_in(parent).map_err(|e| e.to_string())?;
    {
        let mut writer = BufWriter::new(temp.as_file_mut());
        write(&mut writer)?;
        writer.flush().map_err(|e| e.to_string())?;
    }
    temp.as_file().sync_all().map_err(|e| e.to_string())?;
    temp.persist(path).map_err(|e| e.error.to_string())?;
    Ok(())
}

pub(crate) fn memory_budget(w: u32, h: u32, bytes_per_pixel: u64) -> Result<(), String> {
    let system = sysinfo::System::new_with_specifics(sysinfo::RefreshKind::nothing().with_memory(sysinfo::MemoryRefreshKind::everything()));
    check_memory_budget(w, h, bytes_per_pixel, system.available_memory())
}

fn check_memory_budget(w: u32, h: u32, bytes_per_pixel: u64, available: u64) -> Result<(), String> {
    let estimate = u64::from(w).checked_mul(u64::from(h)).and_then(|n| n.checked_mul(bytes_per_pixel)).ok_or("图片尺寸溢出")?;
    if w == 0 || h == 0 { return Err("图片尺寸不能为零。".into()); }
    // This is an estimate, not a process allocation limit. Reserve half of the
    // currently available RAM for model/runtime allocations and other apps.
    // A fixed 2 GiB cap incorrectly rejected ordinary large cutout sources.
    let budget = available / 2;
    if estimate > budget {
        return Err(format!("图片处理预计需要 {} MiB 临时内存；当前系统可用 {} MiB，安全预算 {} MiB（可用内存的 50%）。请关闭其他高内存任务，或缩小图片／降低倍率后重试。", estimate / 1024 / 1024, available / 1024 / 1024, budget / 1024 / 1024));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_cross_directory_and_extension_collisions() {
        assert!(unique_stems(&["a/Hero.png".into(), "b/hero.jpg".into()]).is_err());
        assert!(unique_stems(&["a/hero.png".into(), "b/hero_pose.jpg".into()]).is_ok());
    }
    #[test]
    fn exclusive_task_guard_recovers_after_release() {
        let guard = task_guard().unwrap(); assert!(task_guard().is_err()); drop(guard);
        assert!(task_guard().is_ok());
    }
    #[test]
    fn failed_write_preserves_old_file_and_cleans_staging() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().join("测试区/临时输出/robustness");
        std::fs::create_dir_all(&root).unwrap();
        let dir = tempfile::tempdir_in(root).unwrap();
        let output = dir.path().join("result.bin");
        std::fs::write(&output, b"old").unwrap();
        assert!(atomic_write(&output, |writer| {
            writer.write_all(b"partial").unwrap(); Err("simulated encoder failure".into())
        }).is_err());
        assert_eq!(std::fs::read(&output).unwrap(), b"old");
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
        atomic_write(&output, |writer| writer.write_all(b"new").map_err(|e| e.to_string())).unwrap();
        assert_eq!(std::fs::read(output).unwrap(), b"new");
    }

    #[test]
    fn impossible_memory_requests_are_rejected() {
        assert!(memory_budget(u32::MAX, u32::MAX, u64::MAX).is_err());
        assert!(memory_budget(0, 8, 4).is_err());
    }

    #[test]
    fn cutout_over_two_gib_is_allowed_when_ram_is_available() {
        // Same 128 bytes/pixel estimate used by process_one before decoding.
        // A 4320-square source needs 2278 MiB; 16 GiB free must admit it.
        check_memory_budget(4320, 4320, 128, 16 * 1024 * 1024 * 1024).unwrap();
    }

    #[test]
    fn memory_budget_reserves_half_of_available_ram() {
        let available = 4 * 1024 * 1024 * 1024;
        assert!(check_memory_budget(4320, 4320, 128, available).is_err());
        assert!(check_memory_budget(4096, 4096, 128, available).is_ok());
    }

    #[test]
    fn rejection_explains_available_memory_and_budget() {
        let error = check_memory_budget(4320, 4320, 128, 4 * 1024 * 1024 * 1024).unwrap_err();
        assert!(error.contains("2278 MiB"));
        assert!(error.contains("可用 4096 MiB"));
        assert!(error.contains("预算 2048 MiB"));
    }

    #[test]
    #[ignore = "manual check against local cutout inputs and current system memory"]
    fn reported_cutout_sources_pass_live_memory_preflight() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().join("测试区/测试用图片");
        for name in ["130169544_p0.png", "136565655_p0.jpg"] {
            let (w, h) = image::image_dimensions(root.join(name)).unwrap();
            println!("{name}: {w}x{h}, estimate={} MiB", u64::from(w) * u64::from(h) * 128 / 1024 / 1024);
            memory_budget(w, h, 128).unwrap();
        }
    }
}
