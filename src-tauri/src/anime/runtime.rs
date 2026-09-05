//! `anime::runtime` — 拆分自 anime.rs，职责见模块内条目注释。

use super::*;

use ort::session::{builder::GraphOptimizationLevel, Session};
use serde::Serialize;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;
use tauri::{AppHandle, Emitter};

// ONNX Runtime acquisition (onnxruntime.dll next to the app data)
pub(crate) fn ort_dll_path(base: &Path) -> PathBuf {
    base.join("onnxruntime.dll")
}

pub(crate) static ORT_READY: OnceLock<()> = OnceLock::new();

pub fn ensure_ort_runtime(base: &Path) -> Result<(), String> {
    fs::create_dir_all(base).map_err(to_string_error)?;
    // 进程内 ORT 只会加载一次 dll：优先使用完整 GPU 版运行库，其次才是种子/下载的 CPU 版。
    let dll = if gpu_ort_ready(base) {
        gpu_ort_capi_dir(base).join("onnxruntime.dll")
    } else {
        ort_dll_path(base)
    };
    if !dll.exists() {
        acquire_ort_dll(base)?;
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

pub(crate) fn acquire_ort_dll(base: &Path) -> Result<(), String> {
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
    let mut last_error = String::from("无可用下载源");
    for url in archive_urls {
        match curl_download(&url, &archive, None, &|_, _| {}) {
            Ok(()) => {
                let status = Command::new("tar")
                    .args(["-xf"])
                    .arg(&archive)
                    .arg("-C")
                    .arg(&tmp_dir)
                    .status();
                let dll = find_file(&tmp_dir, "onnxruntime.dll")
                    .ok_or_else(|| "压缩包中未找到 onnxruntime.dll".to_string())?;
                if let Ok(status) = status {
                    if status.success() {
                        fs::copy(&dll, ort_dll_path(base)).map_err(to_string_error)?;
                        let _ = fs::remove_dir_all(&tmp_dir);
                        return Ok(());
                    }
                }
                last_error = "解压 onnxruntime 压缩包失败".into();
            }
            Err(error) => last_error = error,
        }
    }
    let _ = fs::remove_dir_all(&tmp_dir);
    Err(format!("获取 onnxruntime 运行库失败：{last_error}"))
}

pub(crate) fn find_file(dir: &Path, name: &str) -> Option<PathBuf> {
    let entries = fs::read_dir(dir).ok()?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_file() {
            if path.file_name()?.to_str()? == name {
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
    model_id: String,
    file: String,
    completed: u64,
    total: u64,
}

pub(crate) fn emit_progress(app: Option<&AppHandle>, progress: ModelProgress) {
    if let Some(app) = app {
        let _ = app.emit("model-progress", progress);
    }
}

// Downloads (curl.exe ships with Windows 10+; HTTPS without extra crates)
pub(crate) fn curl_download(
    url: &str,
    dest: &Path,
    expected_size: Option<u64>,
    on_progress: &dyn Fn(u64, u64),
) -> Result<(), String> {
    if dest.exists() {
        fs::remove_file(dest).map_err(to_string_error)?;
    }
    let mut child = Command::new("curl")
        .args(["-sSL", "--fail", "--retry", "3", "--retry-delay", "2", "-o"])
        .arg(dest)
        .arg(url)
        .spawn()
        .map_err(|error| format!("无法启动 curl：{error}"))?;
    let pid = child.id();

    let total = head_content_length(url).unwrap_or(0);
    loop {
        std::thread::sleep(Duration::from_millis(300));
        match child.try_wait() {
            Ok(Some(status)) => {
                let size = fs::metadata(dest).map(|meta| meta.len()).unwrap_or(0);
                if !status.success() {
                    let _ = fs::remove_file(dest);
                    return Err(format!(
                        "下载失败（curl 退出码 {}）",
                        status.code().unwrap_or(-1)
                    ));
                }
                if size == 0 {
                    return Err("下载失败：文件为空。".into());
                }
                if let Some(expected) = expected_size {
                    if size != expected {
                        let _ = fs::remove_file(dest);
                        return Err(format!(
                            "下载失败：文件大小不匹配（期望 {expected} 字节，实际 {size} 字节）。"
                        ));
                    }
                }
                on_progress(size, if total > 0 { total } else { size });
                return Ok(());
            }
            Ok(None) => {
                let size = fs::metadata(dest).map(|meta| meta.len()).unwrap_or(0);
                on_progress(size, if total > 0 { total } else { size });
            }
            Err(error) => {
                let _ = Command::new("taskkill")
                    .args(["/PID", &pid.to_string(), "/F"])
                    .status();
                return Err(format!("下载过程出错：{error}"));
            }
        }
    }
}

pub(crate) fn head_content_length(url: &str) -> Option<u64> {
    let output = Command::new("curl")
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
                Ok(()) => break,
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
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("；"))
    }
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

/// 推理会话全局只保留当前在用的一套。这几组模型各自常驻 1-3GB 显存
/// （BiRefNet 系 0.5-1GB 权重 + 大激活张量，RTMDet+精修是两个模型），
/// 同时缓存多套会在切换模型时把显存挤爆：GPU OOM 转内存重试还可能
/// 进一步耗尽主机内存，直接把进程带崩。每次推理前调用，保留 keep，
/// 释放其余；切换模型后首次推理多花一次几秒的会话重建。
pub(crate) enum SessionKeep<'a> {
    Birefnet(&'a str),
    Simple,
    Advanced,
    VitMatte,
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
}

pub(crate) fn build_session(path: &Path, use_gpu: bool) -> Result<Session, String> {
    let threads = std::thread::available_parallelism()
        .map(|value| value.get().clamp(1, 8))
        .unwrap_or(4);
    preload_cuda_runtime();
    let mut builder = Session::builder()
        .map_err(to_string_error)?
        .with_optimization_level(GraphOptimizationLevel::Level3)
        .map_err(to_string_error)?
        .with_intra_threads(threads)
        .map_err(to_string_error)?;
    // 仅当 ONNX Runtime 确实编译了 CUDA EP 时才注册；CPU 版运行库注册只会静默回退并掩盖真实状态。
    if use_gpu && cuda_ep_compiled() {
        // Arena 按需扩展、cuDNN 改启发式搜索并限制 workspace：BiRefNet 官方 fp32
        // 导出在 1024² 全分辨率上有单笔约 784MB 的中间张量，默认的 2 的幂超额
        // arena 与穷举卷积搜索会在显存紧张的卡上触发不必要的 OOM。
        let cuda = ort::ep::CUDA::default()
            .with_arena_extend_strategy(ort::ep::ArenaExtendStrategy::SameAsRequested)
            .with_conv_algorithm_search(ort::ep::cuda::ConvAlgorithmSearch::Heuristic)
            .with_conv_max_workspace(false)
            .build();
        // CUDA EP 注册失败时 ONNX Runtime 仍会静默回退 CPU（error_on_failure 默认 false）。
        builder = builder
            .with_execution_providers([cuda])
            .map_err(to_string_error)?;
    }
    builder
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
                last_error = String::new();
                break;
            }
            Err(error) => last_error = error,
        }
    }
    if !last_error.is_empty() {
        return Err(last_error);
    }
    extract_gpu_ort_dlls(&archive, &gpu_root)?;
    let _ = fs::remove_file(&archive);
    if !gpu_ort_ready(base) {
        return Err("GPU 运行库解压后不完整，请重新下载。".into());
    }
    Ok(())
}

pub(crate) fn extract_gpu_ort_dlls(archive: &Path, dest: &Path) -> Result<(), String> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    // 必须用 Windows 自带的 bsdtar（支持 zip）；GNU tar 无法读取 wheel。
    let tar = std::env::var_os("WINDIR")
        .map(|windir| PathBuf::from(windir).join(r"System32\tar.exe"))
        .filter(|path| path.exists())
        .unwrap_or_else(|| PathBuf::from("tar"));
    let output = Command::new(tar)
        .arg("-xf")
        .arg(archive)
        .arg("-C")
        .arg(dest)
        .args([
            "onnxruntime/capi/onnxruntime.dll",
            "onnxruntime/capi/onnxruntime_providers_cuda.dll",
            "onnxruntime/capi/onnxruntime_providers_shared.dll",
        ])
        .creation_flags(CREATE_NO_WINDOW)
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
    ]
    .iter()
    .any(|needle| message.contains(needle))
}
