//! `anime::runtime` — 拆分自 anime.rs，职责见模块内条目注释。

use super::*;

use ort::session::{builder::GraphOptimizationLevel, Session};
use serde::Serialize;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;
use tauri::{AppHandle, Emitter};

// ONNX Runtime acquisition (onnxruntime.dll next to the app data)
pub(crate) fn ort_dll_path(base: &Path) -> PathBuf {
    base.join("onnxruntime.dll")
}

pub(crate) static ORT_READY: OnceLock<()> = OnceLock::new();

pub fn ensure_ort_runtime(base: &Path) -> Result<(), String> {
    ensure_ort_runtime_with(base, &|_, _| {})
}

/// 同 `ensure_ort_runtime`，但把运行库下载的字节进度透出给调用方
/// （首次使用且本机无种子时，CPU 运行库有几十 MB，静默下载看起来像卡死）。
pub fn ensure_ort_runtime_with(base: &Path, on_progress: &dyn Fn(u64, u64)) -> Result<(), String> {
    fs::create_dir_all(base).map_err(to_string_error)?;
    // 进程内 ORT 只会加载一次 dll：优先使用完整 GPU 版运行库，其次才是种子/下载的 CPU 版。
    let dll = if gpu_ort_ready(base) {
        gpu_ort_capi_dir(base).join("onnxruntime.dll")
    } else {
        ort_dll_path(base)
    };
    if !dll.exists() {
        acquire_ort_dll(base, on_progress)?;
    }
    if ORT_READY.get().is_some() {
        return Ok(());
    }
    std::env::set_var("ORT_DYLIB_PATH", &dll);
    // set 失败说明另一个线程刚刚完成了同样的初始化（相同 dll 路径），视为成功。
    let _ = ORT_READY.set(());
    Ok(())
}

pub fn ort_initialized() -> bool {
    ORT_READY.get().is_some()
}

/// Local seeds to try before downloading: an existing ComfyUI install ships
/// the very same onnxruntime.dll in its venv.
pub(crate) fn ort_seed_candidates() -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    for root in [
        "F:\\WebUI\\ComfyUI",
        "C:\\WebUI\\ComfyUI",
        "D:\\WebUI\\ComfyUI",
        "C:\\ComfyUI",
        "D:\\ComfyUI",
    ] {
        candidates.push(
            PathBuf::from(root)
                .join("venv\\Lib\\site-packages\\onnxruntime\\capi\\onnxruntime.dll"),
        );
        candidates.push(
            PathBuf::from(root)
                .join("python_embeded\\Lib\\site-packages\\onnxruntime\\capi\\onnxruntime.dll"),
        );
    }
    candidates
}

pub(crate) fn acquire_ort_dll(base: &Path, on_progress: &dyn Fn(u64, u64)) -> Result<(), String> {
    for candidate in ort_seed_candidates() {
        if candidate.exists() {
            // CPU 版核心 dll 就够运行了；provider DLL 由 GPU 运行库自带，不复制
            // （ComfyUI 种子里的 providers_cuda.dll 有 350MB，且 CPU 版也用不了）。
            fs::copy(&candidate, ort_dll_path(base)).map_err(to_string_error)?;
            return Ok(());
        }
    }

    // Download a pinned release and extract with the system tar (zip-capable).
    const VERSION: &str = "1.22.0";
    let archive_urls = [
        format!("https://ghfast.top/https://github.com/microsoft/onnxruntime/releases/download/v{VERSION}/onnxruntime-win-x64-{VERSION}.zip"),
        format!("https://github.com/microsoft/onnxruntime/releases/download/v{VERSION}/onnxruntime-win-x64-{VERSION}.zip"),
    ];
    let tmp_dir = base.join("ort_tmp");
    fs::create_dir_all(&tmp_dir).map_err(to_string_error)?;
    let archive = tmp_dir.join(format!("ort-{VERSION}.zip"));
    // 解压出的 onnxruntime.dll 会被 LoadLibrary 进本进程：这是全项目唯一
    // 曾无任何校验的下载（ghfast 代理居首）。固化官方 zip 的尺寸与 SHA256
    // （与 GPU wheel 同款），不匹配即删档换下一镜像。
    const ORT_ZIP_SIZE: u64 = 72_368_545;
    const ORT_ZIP_SHA256: &str =
        "174c616efc0271194488642a72f1a514e01487da4dfe84c49296d66e40ebe0da";
    let mut last_error = String::from("无可用下载源");
    for url in archive_urls {
        match curl_download(&url, &archive, Some(ORT_ZIP_SIZE), on_progress) {
            Ok(()) => match crate::model_bake::sha256_of_file(&archive) {
                Ok(actual) if actual == ORT_ZIP_SHA256 => break,
                Ok(actual) => {
                    let _ = fs::remove_file(&archive);
                    last_error =
                        format!("SHA256 不匹配（期望 {ORT_ZIP_SHA256}，实际 {actual}）");
                }
                Err(error) => {
                    let _ = fs::remove_file(&archive);
                    last_error = format!("校验读取失败：{error}");
                }
            },
            Err(error) => last_error = error,
        }
    }
    if !archive.exists()
        || crate::model_bake::sha256_of_file(&archive)
            .map(|actual| actual != ORT_ZIP_SHA256)
            .unwrap_or(true)
    {
        // 失败路径同样要清 tmp_dir：残留的部分 zip 最大 72MB
        let _ = fs::remove_dir_all(&tmp_dir);
        return Err(format!("获取 onnxruntime 运行库失败：{last_error}"));
    }
    let extract_status = crate::safety::quiet_command("tar")
        .args(["-xf"])
        .arg(&archive)
        .arg("-C")
        .arg(&tmp_dir)
        .status();
    let Some(dll) = find_file(&tmp_dir, "onnxruntime.dll") else {
        let _ = fs::remove_dir_all(&tmp_dir);
        return Err("压缩包中未找到 onnxruntime.dll".into());
    };
    if let Ok(status) = extract_status {
        if status.success() {
            fs::copy(&dll, ort_dll_path(base)).map_err(to_string_error)?;
            let _ = fs::remove_dir_all(&tmp_dir);
            return Ok(());
        }
    }
    last_error = "解压 onnxruntime 压缩包失败".into();
    let _ = fs::remove_dir_all(&tmp_dir);
    Err(format!("获取 onnxruntime 运行库失败：{last_error}"))
}

pub(crate) fn find_file(dir: &Path, name: &str) -> Option<PathBuf> {
    let entries = fs::read_dir(dir).ok()?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_file() {
            // 用 OsStr 相等比较：遇到任一非 UTF-8 文件名不能让整棵搜索短路
            // 放弃（旧写法 to_str()? 会把整次查找误报为“未找到”）。
            if path.file_name() == Some(std::ffi::OsStr::new(name)) {
                return Some(path);
            }
        } else if let Some(found) = find_file(&path, name) {
            return Some(found);
        }
    }
    None
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelProgress {
    pub(crate) model_id: String,
    pub(crate) file: String,
    pub(crate) completed: u64,
    pub(crate) total: u64,
}

pub(crate) fn emit_progress(app: Option<&AppHandle>, progress: ModelProgress) {
    if let Some(app) = app {
        let _ = app.emit("model-progress", progress);
    }
}

// Downloads (curl.exe ships with Windows 10+; HTTPS without extra crates)
enum CurlDownloadError {
    /// 服务端不支持 Range（curl 退出码 33）：重新从零下载一次。
    RangeUnsupported,
    Message(String),
}

/// 先下载到 `{dest}.part`（有已下载内容时 `-C -` 续传），成功后校验大小并改名到
/// dest；失败保留 .part 供换镜像后续传。服务端不支持续传时删掉重下一遍。
pub(crate) fn curl_download(
    url: &str,
    dest: &Path,
    expected_size: Option<u64>,
    on_progress: &dyn Fn(u64, u64),
) -> Result<(), String> {
    let mut part_name = dest.file_name().ok_or("下载目标缺少文件名")?.to_os_string();
    part_name.push(".part");
    let part = dest.with_file_name(part_name);
    match curl_download_to(url, &part, expected_size, on_progress) {
        Ok(()) => {}
        Err(CurlDownloadError::RangeUnsupported) => {
            let _ = fs::remove_file(&part);
            match curl_download_to(url, &part, expected_size, on_progress) {
                Ok(()) => {}
                Err(CurlDownloadError::RangeUnsupported) => {
                    let _ = fs::remove_file(&part);
                    return Err("下载失败：服务器不支持断点续传。".into());
                }
                Err(CurlDownloadError::Message(message)) => return Err(message),
            }
        }
        Err(CurlDownloadError::Message(message)) => return Err(message),
    }
    fs::rename(&part, dest).map_err(to_string_error)?;
    Ok(())
}

fn curl_download_to(
    url: &str,
    part: &Path,
    expected_size: Option<u64>,
    on_progress: &dyn Fn(u64, u64),
) -> Result<(), CurlDownloadError> {
    let resume = fs::metadata(part).map(|meta| meta.len()).unwrap_or(0) > 0;
    // speed-time 是卡死解药：60 秒无进展即失败，外层循环换下一镜像；max-time 只做兜底。
    let mut command = crate::safety::quiet_command("curl");
    command.args([
        "-sSL",
        "--fail",
        "--retry",
        "3",
        "--retry-delay",
        "2",
        "--connect-timeout",
        "30",
        "--speed-limit",
        "1024",
        "--speed-time",
        "60",
        "--max-time",
        "3600",
    ]);
    if resume {
        command.args(["-C", "-"]);
    }
    let mut child = command
        .args(["-o"])
        .arg(part)
        .arg(url)
        .spawn()
        .map_err(|error| CurlDownloadError::Message(format!("无法启动 curl：{error}")))?;
    let pid = child.id();

    let total = head_content_length(url).unwrap_or(0);
    loop {
        std::thread::sleep(Duration::from_millis(300));
        match child.try_wait() {
            Ok(Some(status)) => {
                let size = fs::metadata(part).map(|meta| meta.len()).unwrap_or(0);
                if !status.success() {
                    if status.code() == Some(33) {
                        return Err(CurlDownloadError::RangeUnsupported);
                    }
                    // 23/26 = 写入失败：目标目录无写权限或磁盘满，给出可行动的提示。
                    if matches!(status.code(), Some(23) | Some(26)) {
                        return Err(CurlDownloadError::Message(
                            "下载失败：无法写入目标目录，请检查磁盘权限与剩余空间。".into(),
                        ));
                    }
                    return Err(CurlDownloadError::Message(format!(
                        "下载失败（curl 退出码 {}）",
                        status.code().unwrap_or(-1)
                    )));
                }
                if size == 0 {
                    return Err(CurlDownloadError::Message("下载失败：文件为空。".into()));
                }
                if let Some(expected) = expected_size {
                    if size != expected {
                        // 续传拼接后仍不完整或远端内容已变化：删掉 .part，避免每次
                        // 都在错误的基线上续传。
                        let _ = fs::remove_file(part);
                        return Err(CurlDownloadError::Message(format!(
                            "下载失败：文件大小不匹配（期望 {expected} 字节，实际 {size} 字节）。"
                        )));
                    }
                }
                on_progress(size, if total > 0 { total } else { size });
                return Ok(());
            }
            Ok(None) => {
                let size = fs::metadata(part).map(|meta| meta.len()).unwrap_or(0);
                on_progress(size, if total > 0 { total } else { size });
            }
            Err(error) => {
                let _ = crate::safety::quiet_command("taskkill")
                    .args(["/PID", &pid.to_string(), "/F"])
                    .status();
                return Err(CurlDownloadError::Message(format!("下载过程出错：{error}")));
            }
        }
    }
}

pub(crate) fn head_content_length(url: &str) -> Option<u64> {
    let output = crate::safety::quiet_command("curl")
        .args(["-sI", "-L", "--max-time", "20"])
        .arg(url)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    text.lines()
        .filter_map(|line| {
            let (key, value) = line.split_once(':')?;
            if key.trim().eq_ignore_ascii_case("content-length") {
                value.trim().parse::<u64>().ok()
            } else {
                None
            }
        })
        .next_back()
        .filter(|value| *value > 0)
}

pub fn download_model(app: Option<&AppHandle>, base: &Path, id: &str) -> Result<(), String> {
    let spec = model_spec(id)?;
    download_model_files(app, base, id, spec.files)
}

/// 下载不参与主模型选择器的可选发丝边缘精修器。
pub fn download_hair_refiner(app: Option<&AppHandle>, base: &Path) -> Result<(), String> {
    download_model_files(app, base, HAIR_REFINER_ID, HAIR_REFINER_FILES)
}

fn download_model_files(
    app: Option<&AppHandle>,
    base: &Path,
    id: &str,
    files: &[ModelFileSpec],
) -> Result<(), String> {
    fs::create_dir_all(models_dir(base)).map_err(to_string_error)?;
    for file in files {
        let dest = models_dir(base).join(file.name);
        let already_ok =
            dest.exists() && fs::metadata(&dest).map(|meta| meta.len()).unwrap_or(0) == file.size;
        if already_ok {
            continue;
        }
        let progress = |completed: u64, total: u64| {
            emit_progress(
                app,
                ModelProgress {
                    model_id: id.to_string(),
                    file: file.name.to_string(),
                    completed,
                    total,
                },
            );
        };
        let urls = [file.mirror_url, file.origin_url];
        let mut last_error = String::new();
        for url in urls {
            match curl_download(url, &dest, Some(file.size), &progress) {
                Ok(()) => {
                    // 固化了 SHA256 的文件在下载后校验完整性：仅靠字节数挡不住
                    // 同尺寸的损坏或被替换文件。失败即删档报错，让外层回退镜像。
                    if !file.sha256.is_empty() {
                        match crate::model_bake::sha256_of_file(&dest) {
                            Ok(actual) if actual == file.sha256 => break,
                            Ok(actual) => {
                                let _ = fs::remove_file(&dest);
                                last_error = format!(
                                    "SHA256 不匹配（实际 {actual}，期望 {}）",
                                    file.sha256
                                );
                                continue;
                            }
                            Err(error) => {
                                let _ = fs::remove_file(&dest);
                                last_error = format!("校验读取失败：{error}");
                                continue;
                            }
                        }
                    }
                    break;
                }
                Err(error) => {
                    last_error = error;
                    if dest.exists() {
                        let _ = fs::remove_file(&dest);
                    }
                }
            }
        }
        if !dest.exists() {
            return Err(format!("下载 {} 失败：{last_error}", file.name));
        }
    }
    Ok(())
}

pub fn uninstall_model(base: &Path, id: &str) -> Result<(), String> {
    let spec = model_spec(id)?;
    // Release cached sessions first so Windows lets us delete the files.
    match spec.kind {
        ModelKind::Simple => {
            simple_slot().lock().map_err(lock_error)?.take();
        }
        ModelKind::Advanced => {
            advanced_slot().lock().map_err(lock_error)?.take();
        }
        // BiRefNet 系（含 toonout）共用按 id 缓存的会话仓库。
        ModelKind::Toonout | ModelKind::BiRefNet { .. } => release_birefnet_session(id),
    }
    let mut errors = Vec::new();
    for file in spec.files {
        let path = models_dir(base).join(file.name);
        if path.exists() {
            if let Err(error) = fs::remove_file(&path) {
                errors.push(format!("删除 {} 失败：{error}", file.name));
            }
        }
        // 顺带清掉中断下载留下的 .part 残片。
        let part = part_path(&path);
        if part.exists() {
            let _ = fs::remove_file(&part);
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("；"))
    }
}

pub fn uninstall_hair_refiner(base: &Path) -> Result<(), String> {
    // 先释放 ONNX 会话，确保 Windows 不会锁住模型文件。
    release_vitmatte_session();
    let mut errors = Vec::new();
    for file in HAIR_REFINER_FILES {
        let path = models_dir(base).join(file.name);
        if path.exists() {
            if let Err(error) = fs::remove_file(&path) {
                errors.push(format!("删除 {} 失败：{error}", file.name));
            }
        }
        let part = part_path(&path);
        if part.exists() {
            let _ = fs::remove_file(&part);
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("；"))
    }
}

/// `.part` 临时文件路径（与 curl_download 的命名一致）。
pub(crate) fn part_path(dest: &Path) -> PathBuf {
    let mut name = dest.file_name().unwrap_or_default().to_os_string();
    name.push(".part");
    dest.with_file_name(name)
}

pub(crate) fn lock_error<T>(_: std::sync::PoisonError<T>) -> String {
    "会话锁已损坏".into()
}

// Sessions
pub(crate) struct SimpleSessions {
    pub(crate) session: Session,
}

pub(crate) struct AdvancedSessions {
    pub(crate) seg: Session,
    pub(crate) refine: Session,
}

pub(crate) static SIMPLE_SESSION: OnceLock<Mutex<Option<SimpleSessions>>> = OnceLock::new();

pub(crate) static ADVANCED_SESSIONS: OnceLock<Mutex<Option<AdvancedSessions>>> = OnceLock::new();

/// ViTMatte 与主分割模型串行使用。单独缓存其会话并纳入 `prune_sessions`，避免
/// 两个视觉 Transformer 同时占住显存而在中端显卡上触发 OOM。
pub(crate) static VITMATTE_SESSION: OnceLock<Mutex<Option<Session>>> = OnceLock::new();

/// BiRefNet 系（toonout + birefnet-*）会话按模型 id 缓存：这些模型共享同一条
/// 推理管线，仅输入分辨率与后处理策略不同。
pub(crate) static BIREFNET_SESSIONS: OnceLock<Mutex<Vec<(String, Session)>>> = OnceLock::new();

pub(crate) fn simple_slot() -> &'static Mutex<Option<SimpleSessions>> {
    SIMPLE_SESSION.get_or_init(|| Mutex::new(None))
}

pub(crate) fn advanced_slot() -> &'static Mutex<Option<AdvancedSessions>> {
    ADVANCED_SESSIONS.get_or_init(|| Mutex::new(None))
}

pub(crate) fn vitmatte_slot() -> &'static Mutex<Option<Session>> {
    VITMATTE_SESSION.get_or_init(|| Mutex::new(None))
}

pub(crate) fn birefnet_sessions() -> &'static Mutex<Vec<(String, Session)>> {
    BIREFNET_SESSIONS.get_or_init(|| Mutex::new(Vec::new()))
}

/// 释放指定 BiRefNet 系模型的缓存会话（卸载文件、回退复核前腾显存时调用）。
pub(crate) fn release_birefnet_session(id: &str) {
    if let Ok(mut sessions) = birefnet_sessions().lock() {
        sessions.retain(|(model_id, _)| model_id != id);
    }
}

pub(crate) fn release_vitmatte_session() {
    if let Ok(mut session) = vitmatte_slot().lock() {
        session.take();
    }
}

/// 推理会话全局只保留当前在用的一套，且抠图与超分两族跨功能互斥。这几组模型
/// 各自常驻 1-3GB 显存（BiRefNet 系 0.5-1GB 权重 + 大激活张量，RTMDet+精修是
/// 两个模型，RealESRGAN 超分同样独立常驻一族），同时缓存多套会在切换模型或
/// 功能时把显存挤爆：GPU OOM 转内存重试还可能进一步耗尽主机内存，直接把进程
/// 带崩。每次推理前调用，保留 keep，释放其余；切换模型或功能后首次推理多花
/// 一次几秒的会话重建。
pub(crate) enum SessionKeep<'a> {
    Birefnet(&'a str),
    Simple,
    Advanced,
    VitMatte,
    Superres(&'a str),
}

pub(crate) fn prune_sessions(keep: SessionKeep<'_>) {
    let keep_birefnet = match &keep {
        SessionKeep::Birefnet(id) => Some(*id),
        _ => None,
    };
    if let Ok(mut sessions) = birefnet_sessions().lock() {
        sessions.retain(|(model_id, _)| Some(model_id.as_str()) == keep_birefnet);
    }
    if !matches!(keep, SessionKeep::Simple) {
        if let Ok(mut slot) = simple_slot().lock() {
            *slot = None;
        }
    }
    if !matches!(keep, SessionKeep::Advanced) {
        if let Ok(mut slot) = advanced_slot().lock() {
            *slot = None;
        }
    }
    if !matches!(keep, SessionKeep::VitMatte) {
        if let Ok(mut slot) = vitmatte_slot().lock() {
            *slot = None;
        }
    }
    // 跨功能互斥的另一半：保留超分时按模型前缀只留当前槽位，其余情况全部清空。
    // 此时本函数持有的动漫家族锁均已释放，与超分会话锁无嵌套，不会死锁。
    match &keep {
        SessionKeep::Superres(id) => crate::superres::retain_session(id),
        _ => crate::superres::release_all_sessions(),
    }
}

/// CUDA 注册失败但已回退 CPU 的真实原因（None = 未回退或未尝试）。
/// ORT 默认静默回退会让 UI 误报「GPU 加速已生效」，用户在 CPU 上跑大图
/// 却以为在用显卡；error_on_failure 拿到真实错误后在这里透出。
pub(crate) static CUDA_FALLBACK_REASON: std::sync::LazyLock<
    std::sync::RwLock<Option<String>>,
> = std::sync::LazyLock::new(|| std::sync::RwLock::new(None));

pub(crate) fn cuda_fallback_reason() -> Option<String> {
    CUDA_FALLBACK_REASON
        .read()
        .ok()
        .map(|guard| guard.clone())
        .flatten()
}

pub(crate) fn build_session(path: &Path, use_gpu: bool) -> Result<Session, String> {
    let threads = std::thread::available_parallelism()
        .map(|value| value.get().clamp(1, 16))
        .unwrap_or(4);
    preload_cuda_runtime();
    let mut builder = Session::builder()
        .map_err(to_string_error)?
        .with_optimization_level(GraphOptimizationLevel::Level3)
        .map_err(to_string_error)?
        .with_intra_threads(threads)
        .map_err(to_string_error)?;
    // CPU 中间张量默认驻留 BFC arena：涨到该会话的峰值后永不归还系统，表现为
    // 推理结束后内存不回落。关掉后每次推理结束即归还 OS，代价是少量 malloc/free
    // 开销（会话输入形状固定，中间张量少而大，开销可忽略）。
    let cpu_ep = || ort::ep::CPU::default().with_arena_allocator(false).build();
    // 仅当 ONNX Runtime 确实编译了 CUDA EP 时才注册；CPU 版运行库注册只会静默回退并掩盖真实状态。
    if use_gpu && cuda_ep_compiled() {
        // Arena 按需扩展、cuDNN 改启发式搜索并限制 workspace：BiRefNet 官方 fp32
        // 导出在 1024² 全分辨率上有单笔约 784MB 的中间张量，默认的 2 的幂超额
        // arena 与穷举卷积搜索会在显存紧张的卡上触发不必要的 OOM。
        let cuda = ort::ep::CUDA::default()
            .with_arena_extend_strategy(ort::ep::ArenaExtendStrategy::SameAsRequested)
            .with_conv_algorithm_search(ort::ep::cuda::ConvAlgorithmSearch::Heuristic)
            .with_conv_max_workspace(false)
            .build()
            .error_on_failure();
        let gpu_attempt = builder
            .with_execution_providers([cuda, cpu_ep()])
            .map_err(to_string_error)?
            .commit_from_file(path)
            .map_err(|error| format!("加载模型 {} 失败：{error}", path.display()));
        match gpu_attempt {
            Ok(session) => {
                if let Ok(mut guard) = CUDA_FALLBACK_REASON.write() {
                    *guard = None;
                }
                return Ok(session);
            }
            Err(error) => {
                // cuDNN 缺失/驱动过老/显存不足：记录真实原因，回退 CPU 重建。
                if let Ok(mut guard) = CUDA_FALLBACK_REASON.write() {
                    *guard = Some(error.clone());
                }
                builder = Session::builder()
                    .map_err(to_string_error)?
                    .with_optimization_level(GraphOptimizationLevel::Level3)
                    .map_err(to_string_error)?
                    .with_intra_threads(threads)
                    .map_err(to_string_error)?;
            }
        }
    }
    builder
        .with_execution_providers([cpu_ep()])
        .map_err(to_string_error)?
        .commit_from_file(path)
        .map_err(|error| format!("加载模型 {} 失败：{error}", path.display()))
}

pub(crate) const CUDNN9_FILES: &[&str] = &[
    "cudnn64_9.dll",
    "cudnn_graph64_9.dll",
    "cudnn_ops64_9.dll",
    "cudnn_heuristic64_9.dll",
    "cudnn_adv64_9.dll",
    "cudnn_cnn64_9.dll",
    "cudnn_engines_precompiled64_9.dll",
    "cudnn_engines_runtime_compiled64_9.dll",
];

/// GPU 加速运行库（onnxruntime-gpu PyPI wheel，仅抽取 3 个必需 DLL）。
/// 官方 GPU 版 onnxruntime 与 CPU 版核心 dll 同名，本机种子无法用文件名区分真伪，
/// 因此 GPU 运行库一律从下方固定来源下载，绝不信任种子目录里的副本。
pub(crate) const GPU_RUNTIME_MODEL_ID: &str = "ort-gpu";

pub(crate) const GPU_ORT_WHEEL_NAME: &str = "onnxruntime_gpu-1.23.2-cp312-cp312-win_amd64.whl";

pub(crate) const GPU_ORT_WHEEL_SIZE: u64 = 244_508_327;

pub(crate) const GPU_ORT_WHEEL_PATH: &str = "packages/87/da/2685c79e5ea587beddebe083601fead0bdf3620bc2f92d18756e7de8a636/onnxruntime_gpu-1.23.2-cp312-cp312-win_amd64.whl";

/// PyPI 官方发布的该 wheel SHA256（pypi.org/pypi/onnxruntime-gpu/1.23.2/json）。
pub(crate) const GPU_ORT_WHEEL_SHA256: &str =
    "fe925a84b00e291e0ad3fac29bfd8f8e06112abc760cdc82cb711b4f3935bd95";

/// 前两个为国内 PyPI 镜像，最后一个为官方源；路径在三家完全一致。
pub(crate) const GPU_ORT_HOSTS: &[&str] = &[
    "https://pypi.tuna.tsinghua.edu.cn",
    "https://mirrors.aliyun.com/pypi",
    "https://files.pythonhosted.org",
];

pub(crate) const GPU_ORT_DLLS: &[&str] = &[
    "onnxruntime.dll",
    "onnxruntime_providers_cuda.dll",
    "onnxruntime_providers_shared.dll",
];

pub fn gpu_ort_capi_dir(base: &Path) -> PathBuf {
    base.join("onnxruntime-gpu")
        .join("onnxruntime")
        .join("capi")
}

pub fn gpu_ort_ready(base: &Path) -> bool {
    let dir = gpu_ort_capi_dir(base);
    GPU_ORT_DLLS.iter().all(|name| dir.join(name).exists())
}

/// 加载中的 ORT 是否编译了 CUDA EP（需要 dll 已按 ORT_DYLIB_PATH 加载后才准确）。
pub fn cuda_ep_compiled() -> bool {
    use ort::ep::ExecutionProvider;
    ort::ep::CUDA::default().is_available().unwrap_or(false)
}

/// 下载并安装 GPU 版 onnxruntime（幂等；约 233 MB，一次性）。
pub fn install_gpu_ort(app: Option<&AppHandle>, base: &Path) -> Result<(), String> {
    if gpu_ort_ready(base) {
        return Ok(());
    }
    let gpu_root = base.join("onnxruntime-gpu");
    fs::create_dir_all(&gpu_root).map_err(to_string_error)?;
    let archive = gpu_root.join(GPU_ORT_WHEEL_NAME);
    let progress = |completed: u64, total: u64| {
        emit_progress(
            app,
            ModelProgress {
                model_id: GPU_RUNTIME_MODEL_ID.to_string(),
                file: GPU_ORT_WHEEL_NAME.to_string(),
                completed,
                total,
            },
        );
    };
    let mut last_error = String::from("没有可用的下载地址。");
    for host in GPU_ORT_HOSTS {
        let url = format!("{host}/{GPU_ORT_WHEEL_PATH}");
        match curl_download(&url, &archive, Some(GPU_ORT_WHEEL_SIZE), &progress) {
            Ok(()) => {
                // 244MB 的 wheel 只查字节数不够：SHA256 不匹配即删档换下一镜像。
                match crate::model_bake::sha256_of_file(&archive) {
                    Ok(actual) if actual == GPU_ORT_WHEEL_SHA256 => {
                        last_error = String::new();
                        break;
                    }
                    Ok(actual) => {
                        let _ = fs::remove_file(&archive);
                        last_error = format!(
                            "SHA256 不匹配（期望 {GPU_ORT_WHEEL_SHA256}，实际 {actual}）"
                        );
                    }
                    Err(error) => {
                        let _ = fs::remove_file(&archive);
                        last_error = format!("校验读取失败：{error}");
                    }
                }
            }
            Err(error) => last_error = error,
        }
    }
    if !last_error.is_empty() {
        return Err(last_error);
    }
    // 解压失败也要清掉已校验完好的 wheel：留档不仅占 244MB，还会让重试的
    // 续传基准（.whl.part）对不上而全量重下——本地档其实可以直接复用。
    if let Err(error) = extract_gpu_ort_dlls(&archive, &gpu_root) {
        let _ = fs::remove_file(&archive);
        return Err(error);
    }
    let _ = fs::remove_file(&archive);
    if !gpu_ort_ready(base) {
        return Err("GPU 运行库解压后不完整，请重新下载。".into());
    }
    Ok(())
}

pub(crate) fn extract_gpu_ort_dlls(archive: &Path, dest: &Path) -> Result<(), String> {
    // 必须用 Windows 自带的 bsdtar（支持 zip）；GNU tar 无法读取 wheel。
    let tar = std::env::var_os("WINDIR")
        .map(|windir| PathBuf::from(windir).join(r"System32\tar.exe"))
        .filter(|path| path.exists())
        .unwrap_or_else(|| PathBuf::from("tar"));
    let output = crate::safety::quiet_command(tar)
        .arg("-xf")
        .arg(archive)
        .arg("-C")
        .arg(dest)
        .args([
            "onnxruntime/capi/onnxruntime.dll",
            "onnxruntime/capi/onnxruntime_providers_cuda.dll",
            "onnxruntime/capi/onnxruntime_providers_shared.dll",
        ])
        .output()
        .map_err(|error| format!("无法启动 tar 解压：{error}"))?;
    if !output.status.success() {
        return Err(format!(
            "解压 GPU 运行库失败：{}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(())
}

/// 定位 CUDA 12 与完整 cuDNN 9 运行时并预载；找不到时由 ONNX Runtime 回退 CPU。
pub(crate) fn preload_cuda_runtime() {
    let cuda_root = cuda_runtime_candidates().into_iter().find(|path| {
        [
            "cudart64_12.dll",
            "cublasLt64_12.dll",
            "cublas64_12.dll",
            "cufft64_11.dll",
        ]
        .iter()
        .all(|name| path.join(name).exists())
    });
    let cudnn_root = cudnn_runtime_candidates().into_iter().find(|path| {
        // cuDNN 运行时会按需加载 cublasLt64_1X 等依赖，找不到直接 abort 进程；
        // 因此候选目录必须自带这些依赖，否则视为不可用。
        CUDNN9_FILES.iter().all(|name| path.join(name).exists())
            && dir_contains_prefix(path, "cublasLt64_")
            && dir_contains_prefix(path, "cudart64_")
    });
    if let Some(dir) = &cuda_root {
        for name in ort::ep::cuda::CUDA_DYLIBS {
            let _ = ort::util::preload_dylib(dir.join(name));
        }
    }
    if let Some(dir) = &cudnn_root {
        // cuDNN 内部按自身构建版本（CUDA 12 或 13）动态加载 cublasLt64_1X 等，
        // 缺一个就会直接 abort 进程；torch 在 Python 里的做法同样是把目录插到 PATH
        // 最前，这里保持一致，覆盖 cuDNN 各种内部搜索策略。
        let mut search_dirs: Vec<PathBuf> = vec![dir.clone()];
        if let Some(cuda_dir) = &cuda_root {
            search_dirs.push(cuda_dir.clone());
        }
        prepend_dll_search_paths(&search_dirs);
        preload_dir_cuda_variants(dir);
        for name in ort::ep::cuda::CUDNN_DYLIBS {
            let _ = ort::util::preload_dylib(dir.join(name));
        }
    }
}

/// 把若干目录插到当前进程 PATH 最前（去重），供后续 LoadLibrary 命中。
pub(crate) fn prepend_dll_search_paths(dirs: &[PathBuf]) {
    let current: Vec<PathBuf> = std::env::var_os("PATH")
        .map(|value| std::env::split_paths(&value).collect())
        .unwrap_or_default();
    let mut additions: Vec<PathBuf> = dirs
        .iter()
        .filter(|dir| dir.is_dir())
        .map(|dir| dir.to_path_buf())
        .filter(|dir| !current.iter().any(|path| path == dir))
        .collect();
    if additions.is_empty() {
        return;
    }
    additions.extend(current);
    if let Ok(value) = std::env::join_paths(&additions) {
        std::env::set_var("PATH", value);
    }
}

/// 判断目录内是否存在以指定前缀命名的文件（不区分大小写）。
pub(crate) fn dir_contains_prefix(dir: &Path, prefix: &str) -> bool {
    let prefix = prefix.to_ascii_lowercase();
    fs::read_dir(dir)
        .map(|entries| {
            entries.flatten().any(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .to_ascii_lowercase()
                    .starts_with(&prefix)
            })
        })
        .unwrap_or(false)
}

/// 预载目录内全部 cudart64_*/cublas64_*/cublasLt64_*/cufft64_* DLL（任意主版本）。
pub(crate) fn preload_dir_cuda_variants(dir: &Path) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    let mut paths: Vec<PathBuf> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .map(|name| {
                    let lower = name.to_ascii_lowercase();
                    (lower.starts_with("cudart64_")
                        || lower.starts_with("cublas64_")
                        || lower.starts_with("cublasLt64_")
                        || lower.starts_with("cufft64_"))
                        && lower.ends_with(".dll")
                })
                .unwrap_or(false)
        })
        .collect();
    paths.sort();
    for path in paths {
        let _ = ort::util::preload_dylib(path);
    }
}

pub(crate) fn cuda_runtime_candidates() -> Vec<PathBuf> {
    let mut candidates = env_runtime_paths("AIAS_CUDA12_DIR");
    if let Some(home) = dirs::home_dir() {
        let python_root = home.join(r"AppData\Local\Programs\Python");
        if let Ok(entries) = fs::read_dir(python_root) {
            candidates.extend(
                entries
                    .flatten()
                    .map(|entry| entry.path().join(r"Lib\site-packages\torch\lib")),
            );
        }
    }
    for root in local_ai_roots() {
        candidates.push(root.join(r"forge\venv\Lib\site-packages\torch\lib"));
        candidates.push(root.join(r"ComfyUI\venv\Lib\site-packages\torch\lib"));
        candidates.push(root.join(r"ComfyUI\python_embeded\Lib\site-packages\torch\lib"));
    }
    for version in ["v12.9", "v12.8", "v12.6", "v12.4"] {
        candidates.push(
            PathBuf::from(r"C:\Program Files\NVIDIA GPU Computing Toolkit\CUDA")
                .join(version)
                .join("bin"),
        );
    }
    candidates
}

pub(crate) fn cudnn_runtime_candidates() -> Vec<PathBuf> {
    let mut candidates = env_runtime_paths("AIAS_CUDNN9_DIR");
    if let Some(home) = dirs::home_dir() {
        let python_root = home.join(r"AppData\Local\Programs\Python");
        if let Ok(entries) = fs::read_dir(python_root) {
            candidates.extend(
                entries
                    .flatten()
                    .map(|entry| entry.path().join(r"Lib\site-packages\torch\lib")),
            );
        }
    }
    for root in local_ai_roots() {
        candidates.push(root.join(r"ComfyUI\venv\Lib\site-packages\torch\lib"));
        candidates.push(root.join(r"ComfyUI\python_embeded\Lib\site-packages\torch\lib"));
        candidates.push(root.join(r"forge\venv\Lib\site-packages\torch\lib"));
    }
    for version in ["v12.9", "v12.8", "v12.6", "v12.4"] {
        candidates.push(
            PathBuf::from(r"C:\Program Files\NVIDIA GPU Computing Toolkit\CUDA")
                .join(version)
                .join("bin"),
        );
    }
    candidates
}

pub(crate) fn env_runtime_paths(name: &str) -> Vec<PathBuf> {
    std::env::var_os(name)
        .into_iter()
        .map(PathBuf::from)
        .collect()
}

pub(crate) fn local_ai_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    for drive in ['C', 'D', 'E', 'F', 'G'] {
        roots.push(PathBuf::from(format!(r"{drive}:\WebUI")));
        roots.push(PathBuf::from(format!(r"{drive}:\")));
    }
    roots
}

/// 显存/内存不足的报错来自 ONNX Runtime 的 CUDA arena、cudaMalloc 或 host 端
/// std::bad_alloc（部分算子如 GatherND 无 CUDA 内核，会落在内存执行）；关键词保持宽松。
pub(crate) fn is_gpu_oom_error(error: &str) -> bool {
    let message = error.to_lowercase();
    [
        "failed to allocate",
        "bad allocation",
        "bad_alloc",
        "out of memory",
        "out_of_memory",
        "cuda error",
        // cuDNN 执行失败（5003）常见于显存/工作区不足，按 GPU 故障处理回退 CPU。
        "cudnn",
    ]
    .iter()
    .any(|needle| message.contains(needle))
}
