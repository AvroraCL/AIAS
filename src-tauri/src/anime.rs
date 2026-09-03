//! Anime matting models: catalog, download/uninstall into the app data folder,
//! and built-in ONNX inference (no ComfyUI required at runtime).

use image::imageops::FilterType;
use image::{ImageBuffer, Luma, RgbImage, Rgba, RgbaImage};
use ort::session::{builder::GraphOptimizationLevel, Session};
use ort::value::{Tensor, ValueType};
use serde::Serialize;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;
use tauri::{AppHandle, Emitter};

// ---------------------------------------------------------------------------
// Model catalog
// ---------------------------------------------------------------------------

pub struct ModelFileSpec {
    pub name: &'static str,
    pub size: u64,
    pub mirror_url: &'static str,
    pub origin_url: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelKind {
    /// BiRefNet 动漫微调（ToonOut），保留「整图判前景」自动回退链。
    Toonout,
    /// ISNet 动漫标准。
    Simple,
    /// RTMDet 检测 + 精修的动漫两阶段管线。
    Advanced,
    /// 官方 BiRefNet 通用系；matting = true 表示输出连续 alpha（人像/matting），
    /// 后处理不做对比度拉伸以保留半透明细节。
    BiRefNet { matting: bool },
}

pub struct ModelSpec {
    pub id: &'static str,
    pub label: &'static str,
    pub kind: ModelKind,
    pub files: &'static [ModelFileSpec],
}

const ISNETIS_SIZE: u64 = 176_069_933;
const RTMDET_SIZE: u64 = 238_686_077;
const REFINER_SIZE: u64 = 176_197_192;
const TOONOUT_SIZE: u64 = 492_381_880;
const BIREFNET_LITE_SIZE: u64 = 114_538_787;

pub const MODELS: &[ModelSpec] = &[
    ModelSpec {
        id: "toonout",
        label: "动漫特化（ToonOut）",
        kind: ModelKind::Toonout,
        files: &[ModelFileSpec {
            name: "birefnet-toonout-fp16.onnx",
            size: TOONOUT_SIZE,
            mirror_url: "https://hf-mirror.com/sprited/birefnet-toonout-onnx/resolve/main/birefnet-toonout-fp16.onnx",
            origin_url: "https://huggingface.co/sprited/birefnet-toonout-onnx/resolve/main/birefnet-toonout-fp16.onnx",
        }],
    },
    ModelSpec {
        id: "birefnet-general",
        label: "通用抠图（BiRefNet）",
        kind: ModelKind::BiRefNet { matting: false },
        files: &[ModelFileSpec {
            name: "BiRefNet-general-512-fp16.onnx",
            size: 940_526_436,
            mirror_url: "https://ghfast.top/https://github.com/ZhengPeng7/BiRefNet/releases/download/v1/BiRefNet-general-resolution_512x512-fp16-epoch_216.onnx",
            origin_url: "https://github.com/ZhengPeng7/BiRefNet/releases/download/v1/BiRefNet-general-resolution_512x512-fp16-epoch_216.onnx",
        }],
    },
    ModelSpec {
        id: "birefnet-lite",
        label: "轻量快速（BiRefNet Lite）",
        kind: ModelKind::BiRefNet { matting: false },
        files: &[ModelFileSpec {
            name: "birefnet-lite-fp16.onnx",
            size: BIREFNET_LITE_SIZE,
            mirror_url: "https://ghfast.top/https://github.com/AvroraCL/AIAS/releases/download/models-v1/birefnet-lite-fp16.onnx",
            origin_url: "https://github.com/AvroraCL/AIAS/releases/download/models-v1/birefnet-lite-fp16.onnx",
        }],
    },
    ModelSpec {
        id: "simple",
        label: "动漫标准（ISNet）",
        kind: ModelKind::Simple,
        files: &[ModelFileSpec {
            name: "isnetis.onnx",
            size: ISNETIS_SIZE,
            mirror_url: "https://hf-mirror.com/skytnt/anime-seg/resolve/main/isnetis.onnx",
            origin_url: "https://huggingface.co/skytnt/anime-seg/resolve/main/isnetis.onnx",
        }],
    },
    ModelSpec {
        id: "advanced",
        label: "动漫精细（RTMDet+精修）",
        kind: ModelKind::Advanced,
        files: &[
            ModelFileSpec {
                name: "anime_segmentor_rtmdet_e60_simplified.onnx",
                size: RTMDET_SIZE,
                mirror_url: "https://hf-mirror.com/Faor-Mati/anime-character-segmentation/resolve/main/anime_segmentor_rtmdet_e60_simplified.onnx",
                origin_url: "https://huggingface.co/Faor-Mati/anime-character-segmentation/resolve/main/anime_segmentor_rtmdet_e60_simplified.onnx",
            },
            ModelFileSpec {
                name: "mask_refiner_isnetdis_refine_last_simplified.onnx",
                size: REFINER_SIZE,
                mirror_url: "https://hf-mirror.com/Faor-Mati/anime-character-segmentation/resolve/main/mask_refiner_isnetdis_refine_last_simplified.onnx",
                origin_url: "https://huggingface.co/Faor-Mati/anime-character-segmentation/resolve/main/mask_refiner_isnetdis_refine_last_simplified.onnx",
            },
        ],
    },
];

fn model_spec(id: &str) -> Result<&'static ModelSpec, String> {
    MODELS
        .iter()
        .find(|model| model.id == id)
        .ok_or_else(|| format!("未知模型：{id}"))
}

pub fn models_dir(base: &Path) -> PathBuf {
    base.join("models").join("anime")
}

// ---------------------------------------------------------------------------
// Status
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelFileStatus {
    name: String,
    present: bool,
    size: u64,
    expected_size: u64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelStatus {
    pub id: String,
    pub label: String,
    pub installed: bool,
    pub total_size: u64,
    pub files: Vec<ModelFileStatus>,
}

pub fn models_status(base: &Path) -> Vec<ModelStatus> {
    let dir = models_dir(base);
    MODELS
        .iter()
        .map(|model| {
            let files: Vec<ModelFileStatus> = model
                .files
                .iter()
                .map(|file| {
                    let path = dir.join(file.name);
                    let present = path.exists();
                    let size = fs::metadata(&path).map(|meta| meta.len()).unwrap_or(0);
                    ModelFileStatus {
                        name: file.name.to_string(),
                        present,
                        size,
                        expected_size: file.size,
                    }
                })
                .collect();
            let installed = files
                .iter()
                .all(|file| file.present && file.size == file.expected_size);
            ModelStatus {
                id: model.id.to_string(),
                label: model.label.to_string(),
                installed,
                total_size: model.files.iter().map(|file| file.size).sum(),
                files,
            }
        })
        .collect()
}

pub fn is_model_ready(base: &Path, id: &str) -> bool {
    models_status(base)
        .into_iter()
        .find(|model| model.id == id)
        .map(|model| model.installed)
        .unwrap_or(false)
}

// ---------------------------------------------------------------------------
// ONNX Runtime acquisition (onnxruntime.dll next to the app data)
// ---------------------------------------------------------------------------

fn ort_dll_path(base: &Path) -> PathBuf {
    base.join("onnxruntime.dll")
}

static ORT_READY: OnceLock<()> = OnceLock::new();

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
fn ort_seed_candidates() -> Vec<PathBuf> {
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

fn acquire_ort_dll(base: &Path) -> Result<(), String> {
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

use std::process::Command;

fn find_file(dir: &Path, name: &str) -> Option<PathBuf> {
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

// ---------------------------------------------------------------------------
// Downloads (curl.exe ships with Windows 10+; HTTPS without extra crates)
// ---------------------------------------------------------------------------

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelProgress {
    model_id: String,
    file: String,
    completed: u64,
    total: u64,
}

fn emit_progress(app: Option<&AppHandle>, progress: ModelProgress) {
    if let Some(app) = app {
        let _ = app.emit("model-progress", progress);
    }
}

fn curl_download(
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

fn head_content_length(url: &str) -> Option<u64> {
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
    fs::create_dir_all(models_dir(base)).map_err(to_string_error)?;
    for file in spec.files {
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

fn lock_error<T>(_: std::sync::PoisonError<T>) -> String {
    "会话锁已损坏".into()
}

// ---------------------------------------------------------------------------
// Sessions
// ---------------------------------------------------------------------------

struct SimpleSessions {
    session: Session,
}

struct AdvancedSessions {
    seg: Session,
    refine: Session,
}

static SIMPLE_SESSION: OnceLock<Mutex<Option<SimpleSessions>>> = OnceLock::new();
static ADVANCED_SESSIONS: OnceLock<Mutex<Option<AdvancedSessions>>> = OnceLock::new();
/// BiRefNet 系（toonout + birefnet-*）会话按模型 id 缓存：这些模型共享同一条
/// 推理管线，仅输入分辨率与后处理策略不同。
static BIREFNET_SESSIONS: OnceLock<Mutex<Vec<(String, Session)>>> = OnceLock::new();

fn simple_slot() -> &'static Mutex<Option<SimpleSessions>> {
    SIMPLE_SESSION.get_or_init(|| Mutex::new(None))
}

fn advanced_slot() -> &'static Mutex<Option<AdvancedSessions>> {
    ADVANCED_SESSIONS.get_or_init(|| Mutex::new(None))
}

fn birefnet_sessions() -> &'static Mutex<Vec<(String, Session)>> {
    BIREFNET_SESSIONS.get_or_init(|| Mutex::new(Vec::new()))
}

/// 释放指定 BiRefNet 系模型的缓存会话（卸载文件、回退复核前腾显存时调用）。
fn release_birefnet_session(id: &str) {
    if let Ok(mut sessions) = birefnet_sessions().lock() {
        sessions.retain(|(model_id, _)| model_id != id);
    }
}

/// 推理会话全局只保留当前在用的一套。这几组模型各自常驻 1-3GB 显存
/// （BiRefNet 系 0.5-1GB 权重 + 大激活张量，RTMDet+精修是两个模型），
/// 同时缓存多套会在切换模型时把显存挤爆：GPU OOM 转内存重试还可能
/// 进一步耗尽主机内存，直接把进程带崩。每次推理前调用，保留 keep，
/// 释放其余；切换模型后首次推理多花一次几秒的会话重建。
enum SessionKeep<'a> {
    Birefnet(&'a str),
    Simple,
    Advanced,
}

fn prune_sessions(keep: SessionKeep<'_>) {
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
}

fn build_session(path: &Path, use_gpu: bool) -> Result<Session, String> {
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

const CUDNN9_FILES: &[&str] = &[
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
const GPU_RUNTIME_MODEL_ID: &str = "ort-gpu";
const GPU_ORT_WHEEL_NAME: &str = "onnxruntime_gpu-1.23.2-cp312-cp312-win_amd64.whl";
const GPU_ORT_WHEEL_SIZE: u64 = 244_508_327;
const GPU_ORT_WHEEL_PATH: &str = "packages/87/da/2685c79e5ea587beddebe083601fead0bdf3620bc2f92d18756e7de8a636/onnxruntime_gpu-1.23.2-cp312-cp312-win_amd64.whl";
/// 前两个为国内 PyPI 镜像，最后一个为官方源；路径在三家完全一致。
const GPU_ORT_HOSTS: &[&str] = &[
    "https://pypi.tuna.tsinghua.edu.cn",
    "https://mirrors.aliyun.com/pypi",
    "https://files.pythonhosted.org",
];
const GPU_ORT_DLLS: &[&str] = &[
    "onnxruntime.dll",
    "onnxruntime_providers_cuda.dll",
    "onnxruntime_providers_shared.dll",
];

pub fn gpu_ort_capi_dir(base: &Path) -> PathBuf {
    base.join("onnxruntime-gpu").join("onnxruntime").join("capi")
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

fn extract_gpu_ort_dlls(archive: &Path, dest: &Path) -> Result<(), String> {
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
fn preload_cuda_runtime() {
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
    let cudnn_root = cudnn_runtime_candidates()
        .into_iter()
        .find(|path| {
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
fn prepend_dll_search_paths(dirs: &[PathBuf]) {
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
fn dir_contains_prefix(dir: &Path, prefix: &str) -> bool {
    let prefix = prefix.to_ascii_lowercase();
    fs::read_dir(dir)
        .map(|entries| {
            entries
                .flatten()
                .any(|entry| entry.file_name().to_string_lossy().to_ascii_lowercase().starts_with(&prefix))
        })
        .unwrap_or(false)
}

/// 预载目录内全部 cudart64_*/cublas64_*/cublasLt64_*/cufft64_* DLL（任意主版本）。
fn preload_dir_cuda_variants(dir: &Path) {
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

fn cuda_runtime_candidates() -> Vec<PathBuf> {
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

fn cudnn_runtime_candidates() -> Vec<PathBuf> {
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

fn env_runtime_paths(name: &str) -> Vec<PathBuf> {
    std::env::var_os(name)
        .into_iter()
        .map(PathBuf::from)
        .collect()
}

fn local_ai_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    for drive in ['C', 'D', 'E', 'F', 'G'] {
        roots.push(PathBuf::from(format!(r"{drive}:\WebUI")));
        roots.push(PathBuf::from(format!(r"{drive}:\")));
    }
    roots
}

fn input_size(session: &Session) -> Result<(usize, usize), String> {
    let input = session
        .inputs()
        .first()
        .ok_or_else(|| "模型没有输入".to_string())?;
    let shape = outlet_tensor_shape(input)?;
    let height = shape
        .get(2)
        .copied()
        .filter(|value| *value > 0)
        .ok_or_else(|| "模型输入尺寸未知（动态 shape）".to_string())? as usize;
    let width = shape
        .get(3)
        .copied()
        .filter(|value| *value > 0)
        .ok_or_else(|| "模型输入尺寸未知（动态 shape）".to_string())? as usize;
    Ok((height, width))
}

fn outlet_tensor_shape(outlet: &ort::value::Outlet) -> Result<Vec<i64>, String> {
    match outlet.dtype() {
        ValueType::Tensor { shape, .. } => Ok(shape.to_vec()),
        other => Err(format!("模型输入类型不支持：{other:?}")),
    }
}

// ---------------------------------------------------------------------------
// Shared image helpers
// ---------------------------------------------------------------------------

/// PIL-style thumbnail: aspect-preserving downscale, never enlarges.
fn thumbnail_fit(w: u32, h: u32, max_w: u32, max_h: u32) -> (u32, u32) {
    let ratio = (max_w as f64 / w as f64)
        .min(max_h as f64 / h as f64)
        .min(1.0);
    let new_w = (w as f64 * ratio).round() as u32;
    let new_h = (h as f64 * ratio).round() as u32;
    (new_w.max(1), new_h.max(1))
}

fn bilinear_resize_luma(data: &[u8], width: u32, height: u32, new_w: u32, new_h: u32) -> Vec<u8> {
    let source = ImageBuffer::<Luma<u8>, Vec<u8>>::from_raw(width, height, data.to_vec())
        .expect("buffer size mismatch");
    image::imageops::resize(&source, new_w, new_h, FilterType::Triangle).into_raw()
}

fn to_f32(data: Vec<u8>) -> Vec<f32> {
    data.into_iter().map(|value| value as f32 / 255.0).collect()
}

fn probability_luma(probabilities: &[f32], threshold: f32) -> Vec<u8> {
    probabilities
        .iter()
        .map(|probability| if *probability > threshold { 255 } else { 0 })
        .collect()
}

fn refine_threshold() -> f32 {
    #[cfg(test)]
    {
        return std::env::var("AIAS_AB_REFINE_THRESHOLD")
            .ok()
            .and_then(|value| value.parse::<f32>().ok())
            .filter(|value| (0.0..1.0).contains(value))
            .unwrap_or(REFINE_THRESHOLD);
    }

    #[cfg(not(test))]
    {
        REFINE_THRESHOLD
    }
}

fn advanced_min_component_area(width: u32, height: u32) -> usize {
    let default = ((width as usize * height as usize) / 1_500).clamp(128, 2_048);
    #[cfg(test)]
    {
        return std::env::var("AIAS_AB_MIN_COMPONENT_AREA")
            .ok()
            .and_then(|value| value.parse::<usize>().ok())
            .filter(|value| *value >= 2)
            .unwrap_or(default);
    }

    #[cfg(not(test))]
    {
        default
    }
}

fn remove_small_foreground_components(
    matte: &mut [f32],
    width: u32,
    height: u32,
    min_area: usize,
) {
    let (width, height) = (width as usize, height as usize);
    if min_area <= 1 || matte.len() != width.saturating_mul(height) {
        return;
    }

    let mut visited = vec![false; matte.len()];
    let mut component = Vec::new();
    for start in 0..matte.len() {
        if visited[start] || matte[start] <= 0.5 {
            continue;
        }
        visited[start] = true;
        component.clear();
        component.push(start);
        let mut cursor = 0;
        while cursor < component.len() {
            let index = component[cursor];
            cursor += 1;
            let x = index % width;
            let y = index / width;
            for neighbor in [
                x.checked_sub(1).map(|value| y * width + value),
                (x + 1 < width).then_some(y * width + x + 1),
                y.checked_sub(1).map(|value| value * width + x),
                (y + 1 < height).then_some((y + 1) * width + x),
                x.checked_sub(1)
                    .zip(y.checked_sub(1))
                    .map(|(nx, ny)| ny * width + nx),
                (x + 1 < width)
                    .then_some(())
                    .zip(y.checked_sub(1))
                    .map(|(_, ny)| ny * width + x + 1),
                x.checked_sub(1)
                    .zip((y + 1 < height).then_some(y + 1))
                    .map(|(nx, ny)| ny * width + nx),
                (x + 1 < width)
                    .then_some(())
                    .zip((y + 1 < height).then_some(y + 1))
                    .map(|(_, ny)| ny * width + x + 1),
            ]
            .into_iter()
            .flatten()
            {
                if !visited[neighbor] && matte[neighbor] > 0.5 {
                    visited[neighbor] = true;
                    component.push(neighbor);
                }
            }
        }

        if component.len() < min_area {
            for index in &component {
                matte[*index] = 0.0;
            }
        }
    }
}

fn fill_small_background_holes(
    matte: &mut [f32],
    width: u32,
    height: u32,
    max_area: usize,
) {
    let (width, height) = (width as usize, height as usize);
    if max_area == 0 || matte.len() != width.saturating_mul(height) {
        return;
    }

    let mut visited = vec![false; matte.len()];
    let mut component = Vec::new();
    for start in 0..matte.len() {
        if visited[start] || matte[start] > 0.5 {
            continue;
        }
        visited[start] = true;
        component.clear();
        component.push(start);
        let mut cursor = 0;
        let mut touches_edge = false;
        while cursor < component.len() {
            let index = component[cursor];
            cursor += 1;
            let x = index % width;
            let y = index / width;
            touches_edge |= x == 0 || y == 0 || x + 1 == width || y + 1 == height;
            for neighbor in [
                x.checked_sub(1).map(|value| y * width + value),
                (x + 1 < width).then_some(y * width + x + 1),
                y.checked_sub(1).map(|value| value * width + x),
                (y + 1 < height).then_some((y + 1) * width + x),
                x.checked_sub(1)
                    .zip(y.checked_sub(1))
                    .map(|(nx, ny)| ny * width + nx),
                (x + 1 < width)
                    .then_some(())
                    .zip(y.checked_sub(1))
                    .map(|(_, ny)| ny * width + x + 1),
                x.checked_sub(1)
                    .zip((y + 1 < height).then_some(y + 1))
                    .map(|(nx, ny)| ny * width + nx),
                (x + 1 < width)
                    .then_some(())
                    .zip((y + 1 < height).then_some(y + 1))
                    .map(|(_, ny)| ny * width + x + 1),
            ]
            .into_iter()
            .flatten()
            {
                if !visited[neighbor] && matte[neighbor] <= 0.5 {
                    visited[neighbor] = true;
                    component.push(neighbor);
                }
            }
        }

        if !touches_edge && component.len() <= max_area {
            for index in &component {
                matte[*index] = 1.0;
            }
        }
    }
}

fn stable_sigmoid(value: f32) -> f32 {
    if value >= 0.0 {
        1.0 / (1.0 + (-value).exp())
    } else {
        let exp = value.exp();
        exp / (1.0 + exp)
    }
}

// ---------------------------------------------------------------------------
// Simple model (ISNet / isnetis.onnx) — port of simple_anime_seg.py
// ---------------------------------------------------------------------------

fn run_simple(base: &Path, rgb: &RgbImage) -> Result<Vec<f32>, String> {
    ensure_ort_runtime(base)?;
    prune_sessions(SessionKeep::Simple);
    let mut guard = simple_slot().lock().map_err(lock_error)?;
    if guard.is_none() {
        let path = models_dir(base).join("isnetis.onnx");
        if !path.exists() {
            return Err("标准模型未安装，请先在参数面板下载。".into());
        }
        *guard = Some(SimpleSessions {
            session: build_session(&path, true)?,
        });
    }
    let sessions = guard.as_mut().expect("session initialized");
    let session = &mut sessions.session;

    let (seg_h, seg_w) = input_size(session)?;
    let (w, h) = rgb.dimensions();
    let (sw, sh) = thumbnail_fit(w, h, seg_w as u32, seg_h as u32);
    let dx = (seg_w as u32 - sw) / 2;
    let dy = (seg_h as u32 - sh) / 2;

    // PIL thumbnail(): aspect-preserving downscale, never enlarges (BICUBIC).
    let thumb = if sw == w && sh == h {
        rgb.clone()
    } else {
        image::imageops::resize(rgb, sw, sh, FilterType::CatmullRom)
    };

    // BGR / 255, black center pad, NCHW.
    let mut input = vec![0_f32; 3 * seg_h * seg_w];
    for y in 0..sh {
        for x in 0..sw {
            let pixel = thumb.get_pixel(x, y);
            let dst = ((y + dy) as usize) * seg_w + (x + dx) as usize;
            input[dst] = pixel[2] as f32 / 255.0;
            input[seg_w * seg_h + dst] = pixel[1] as f32 / 255.0;
            input[2 * seg_w * seg_h + dst] = pixel[0] as f32 / 255.0;
        }
    }

    let input_name = session.inputs()[0].name().to_string();
    let output_name = session.outputs()[0].name().to_string();
    let tensor =
        Tensor::from_array((vec![1_usize, 3, seg_h, seg_w], input)).map_err(to_string_error)?;
    let outputs = session
        .run(ort::inputs![input_name.as_str() => tensor])
        .map_err(to_string_error)?;
    let (shape, mask) = outputs[output_name.as_str()]
        .try_extract_tensor::<f32>()
        .map_err(to_string_error)?;
    let mask_h = (*shape.get(2).ok_or("模型输出 shape 无效")?) as usize;
    let mask_w = (*shape.get(3).ok_or("模型输出 shape 无效")?) as usize;
    let _ = mask_h;

    // Crop the pasted region back out of the padded mask.
    let cropped_w = sw as usize;
    let cropped_h = sh as usize;
    let mut cropped = vec![0_f32; cropped_w * cropped_h];
    for y in 0..cropped_h {
        let src_row = (y + dy as usize) * mask_w;
        let dst_row = y * cropped_w;
        let lo = (dx as usize).min(mask_w);
        let hi = (dx as usize + cropped_w).min(mask_w);
        if hi > lo {
            cropped[dst_row..dst_row + (hi - lo)]
                .copy_from_slice(&mask[src_row + lo..src_row + hi]);
        }
    }

    let resized = if cropped_w == w as usize && cropped_h == h as usize {
        cropped
    } else {
        let source = ImageBuffer::<Luma<f32>, Vec<f32>>::from_raw(
            cropped_w as u32,
            cropped_h as u32,
            cropped,
        )
        .ok_or("掩码缓冲无效")?;
        image::imageops::resize(&source, w, h, FilterType::Triangle).into_raw()
    };

    Ok(resized
        .into_iter()
        .map(|value| value.clamp(0.0, 1.0))
        .collect())
}

// ---------------------------------------------------------------------------
// Advanced model (RTMDet + ISNetDis refiner) — port of advanced_anime_seg.py
// ---------------------------------------------------------------------------

const STRIDES: [u32; 3] = [8, 16, 32];
const DETECTION_THRESHOLD: f32 = 0.3;
const REFINE_THRESHOLD: f32 = 0.3;
const MEAN: [f32; 3] = [123.675, 116.28, 103.53];
const STD: [f32; 3] = [58.395, 57.12, 57.375];

fn resize_pad_rgb(img: &RgbImage, size: u32) -> (RgbImage, (u32, u32, u32, u32)) {
    let (w, h) = img.dimensions();
    let scale = size as f64 / w.max(h) as f64;
    let new_w = ((w as f64 * scale) as u32).clamp(1, size);
    let new_h = ((h as f64 * scale) as u32).clamp(1, size);
    let resized = image::imageops::resize(img, new_w, new_h, FilterType::Triangle);
    let pad_l = (size - new_w) / 2;
    let pad_t = (size - new_h) / 2;
    let mut canvas = RgbImage::new(size, size);
    for y in 0..new_h {
        for x in 0..new_w {
            canvas.put_pixel(pad_l + x, pad_t + y, *resized.get_pixel(x, y));
        }
    }
    let pads = (pad_t, size - new_h - pad_t, pad_l, size - new_w - pad_l);
    (canvas, pads)
}

fn run_advanced(base: &Path, rgb: &RgbImage) -> Result<Vec<f32>, String> {
    ensure_ort_runtime(base)?;
    prune_sessions(SessionKeep::Advanced);
    let mut guard = advanced_slot().lock().map_err(lock_error)?;
    if guard.is_none() {
        let seg_path = models_dir(base).join("anime_segmentor_rtmdet_e60_simplified.onnx");
        let refine_path =
            models_dir(base).join("mask_refiner_isnetdis_refine_last_simplified.onnx");
        if !seg_path.exists() || !refine_path.exists() {
            return Err("精细模型未安装，请先在参数面板下载。".into());
        }
        *guard = Some(AdvancedSessions {
            seg: build_session(&seg_path, true)?,
            refine: build_session(&refine_path, true)?,
        });
    }
    let sessions = guard.as_mut().expect("sessions initialized");
    let (w, h) = rgb.dimensions();

    // -- Stage 1: detect the character and build the coarse mask ------------
    let (seg_h, seg_w) = input_size(&sessions.seg)?;
    let (sw, sh) = thumbnail_fit(w, h, seg_w as u32, seg_h as u32);
    let pad_w = ((seg_w as u32 - sw) / 2) as usize;
    let pad_h = ((seg_h as u32 - sh) / 2) as usize;

    // PIL thumbnail with LANCZOS, then gray-114 center pad.
    let thumb = if sw == w && sh == h {
        rgb.clone()
    } else {
        image::imageops::resize(rgb, sw, sh, FilterType::Lanczos3)
    };

    // PIL pads the canvas with raw gray 114 BEFORE normalizing, so the pad
    // planes must start at (114 - mean) / std, not 0.
    let pad_value = [
        (114.0 - MEAN[0]) / STD[0],
        (114.0 - MEAN[1]) / STD[1],
        (114.0 - MEAN[2]) / STD[2],
    ];
    let plane = seg_h * seg_w;
    let mut input = Vec::with_capacity(3 * plane);
    for pad in pad_value {
        input.extend(std::iter::repeat_n(pad, plane));
    }
    for y in 0..sh {
        for x in 0..sw {
            let pixel = thumb.get_pixel(x, y);
            let dst = (y as usize + pad_h) * seg_w + (x as usize + pad_w);
            // arr[::-1] maps B->mean[0], G->mean[1], R->mean[2].
            input[dst] = (pixel[2] as f32 - MEAN[0]) / STD[0];
            input[plane + dst] = (pixel[1] as f32 - MEAN[1]) / STD[1];
            input[2 * plane + dst] = (pixel[0] as f32 - MEAN[2]) / STD[2];
        }
    }

    let seg_input_name = sessions.seg.inputs()[0].name().to_string();
    let tensor =
        Tensor::from_array((vec![1_usize, 3, seg_h, seg_w], input)).map_err(to_string_error)?;
    let outputs = sessions
        .seg
        .run(ort::inputs![seg_input_name.as_str() => tensor])
        .map_err(to_string_error)?;

    // Pick the single highest-scoring anchor across strides.
    let mut best_score = f32::NEG_INFINITY;
    let mut best_stride = 0_u32;
    let mut best_row = 0_usize;
    let mut best_col = 0_usize;
    for stride in STRIDES {
        let name = format!("scores.stride{stride}");
        let (shape, logits) = outputs[name.as_str()]
            .try_extract_tensor::<f32>()
            .map_err(to_string_error)?;
        let grid_h = (*shape.get(2).ok_or("scores shape 无效")?) as usize;
        let grid_w = (*shape.get(3).ok_or("scores shape 无效")?) as usize;
        for (index, &logit) in logits.iter().enumerate().take(grid_h * grid_w) {
            let prob = stable_sigmoid(logit);
            if prob > best_score {
                best_score = prob;
                best_stride = stride;
                best_row = index / grid_w;
                best_col = index % grid_w;
            }
        }
    }

    // Decode the box around the best anchor (seg-canvas coordinates).
    // bboxes layout is channels-first: [1, 4, grid_h, grid_w].
    let bbox_name = format!("bboxes.stride{}", best_stride);
    let (bbox_shape, bboxes) = outputs[bbox_name.as_str()]
        .try_extract_tensor::<f32>()
        .map_err(to_string_error)?;
    let bbox_grid_h = (*bbox_shape.get(2).ok_or("bboxes shape 无效")?) as usize;
    let bbox_grid_w = (*bbox_shape.get(3).ok_or("bboxes shape 无效")?) as usize;
    let bbox_plane = bbox_grid_h * bbox_grid_w;
    let bbox_cell = best_row * bbox_grid_w + best_col;
    let x_anchor = best_col as f32 * best_stride as f32;
    let y_anchor = best_row as f32 * best_stride as f32;
    let bx1 = x_anchor - bboxes[bbox_cell];
    let by1 = y_anchor - bboxes[bbox_plane + bbox_cell];
    let bx2 = x_anchor + bboxes[2 * bbox_plane + bbox_cell];
    let by2 = y_anchor + bboxes[3 * bbox_plane + bbox_cell];

    // Rescale the box into original image coordinates.
    let eff_w = (seg_w - 2 * pad_w) as f32;
    let eff_h = (seg_h - 2 * pad_h) as f32;
    let x1 = ((bx1 - pad_w as f32) * w as f32 / eff_w).max(0.0);
    let x2 = ((bx2 - pad_w as f32) * w as f32 / eff_w).min(w as f32);
    let y1 = ((by1 - pad_h as f32) * h as f32 / eff_h).max(0.0);
    let y2 = ((by2 - pad_h as f32) * h as f32 / eff_h).min(h as f32);
    let center_x = (x1 + x2) * 0.5;
    let center_y = (y1 + y2) * 0.5;

    // Build the mask prototype features and run the dynamic 1x1-conv head.
    let (proto_shape, proto) = outputs["mask_proto"]
        .try_extract_tensor::<f32>()
        .map_err(to_string_error)?;
    let proto_c = (*proto_shape.get(1).ok_or("proto shape 无效")?) as usize;
    let proto_h = (*proto_shape.get(2).ok_or("proto shape 无效")?) as usize;
    let proto_w = (*proto_shape.get(3).ok_or("proto shape 无效")?) as usize;

    let coeff_name = format!("coeffs.stride{}", best_stride);
    let (coeff_shape, coeffs) = outputs[coeff_name.as_str()]
        .try_extract_tensor::<f32>()
        .map_err(to_string_error)?;
    let coeff_grid_h = (*coeff_shape.get(2).ok_or("coeffs shape 无效")?) as usize;
    let coeff_grid_w = (*coeff_shape.get(3).ok_or("coeffs shape 无效")?) as usize;
    // coeffs layout is channels-first: [1, kernel_len, grid_h, grid_w].
    let coeff_plane = coeff_grid_h * coeff_grid_w;
    let coeff_cell = best_row * coeff_grid_w + best_col;

    let in_ch = proto_c + 2;
    let mut mask_feat = vec![0_f32; in_ch * proto_h * proto_w];
    let center_x_proto = center_x / best_stride as f32;
    let center_y_proto = center_y / best_stride as f32;
    let rel_scale = best_stride as f32 * 8.0;
    for y in 0..proto_h {
        for x in 0..proto_w {
            let dst = y * proto_w + x;
            mask_feat[dst] = (center_x_proto - x as f32) / rel_scale;
            mask_feat[proto_h * proto_w + dst] = (center_y_proto - y as f32) / rel_scale;
            let proto_base = dst;
            for c in 0..proto_c {
                let channel_offset = (2 + c) * proto_h * proto_w;
                mask_feat[channel_offset + dst] = proto[proto_base + c * proto_h * proto_w];
            }
        }
    }

    // Split the kernel into three 1x1 conv layers (inter = 8, then 8, then 1).
    let inter = 8_usize;
    let w1_len = inter * in_ch;
    let w2_len = inter * inter;
    let w3_len = inter;
    let b1_off = w1_len + w2_len + w3_len;
    let b2_off = b1_off + inter;
    let b3_off = b2_off + inter;
    let kernel_len = b3_off + 1;
    let kernel: Vec<f32> = (0..kernel_len)
        .map(|k| coeffs[k * coeff_plane + coeff_cell])
        .collect();

    let pixels = proto_h * proto_w;
    let mut layer1 = vec![0_f32; inter * pixels];
    for pixel in 0..pixels {
        for out_c in 0..inter {
            let mut acc = kernel[b1_off + out_c];
            for in_c in 0..in_ch {
                acc += kernel[out_c * in_ch + in_c] * mask_feat[in_c * pixels + pixel];
            }
            layer1[out_c * pixels + pixel] = acc.max(0.0);
        }
    }
    let mut layer2 = vec![0_f32; inter * pixels];
    for pixel in 0..pixels {
        for out_c in 0..inter {
            let mut acc = kernel[b2_off + out_c];
            for in_c in 0..inter {
                acc += kernel[w1_len + out_c * inter + in_c] * layer1[in_c * pixels + pixel];
            }
            layer2[out_c * pixels + pixel] = acc.max(0.0);
        }
    }
    let mut logits_small = vec![0_f32; pixels];
    for pixel in 0..pixels {
        let mut acc = kernel[b3_off];
        for in_c in 0..inter {
            acc += kernel[w1_len + w2_len + in_c] * layer2[in_c * pixels + pixel];
        }
        logits_small[pixel] = stable_sigmoid(acc);
    }

    // Threshold, unpad, and scale the coarse mask back to the original size.
    let small_bin: Vec<u8> = logits_small
        .iter()
        .map(|prob| if *prob > DETECTION_THRESHOLD { 255 } else { 0 })
        .collect();
    let upscaled = bilinear_resize_luma(
        &small_bin,
        proto_w as u32,
        proto_h as u32,
        seg_w as u32,
        seg_h as u32,
    );
    let cropped_w = sw as usize;
    let cropped_h = sh as usize;
    let mut cropped = vec![0_u8; cropped_w * cropped_h];
    for y in 0..cropped_h {
        let src_row = (y + pad_h) * seg_w;
        let dst_row = y * cropped_w;
        let lo = pad_w.min(seg_w);
        let hi = (pad_w + cropped_w).min(seg_w);
        if hi > lo {
            cropped[dst_row..dst_row + (hi - lo)]
                .copy_from_slice(&upscaled[src_row + lo..src_row + hi]);
        }
    }
    let coarse_f32 = to_f32(bilinear_resize_luma(
        &cropped,
        cropped_w as u32,
        cropped_h as u32,
        w,
        h,
    ));

    // -- Stage 2: refine the coarse mask ------------------------------------
    let refine_size = outlet_tensor_shape(
        sessions.refine.inputs().first().ok_or("精修模型没有输入")?,
    )
    .and_then(|dims| {
        dims.get(2)
            .copied()
            .filter(|value| *value > 0)
            .ok_or_else(|| "精修模型输入尺寸未知".to_string())
    })? as u32;

    let (img_pad, (pt, pb, pl, pr)) = resize_pad_rgb(rgb, refine_size);
    // Planar CHW, not pixel-interleaved.
    let plane_i = (refine_size * refine_size) as usize;
    let mut img_data = vec![0_f32; 3 * plane_i];
    for y in 0..refine_size {
        for x in 0..refine_size {
            let pixel = img_pad.get_pixel(x, y);
            let dst = y as usize * refine_size as usize + x as usize;
            img_data[dst] = pixel[0] as f32 / 255.0;
            img_data[plane_i + dst] = pixel[1] as f32 / 255.0;
            img_data[2 * plane_i + dst] = pixel[2] as f32 / 255.0;
        }
    }
    // The coarse mask must go through the SAME resize_pad geometry as the
    // image: aspect-fit plus centered zero pad, never a square stretch.
    let coarse_u8: Vec<u8> = coarse_f32
        .iter()
        .map(|value| (value * 255.0).round() as u8)
        .collect();
    let mask_scale = refine_size as f64 / w.max(h) as f64;
    let mask_w = ((w as f64 * mask_scale) as u32).clamp(1, refine_size);
    let mask_h = ((h as f64 * mask_scale) as u32).clamp(1, refine_size);
    let mask_small = bilinear_resize_luma(&coarse_u8, w, h, mask_w, mask_h);
    let mask_pl = (refine_size - mask_w) / 2;
    let mask_pt = (refine_size - mask_h) / 2;
    let mut seg_pad = vec![0_u8; (refine_size * refine_size) as usize];
    for y in 0..mask_h {
        for x in 0..mask_w {
            seg_pad[((y + mask_pt) * refine_size + (x + mask_pl)) as usize] =
                mask_small[(y * mask_w + x) as usize];
        }
    }
    let seg_data: Vec<f32> = to_f32(seg_pad);

    let mut refine_in = vec![0_f32; 4 * plane_i];
    for (index, value) in img_data.into_iter().enumerate() {
        refine_in[index] = value;
    }
    for (index, value) in seg_data.into_iter().enumerate() {
        refine_in[3 * plane_i + index] = value;
    }

    let refine_input_name = sessions.refine.inputs()[0].name().to_string();
    let refine_output_name = sessions.refine.outputs()[0].name().to_string();
    let tensor = Tensor::from_array((
        vec![1_usize, 4, refine_size as usize, refine_size as usize],
        refine_in,
    ))
    .map_err(to_string_error)?;
    let outputs = sessions
        .refine
        .run(ort::inputs![refine_input_name.as_str() => tensor])
        .map_err(to_string_error)?;
    let (shape, logits) = outputs[refine_output_name.as_str()]
        .try_extract_tensor::<f32>()
        .map_err(to_string_error)?;
    let logits_h = (*shape.get(2).ok_or("精修输出 shape 无效")?) as usize;
    let logits_w = (*shape.get(3).ok_or("精修输出 shape 无效")?) as usize;

    let mut refined = vec![0_u8; (w * h) as usize];
    let crop_x0 = pl as usize;
    let crop_y0 = pt as usize;
    let crop_w = logits_w
        .saturating_sub(pl as usize)
        .saturating_sub(pr as usize);
    let crop_h = logits_h
        .saturating_sub(pt as usize)
        .saturating_sub(pb as usize);
    let mut cropped = vec![0_f32; crop_w * crop_h];
    for y in 0..crop_h {
        for x in 0..crop_w {
            let value = logits[(y + crop_y0) * logits_w + (x + crop_x0)];
            let value = value.clamp(-50.0, 50.0);
            cropped[y * crop_w + x] = stable_sigmoid(value);
        }
    }
    let prob_u8 = probability_luma(&cropped, refine_threshold());
    let mask = bilinear_resize_luma(&prob_u8, crop_w as u32, crop_h as u32, w, h);
    for (index, value) in mask.into_iter().enumerate() {
        refined[index] = value;
    }
    let mut mask = to_f32(refined);
    let component_area = advanced_min_component_area(w, h);
    fill_small_background_holes(&mut mask, w, h, component_area);
    remove_small_foreground_components(&mut mask, w, h, component_area);
    Ok(mask)
}

// ---------------------------------------------------------------------------
// BiRefNet family (ToonOut + official general/portrait/HR/lite)
// ---------------------------------------------------------------------------

const BIREFNET_MEAN: [f32; 3] = [0.485, 0.456, 0.406];
const BIREFNET_STD: [f32; 3] = [0.229, 0.224, 0.225];

/// BiRefNet 发布推理的图像预处理：整图直接缩放到模型固定输入。
/// 不进行等比留边，否则竖图会浪费掉大部分有效分割面积。
fn resize_birefnet_input(rgb: &RgbImage, target_w: u32, target_h: u32) -> RgbImage {
    if rgb.dimensions() == (target_w, target_h) {
        rgb.clone()
    } else {
        image::imageops::resize(rgb, target_w, target_h, FilterType::CatmullRom)
    }
}

/// BiRefNet 系共享推理管线入口：ToonOut（动漫微调）与官方 general/portrait/HR/lite
/// 预处理完全一致，仅输入分辨率随模型不同（1024/2048，从会话动态读取）。
/// `matting` 为 true 表示模型输出连续 alpha（人像类），后处理不做对比度拉伸。
///
/// 官方 fp32 导出在 1024² 下显存占用约为 fp16 的两倍（整图 ASPP 中间张量单笔
/// 约 784MB），桌面应用占用较多显存的卡上 GPU 推理会 OOM：此时释放 GPU 会话、
/// 改用 CPU 重建并重试一次，保证出图（慢但可用）。
fn run_birefnet(base: &Path, id: &str, matting: bool, rgb: &RgbImage) -> Result<Vec<f32>, String> {
    match try_run_birefnet(base, id, matting, rgb, true) {
        Ok(mask) => Ok(mask),
        Err(error) if is_gpu_oom_error(&error) => {
            release_birefnet_session(id);
            try_run_birefnet(base, id, matting, rgb, false)
        }
        Err(error) => Err(error),
    }
}

/// 显存/内存不足的报错来自 ONNX Runtime 的 CUDA arena、cudaMalloc 或 host 端
/// std::bad_alloc（部分算子如 GatherND 无 CUDA 内核，会落在内存执行）；关键词保持宽松。
fn is_gpu_oom_error(error: &str) -> bool {
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

fn try_run_birefnet(
    base: &Path,
    id: &str,
    matting: bool,
    rgb: &RgbImage,
    use_gpu: bool,
) -> Result<Vec<f32>, String> {
    let spec = model_spec(id)?;
    let file_name = spec
        .files
        .split_first()
        .map(|(file, _)| file.name)
        .ok_or("模型注册缺少文件")?;
    ensure_ort_runtime(base)?;
    prune_sessions(SessionKeep::Birefnet(id));
    let mut sessions = birefnet_sessions().lock().map_err(lock_error)?;
    if !sessions.iter().any(|(model_id, _)| model_id == id) {
        let path = models_dir(base).join(file_name);
        if !path.exists() {
            return Err(format!("{}模型未安装，请先在参数面板下载。", spec.label));
        }
        sessions.push((id.to_string(), build_session(&path, use_gpu)?));
    }
    let session = sessions
        .iter_mut()
        .find(|(model_id, _)| model_id == id)
        .map(|(_, session)| session)
        .expect("session just ensured");
    let (w, h) = rgb.dimensions();

    // Exports fix the input square; fall back if a rebuild is dynamic.
    let (seg_h, seg_w) = input_size(session).unwrap_or((1024, 1024));

    // 发布推理流程是直接缩放到固定方形输入；不添加训练流程中
    // 不存在的灰色留边，也不在输出阶段裁剪。这样竖图会获得完整的
    // 有效分割面积，而不是把主体压进窄条内容区。
    let model_input = resize_birefnet_input(rgb, seg_w as u32, seg_h as u32);
    let plane = seg_h * seg_w;
    let mut input = vec![0_f32; 3 * plane];
    for y in 0..seg_h as u32 {
        for x in 0..seg_w as u32 {
            let pixel = model_input.get_pixel(x, y);
            let dst = y as usize * seg_w + x as usize;
            input[dst] = (pixel[0] as f32 / 255.0 - BIREFNET_MEAN[0]) / BIREFNET_STD[0];
            input[plane + dst] = (pixel[1] as f32 / 255.0 - BIREFNET_MEAN[1]) / BIREFNET_STD[1];
            input[2 * plane + dst] = (pixel[2] as f32 / 255.0 - BIREFNET_MEAN[2]) / BIREFNET_STD[2];
        }
    }

    let input_name = session.inputs()[0].name().to_string();
    let output_name = session.outputs()[0].name().to_string();
    let tensor =
        Tensor::from_array((vec![1_usize, 3, seg_h, seg_w], input)).map_err(to_string_error)?;
    let outputs = session
        .run(ort::inputs![input_name.as_str() => tensor])
        .map_err(to_string_error)?;
    let (shape, mask) = outputs[output_name.as_str()]
        .try_extract_tensor::<f32>()
        .map_err(to_string_error)?;
    let mask_h = (*shape.get(2).ok_or("模型输出 shape 无效")?) as u32;
    let mask_w = (*shape.get(3).ok_or("模型输出 shape 无效")?) as u32;
    let mask_len = mask_h as usize * mask_w as usize;

    // The export already applies sigmoid; the range guard just keeps an
    // un-sigmoided rebuild from producing a fully transparent result.
    let needs_sigmoid = mask[..mask_len]
        .iter()
        .any(|value| *value < -0.01 || *value > 1.01);

    // 官方 BiRefNet 导出的 sigmoid 输出整体置信度偏低（相对 ToonOut 明显平坦），
    // 直接用固定阈值拉伸会得到“全前景”的半透明掩码；rembg 对官方模型的处理
    // 就是先做 min-max 归一化。ToonOut 已在线上充分验证，保持原后处理不动。
    let mut native: Vec<f32> = mask[..mask_len].to_vec();
    if needs_sigmoid {
        native = native.into_iter().map(stable_sigmoid).collect();
    }
    if matches!(spec.kind, ModelKind::BiRefNet { .. }) {
        let mi = native.iter().cloned().fold(f32::INFINITY, f32::min);
        let ma = native.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
        if ma - mi > 1e-3 {
            for value in &mut native {
                *value = (*value - mi) / (ma - mi);
            }
        }
    }

    // 与预处理相反，直接把模型的方形输出缩回原图大小；不裁切任何有效区域。
    let upscaled_mask = if mask_w == w && mask_h == h {
        native
    } else {
        let source =
            ImageBuffer::<Luma<f32>, Vec<f32>>::from_raw(mask_w, mask_h, native)
                .ok_or("掩码缓冲无效")?;
        image::imageops::resize(&source, w, h, FilterType::Lanczos3).into_raw()
    };
    if matting {
        // 人像/matting 导出的是连续 alpha：保留原值，仅平滑过渡带。
        // 对比度拉伸会把发丝等半透明细节压向全透/全不透明，毁掉 matting 的优势。
        Ok(smooth_matte_edges(&upscaled_mask, w, h))
    } else {
        // 分割类输出：柔化对比度，只把接近全透明/全不透明的两头压向极值，
        // 保留发丝、水花等半透明细节的中间响应，不做硬二值化（那会毁掉这些细节）。
        const MATTE_FLOOR: f32 = 0.05;
        const MATTE_CEIL: f32 = 0.95;
        let mut result = upscaled_mask;
        for value in &mut result {
            *value = ((value.clamp(0.0, 1.0) - MATTE_FLOOR) / (MATTE_CEIL - MATTE_FLOOR))
                .clamp(0.0, 1.0);
        }
        Ok(smooth_matte_edges(&result, w, h))
    }
}

// matte 上采样后，半透明过渡带会出现锯齿和孤立噪点；只对过渡带（含 1px
// 膨胀）做 3x3 高斯平滑，实心和透明区域保持原样，避免啃掉细发丝。
/// 半透明「幽灵残留」抑制：分割模型对复杂背景的低置信度响应会在发丝
/// 间隙、角色两侧留下大片半透明背景碎屑，浅色预览看不出来，换底后是
/// 一片幽灵色块，是抠图观感差的主因。策略：以实心主体（alpha ≥ SOLID）
/// 为源做城市块距离变换，非实心像素随距离渐进衰减——紧贴实心边缘的
/// 发丝/水花细节几乎不受影响，远离主体的背景碎屑平滑归零。孤立但实心
/// 的前景（如脱手的饰品）不受影响。
fn suppress_background_ghosts(mask: &mut [f32], w: u32, h: u32) {
    let (w, h) = (w as usize, h as usize);
    if w < 3 || h < 3 {
        return;
    }
    const SOLID: f32 = 0.85;
    const INF: u32 = u32::MAX;
    // 半径只覆盖贴边抗锯齿（1-3px）；4px 起衰减、16px 处归零。实测发丝
    // 间隙里距实心边缘 10px 以上的半透明笔触全部落进衰减区被压掉，
    // 同时保留了发丝边缘的抗锯齿过渡。
    let radius = (w.min(h) / 400).clamp(3, 12) as u32;
    let fade = radius * 4;
    let mut dist = vec![INF; w * h];
    for y in 0..h {
        for x in 0..w {
            let index = y * w + x;
            if mask[index] >= SOLID {
                dist[index] = 0;
                continue;
            }
            let mut value = INF;
            if x > 0 {
                value = value.min(dist[index - 1].saturating_add(1));
            }
            if y > 0 {
                value = value.min(dist[index - w].saturating_add(1));
            }
            dist[index] = value;
        }
    }
    for y in (0..h).rev() {
        for x in (0..w).rev() {
            let index = y * w + x;
            if dist[index] == 0 {
                continue;
            }
            let mut value = dist[index];
            if x + 1 < w {
                value = value.min(dist[index + 1].saturating_add(1));
            }
            if y + 1 < h {
                value = value.min(dist[index + w].saturating_add(1));
            }
            dist[index] = value.min(fade);
        }
    }
    for (index, value) in mask.iter_mut().enumerate() {
        if dist[index] == 0 || dist[index] == INF {
            continue;
        }
        if dist[index] >= fade {
            *value = 0.0;
        } else if dist[index] > radius {
            let keep = (fade - dist[index]) as f32 / (fade - radius) as f32;
            *value *= keep;
        }
    }
}

fn smooth_matte_edges(mask: &[f32], w: u32, h: u32) -> Vec<f32> {
    let (w, h) = (w as usize, h as usize);
    if w < 3 || h < 3 {
        return mask.to_vec();
    }
    let soft = |value: f32| value > 0.02 && value < 0.98;
    let mut band = vec![false; w * h];
    for y in 0..h {
        for x in 0..w {
            if !soft(mask[y * w + x]) {
                continue;
            }
            let x0 = x.saturating_sub(1);
            let y0 = y.saturating_sub(1);
            let x1 = (x + 1).min(w - 1);
            let y1 = (y + 1).min(h - 1);
            for yy in y0..=y1 {
                for xx in x0..=x1 {
                    band[yy * w + xx] = true;
                }
            }
        }
    }
    const KERNEL: [f32; 9] = [1.0, 2.0, 1.0, 2.0, 4.0, 2.0, 1.0, 2.0, 1.0];
    mask.iter()
        .enumerate()
        .map(|(index, value)| {
            if !band[index] {
                return *value;
            }
            let (x, y) = (index % w, index / w);
            let mut sum = 0.0;
            let mut weights = 0.0;
            for dy in -1i64..=1 {
                for dx in -1i64..=1 {
                    let xx = (x as i64 + dx).clamp(0, w as i64 - 1) as usize;
                    let yy = (y as i64 + dy).clamp(0, h as i64 - 1) as usize;
                    let k = KERNEL[((dy + 1) * 3 + (dx + 1)) as usize];
                    sum += mask[yy * w + xx] * k;
                    weights += k;
                }
            }
            sum / weights
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Matte 后处理：背景残留抑制、引导滤波、边缘去污染
// ---------------------------------------------------------------------------

/// 边缘去污染（defringe）：过渡带的颜色被旧背景混入，换背景后边缘发灰发粉。
/// 先用大半径加权估计局部背景色 B = Σ(C·(1-a)) / Σ(1-a)，
/// 再对 a < DECONTAM_CEIL 的像素解混 F = (C - (1-a)·B) / max(a, ε)。
fn decontaminate_colors(rgb: &RgbImage, matte: &[f32]) -> Vec<[u8; 3]> {
    const RADIUS: usize = 16;
    // CEIL 拉到 0.95：细发丝的「实心」像素只有 2-6px 宽，颜色同样被旧背景
    // 污染（换底后边缘发粉）。a=0.9 时解混修正量只有 ~10%，把近实心像素
    // 也纳入解混收益明显、风险很小；合法粉色主体（发饰）周围背景占比低，
    // 由 BG_PRESENCE_MIN 守卫。
    const CEIL: f32 = 0.95;
    const FLOOR_A: f32 = 0.15;
    const BG_PRESENCE_MIN: f64 = 0.05;

    let (w, h) = rgb.dimensions();
    let (w, h) = (w as usize, h as usize);
    let total = w * h;
    let mut out = Vec::with_capacity(total);
    if total == 0 || matte.len() != total {
        for pixel in rgb.pixels() {
            out.push([pixel[0], pixel[1], pixel[2]]);
        }
        return out;
    }
    let weight: Vec<f64> = matte.iter().map(|a| (1.0 - *a) as f64).collect();
    let mut weighted = [Vec::with_capacity(total), Vec::with_capacity(total), Vec::with_capacity(total)];
    for (index, pixel) in rgb.pixels().enumerate() {
        let wgt = weight[index];
        for ch in 0..3 {
            weighted[ch].push(pixel[ch] as f64 * wgt);
        }
    }
    let mean_w = box_mean_f64(&weight, w, h, RADIUS);
    let mean_c = [
        box_mean_f64(&weighted[0], w, h, RADIUS),
        box_mean_f64(&weighted[1], w, h, RADIUS),
        box_mean_f64(&weighted[2], w, h, RADIUS),
    ];
    for (index, pixel) in rgb.pixels().enumerate() {
        let a = matte[index];
        let wsum = mean_w[index];
        if a >= CEIL || wsum < BG_PRESENCE_MIN {
            out.push([pixel[0], pixel[1], pixel[2]]);
            continue;
        }
        let aa = (a as f64).max(FLOOR_A as f64);
        let mut color = [0u8; 3];
        for ch in 0..3 {
            let bg = mean_c[ch][index] / wsum;
            let foreground = (pixel[ch] as f64 - (1.0 - a) as f64 * bg) / aa;
            color[ch] = foreground.round().clamp(0.0, 255.0) as u8;
        }
        out.push(color);
    }
    out
}

/// O(n) 积分图盒均值。
fn box_mean_f64(values: &[f64], w: usize, h: usize, radius: usize) -> Vec<f64> {
    let stride = w + 1;
    let mut sat = vec![0f64; stride * (h + 1)];
    for y in 0..h {
        let mut row_sum = 0f64;
        for x in 0..w {
            row_sum += values[y * w + x];
            sat[(y + 1) * stride + (x + 1)] = sat[y * stride + (x + 1)] + row_sum;
        }
    }
    let mut out = vec![0f64; w * h];
    for y in 0..h {
        let y0 = y.saturating_sub(radius);
        let y1 = (y + radius + 1).min(h);
        for x in 0..w {
            let x0 = x.saturating_sub(radius);
            let x1 = (x + radius + 1).min(w);
            let area = ((y1 - y0) * (x1 - x0)) as f64;
            let sum = sat[y1 * stride + x1] - sat[y0 * stride + x1] - sat[y1 * stride + x0]
                + sat[y0 * stride + x0];
            out[y * w + x] = sum / area;
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Public entry: cut a single image
// ---------------------------------------------------------------------------

/// 一次抠图的结果：实际生效的模型、以及是否发生了兜底回退。
pub struct CutoutOutcome {
    pub model_used: String,
    pub fallback: bool, // 是否从 toonout 回退到其他模型
}

pub fn cutout(base: &Path, model_id: &str, input: &Path, output: &Path) -> Result<(), String> {
    cutout_with_fallback(base, model_id, input, output).map(|_| ())
}

/// 与 `cutout` 相同的流程，但 ToonOut 在复杂背景上「整图判前景」时会
/// 优先由高级模型复核，只有复核结果确实更干净时才切换输出。
pub fn cutout_with_fallback(
    base: &Path,
    model_id: &str,
    input: &Path,
    output: &Path,
) -> Result<CutoutOutcome, String> {
    let mut image = image::open(input).map_err(to_string_error)?;
    if let Some(orientation) = exif_orientation(input)? {
        image.apply_orientation(orientation);
    }
    let rgb = image.to_rgb8();
    let (w, h) = rgb.dimensions();

    let (mut mask, fallback_model) = match model_spec(model_id)?.kind {
        ModelKind::Simple => (run_simple(base, &rgb)?, model_id),
        ModelKind::Advanced => (run_advanced(base, &rgb)?, model_id),
        ModelKind::BiRefNet { matting } => {
            (run_birefnet(base, model_id, matting, &rgb)?, model_id)
        }
        ModelKind::Toonout => {
            let mask = run_birefnet(base, "toonout", false, &rgb)?;
            if toonout_likely_failed(&mask, w, h) {
                // 先释放 ToonOut 会话，避免两套大模型在显存中重叠；再用高级模型
                // 复核。复杂插画背景上它通常更能清掉被 ToonOut 保留的线稿。
                release_birefnet_session("toonout");
                if is_model_ready(base, "advanced") {
                    match run_advanced(base, &rgb) {
                        Ok(candidate) if matte_is_substantially_cleaner(&candidate, &mask, w, h) => {
                            (candidate, "advanced")
                        }
                        _ => (mask, "toonout"),
                    }
                } else if is_model_ready(base, "simple") {
                    (run_simple(base, &rgb)?, "simple")
                } else {
                    (mask, "toonout")
                }
            } else {
                (mask, "toonout")
            }
        }
    };

    let fallback = model_id == "toonout" && fallback_model != "toonout";

    // 幽灵残留抑制先于去污染：碎屑清除后，过渡带背景色估计更准。
    suppress_background_ghosts(&mut mask, w, h);
    // 再清一轮孤岛：残留中与主体不连通的小碎块（线稿笔触、噪点）整块移除，
    // 与 advanced 管线共用同一面积尺度。
    remove_small_foreground_components(&mut mask, w, h, advanced_min_component_area(w, h));

    // 边缘去污染：把过渡带颜色从「前景+背景混合」解混回纯前景色。
    // 实测对发丝、皮肤边缘的粉色/蓝色 fringe 有明显改善，且深/浅背景图都稳健。
    let colors = decontaminate_colors(&rgb, &mask);

    let mut result = RgbaImage::new(w, h);
    for y in 0..h {
        for x in 0..w {
            let index = (y * w + x) as usize;
            let alpha = (mask[index] * 255.0).round().clamp(0.0, 255.0) as u8;
            let color = colors[index];
            result.put_pixel(x, y, Rgba([color[0], color[1], color[2], alpha]));
        }
    }
    result
        .save_with_format(output, image::ImageFormat::Png)
        .map_err(to_string_error)?;

    // AB 回归可视化：设 AIAS_AB_DEBUG=1 时对单张图导出 matte 灰度图。
    if std::env::var_os("AIAS_AB_DEBUG").is_some() {
        let stem = output
            .file_stem()
            .and_then(|value| value.to_str())
            .unwrap_or("matte")
            .to_string();
        let mut matte_image = ImageBuffer::<Luma<u8>, Vec<u8>>::new(w, h);
        for (index, slot) in matte_image.pixels_mut().enumerate() {
            *slot = image::Luma([(mask[index] * 255.0).round().clamp(0.0, 255.0) as u8]);
        }
        let _ = matte_image.save(output.with_file_name(format!("{stem}_matte.png")));
    }
    Ok(CutoutOutcome {
        model_used: fallback_model.to_string(),
        fallback,
    })
}

/// toonout 是否「整图判前景」失效：复杂插画/线稿背景上它会保留整片背景。
/// 实测失效特征为前景占比异常高，且边缘仍有明显的半透明/不透明残留。
/// 此信号只触发一次高级模型复核；只有复核在两项指标上都显著更干净才会切换。
pub fn toonout_likely_failed(matte: &[f32], w: u32, h: u32) -> bool {
    const BAND_PX: usize = 24;
    const BORDER_MEAN_THRESHOLD: f32 = 0.25;
    const FG_RATIO_MIN: f32 = 0.67;

    if w < BAND_PX as u32 * 2 + 2 || h < BAND_PX as u32 * 2 + 2 {
        return false;
    }
    let border = border_band_mean(matte, w, h, BAND_PX);
    let fg = foreground_ratio(matte, w, h);
    border > BORDER_MEAN_THRESHOLD && fg > FG_RATIO_MIN
}

fn matte_is_substantially_cleaner(candidate: &[f32], baseline: &[f32], w: u32, h: u32) -> bool {
    if candidate.len() != baseline.len() || candidate.len() != (w * h) as usize {
        return false;
    }
    let border_improvement = border_band_mean(baseline, w, h, 24)
        - border_band_mean(candidate, w, h, 24);
    let foreground_improvement = foreground_ratio(baseline, w, h) - foreground_ratio(candidate, w, h);
    border_improvement >= 0.10 && foreground_improvement >= 0.08
}

/// 图像四周边框环带（band_px 宽）内的 alpha 均值。
fn border_band_mean(matte: &[f32], w: u32, h: u32, band_px: usize) -> f32 {
    let (w, h) = (w as usize, h as usize);
    let mut sum = 0f64;
    let mut count = 0usize;
    for y in 0..h {
        for x in 0..w {
            if x < band_px || y < band_px || x >= w - band_px || y >= h - band_px {
                sum += matte[y * w + x] as f64;
                count += 1;
            }
        }
    }
    if count == 0 {
        0.0
    } else {
        (sum / count as f64) as f32
    }
}

/// matte 中前景（alpha > 0.5）像素占全图的比例。
fn foreground_ratio(matte: &[f32], w: u32, h: u32) -> f32 {
    if w == 0 || h == 0 {
        return 0.0;
    }
    let count = matte.iter().filter(|value| **value > 0.5).count();
    count as f32 / (w * h) as f32
}

/// EXIF orientation via the format decoder; only JPEG actually carries it here.
fn exif_orientation(input: &Path) -> Result<Option<image::metadata::Orientation>, String> {
    let file = fs::File::open(input).map_err(to_string_error)?;
    let mut decoder = match image::codecs::jpeg::JpegDecoder::new(std::io::BufReader::new(file)) {
        Ok(decoder) => decoder,
        Err(_) => return Ok(None),
    };
    use image::ImageDecoder as _;
    Ok(decoder.orientation().ok())
}

fn to_string_error(error: impl std::fmt::Display) -> String {
    error.to_string()
}

#[cfg(test)]
mod toonout_tests {
    use super::*;

    #[test]
    fn probability_luma_applies_the_requested_confidence_threshold() {
        assert_eq!(
            probability_luma(&[0.0, 0.25, 0.5, 1.0], 0.3),
            [0, 0, 255, 255]
        );
    }

    #[test]
    fn small_foreground_islands_are_removed_without_eroding_the_subject() {
        let mut matte = vec![0.0; 36];
        for index in [7, 8, 13, 14, 15] {
            matte[index] = 1.0;
        }
        matte[35] = 1.0;

        remove_small_foreground_components(&mut matte, 6, 6, 2);

        assert_eq!(matte[35], 0.0, "单像素孤岛应被移除");
        for index in [7, 8, 13, 14, 15] {
            assert_eq!(matte[index], 1.0, "主体连通块不能被侵蚀");
        }
    }

    #[test]
    fn enclosed_background_holes_are_filled_without_touching_the_outer_background() {
        let mut matte = vec![1.0; 25];
        matte[12] = 0.0;
        matte[0] = 0.0;

        fill_small_background_holes(&mut matte, 5, 5, 2);

        assert_eq!(matte[12], 1.0, "被主体包围的小孔应被填充");
        assert_eq!(matte[0], 0.0, "与画面边缘相连的背景不能被填充");
    }

    #[test]
    fn toonout_complex_background_signal_catches_a_translucent_border_leak() {
        let (width, height) = (512, 512);
        let mut matte = vec![0.36; (width * height) as usize];
        for y in 24..height - 24 {
            for x in 24..width - 24 {
                matte[(y * width + x) as usize] = 1.0;
            }
        }

        assert!(toonout_likely_failed(&matte, width, height));
    }

    #[test]
    fn toonout_preprocess_fills_the_entire_model_input() {
        let source = RgbImage::from_pixel(1447, 2036, image::Rgb([17, 49, 91]));
        let resized = resize_birefnet_input(&source, 1024, 1024);
        assert_eq!(resized.dimensions(), (1024, 1024));
        assert_eq!(*resized.get_pixel(0, 0), image::Rgb([17, 49, 91]));
        assert_eq!(*resized.get_pixel(512, 512), image::Rgb([17, 49, 91]));
        assert_eq!(*resized.get_pixel(1023, 1023), image::Rgb([17, 49, 91]));
    }

    /// 端到端验证 ToonOut：下载 470MB 模型 + 真实推理，仅在手动运行：
    /// `cargo test toonout -- --ignored --nocapture`
    #[test]
    #[ignore = "下载并运行 470MB 模型，手动执行"]
    fn toonout_end_to_end() {
        let base = std::env::temp_dir().join("aias-toonout-e2e");
        fs::create_dir_all(&base).unwrap();
        download_model(None, &base, "toonout").unwrap();

        // 深蓝背景上一块亮橙色圆角主体，模型应保留主体、清除背景。
        let mut img = RgbImage::new(512, 384);
        for y in 0..384 {
            for x in 0..512 {
                img.put_pixel(x, y, image::Rgb([28, 34, 58]));
            }
        }
        for y in 96..288 {
            for x in 128..384 {
                img.put_pixel(x, y, image::Rgb([244, 150, 58]));
            }
        }
        let input = base.join("in.png");
        let output = base.join("out.png");
        img.save(&input).unwrap();

        cutout(&base, "toonout", &input, &output).unwrap();
        let out = image::open(&output).unwrap().to_rgba8();
        assert_eq!(out.dimensions(), (512, 384));

        let alpha = |x: u32, y: u32| out.get_pixel(x, y)[3];
        let center = alpha(256, 192);
        let corner = alpha(8, 8);
        assert!(center > 220, "主体中心应不透明，实际 {center}");
        assert!(corner < 60, "背景角应接近透明，实际 {corner}");
        println!("center={center} corner={corner}");
    }

    /// 用指定图片检查 ToonOut 质量：
    /// `AIAS_TOONOUT_TEST_IMAGE=/path/to/image cargo test toonout_real_image -- --ignored --nocapture`
    #[test]
    #[ignore = "运行 470MB 模型推理，手动执行"]
    fn toonout_real_image() {
        let base = std::env::temp_dir().join("aias-toonout-e2e");
        fs::create_dir_all(&base).unwrap();
        download_model(None, &base, "toonout").unwrap();

        let input_path = std::env::var_os("AIAS_TOONOUT_TEST_IMAGE")
            .map(PathBuf::from)
            .expect("请设置 AIAS_TOONOUT_TEST_IMAGE 指向测试图片");
        let rgb = image::open(&input_path).unwrap().to_rgb8();
        println!("input {}x{}", rgb.width(), rgb.height());

        ensure_ort_runtime(&base).unwrap();
        let path = models_dir(&base).join("birefnet-toonout-fp16.onnx");
        let session = build_session(&path, true).unwrap();
        println!("inputs: {:?}", session.inputs());
        println!("outputs: {:?}", session.outputs());

        let matte = run_birefnet(&base, "toonout", false, &rgb).unwrap();
        let (w, h) = rgb.dimensions();
        let (mut strong, mut weak, mut mid) = (0usize, 0usize, 0usize);
        let (mut border_sum, mut border_n) = (0f64, 0f64);
        for y in 0..h {
            for x in 0..w {
                let value = matte[y as usize * w as usize + x as usize];
                if value > 0.9 {
                    strong += 1
                } else if value < 0.1 {
                    weak += 1
                } else {
                    mid += 1
                }
                if x < 16 || y < 16 || x >= w - 16 || y >= h - 16 {
                    border_sum += f64::from(value);
                    border_n += 1.0;
                }
            }
        }
        let total = (w * h) as usize;
        println!(
            "strong={}% weak={}% mid={}% border_mean={:.4}",
            strong * 100 / total,
            weak * 100 / total,
            mid * 100 / total,
            border_sum / border_n
        );

        let output = base.join("out_real.png");
        cutout(&base, "toonout", &input_path, &output).unwrap();
        println!("saved {}", output.display());
    }

    /// A/B 基准：对 AIAS_AB_INPUT 目录（默认「测试」参考图文件夹）跑指定模型，
    /// 结果存 F:\AIAS\ab\<AIAS_AB_TAG>\{stem}_{model}.png，并生成
    /// {stem}_{model}_preview.jpg（原图 | 白底合成 | 深底合成）便于目测对比：
    /// `AIAS_AB_TAG=base cargo test ab_reference --release -- --ignored --nocapture`
    /// 可用 `AIAS_AB_MODELS=advanced` 限定单模型，避免 A/B 验证被其他模型的
    /// 显存需求阻断；默认仍依次运行 toonout、simple、advanced。
    #[test]
    #[ignore = "手动执行：真实模型推理"]
    fn ab_reference() {
        let base = dirs::home_dir()
            .expect("无法定位用户目录")
            .join(r"AppData\Roaming\studio.avroracl.aias");
        let input_dir = std::env::var_os("AIAS_AB_INPUT")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(r"F:\战争雷霆涂装\贴图素材\F15E 塞雷娅\测试"));
        let tag = std::env::var("AIAS_AB_TAG").unwrap_or_else(|_| "base".into());
        let selected_models: Vec<String> = std::env::var("AIAS_AB_MODELS")
            .unwrap_or_else(|_| "toonout,simple,advanced".into())
            .split(',')
            .map(str::trim)
            .filter(|model| !model.is_empty())
            .map(str::to_owned)
            .collect();
        assert!(
            !selected_models.is_empty(),
            "AIAS_AB_MODELS 至少需要指定一个模型"
        );
        for model in &selected_models {
            assert!(
                MODELS.iter().any(|spec| spec.id == model),
                "AIAS_AB_MODELS 不支持模型：{model}"
            );
        }
        let out_dir = PathBuf::from(r"F:\AIAS\ab").join(&tag);
        fs::create_dir_all(&out_dir).unwrap();
        ensure_ort_runtime(&base).unwrap();

        for entry in fs::read_dir(&input_dir).unwrap().flatten() {
            let path = entry.path();
            let ext = path
                .extension()
                .and_then(|value| value.to_str())
                .unwrap_or("")
                .to_ascii_lowercase();
            if !matches!(ext.as_str(), "png" | "jpg" | "jpeg" | "webp") {
                continue;
            }
            let stem = path
                .file_stem()
                .and_then(|value| value.to_str())
                .unwrap_or("image")
                .to_string();
            for model in &selected_models {
                let started = std::time::Instant::now();
                let output = out_dir.join(format!("{stem}_{model}.png"));
                let outcome = cutout_with_fallback(&base, model, &path, &output)
                    .unwrap_or_else(|error| panic!("{model} {stem}: {error}"));
                println!(
                    "{tag} {} {} 耗时 {:?} 实际模型={} 回退={} -> {}",
                    stem,
                    model,
                    started.elapsed(),
                    outcome.model_used,
                    outcome.fallback,
                    output.display()
                );
                let result = image::open(&output).unwrap().to_rgba8();
                let alpha: Vec<f32> = result
                    .pixels()
                    .map(|pixel| pixel[3] as f32 / 255.0)
                    .collect();
                println!(
                    "{tag} {} {} 前景占比={:.3} 边缘均值={:.3}",
                    stem,
                    model,
                    foreground_ratio(&alpha, result.width(), result.height()),
                    border_band_mean(&alpha, result.width(), result.height(), 24)
                );
                let preview = out_dir.join(format!("{stem}_{model}_preview.jpg"));
                ab_preview(&path, &result, &preview);
            }
        }
    }

    /// 横向拼三联图：原图 | 抠图白底合成 | 抠图深底合成，高度压到 1200 内。
    fn ab_preview(input: &Path, result: &RgbaImage, out: &Path) {
        let (w, h) = result.dimensions();
        let scale = (1200.0 / h as f32).min(1.0);
        let (tw, th) = (
            ((w as f32 * scale).round() as u32).max(1),
            ((h as f32 * scale).round() as u32).max(1),
        );
        let original = image::open(input)
            .expect("原图可读")
            .to_rgb8();
        let o_small = image::imageops::resize(&original, tw, th, FilterType::Triangle);
        let r_small = image::imageops::resize(result, tw, th, FilterType::Triangle);
        let mut over_white = RgbImage::new(tw, th);
        let mut over_dark = RgbImage::new(tw, th);
        for y in 0..th {
            for x in 0..tw {
                let pixel = r_small.get_pixel(x, y);
                let a = pixel[3] as f32 / 255.0;
                let blend = |bg: [f32; 3]| {
                    [
                        (pixel[0] as f32 * a + bg[0] * (1.0 - a)).round() as u8,
                        (pixel[1] as f32 * a + bg[1] * (1.0 - a)).round() as u8,
                        (pixel[2] as f32 * a + bg[2] * (1.0 - a)).round() as u8,
                    ]
                };
                over_white.put_pixel(x, y, image::Rgb(blend([255.0, 255.0, 255.0])));
                over_dark.put_pixel(x, y, image::Rgb(blend([38.0, 42.0, 50.0])));
            }
        }
        let gap = 8;
        let mut canvas = RgbImage::new(tw * 3 + gap * 2, th);
        image::imageops::overlay(&mut canvas, &o_small, 0, 0);
        image::imageops::overlay(&mut canvas, &over_white, (tw + gap) as i64, 0);
        image::imageops::overlay(&mut canvas, &over_dark, ((tw + gap) * 2) as i64, 0);
        canvas.save(out).expect("预览图写出");
    }

    /// 手动下载并安装 GPU 版 onnxruntime：
    /// `cargo test gpu_ort_install_manual -- --ignored --nocapture`
    /// 完成后运行 `cuda_ep_diagnostic` 验证 CUDA 是否真正生效。
    #[test]
    #[ignore = "手动执行：下载约 233MB 运行库"]
    fn gpu_ort_install_manual() {
        let base = dirs::home_dir()
            .expect("无法定位用户目录")
            .join(r"AppData\Roaming\studio.avroracl.aias");
        install_gpu_ort(None, &base).unwrap();
        assert!(gpu_ort_ready(&base));
        println!("installed at {}", gpu_ort_capi_dir(&base).display());
    }

    /// 诊断 CUDA EP 是否真正生效（会话日志 + 推理耗时 + 进程模块核对）：
    /// `cargo test cuda_ep_diagnostic -- --ignored --nocapture`
    /// 运行期间可用以下命令核对本进程加载的 CUDA 模块：
    /// `powershell "Get-Process -Id <pid> -Module | ? { $_.ModuleName -match 'cuda|cudnn|onnxruntime' } | select ModuleName,FileName"`
    #[test]
    #[ignore = "手动执行：需要本机已装抠图模型"]
    fn cuda_ep_diagnostic() {
        use std::sync::Arc;
        ort::init()
            .with_name("aias-cuda-diag")
            .with_logger(Arc::new(|level, category, _id, code_location, message| {
                println!("[ORT {:?}] {} {} {}", level, category, code_location, message);
            }))
            .commit();

        let base = dirs::home_dir()
            .expect("无法定位用户目录")
            .join(r"AppData\Roaming\studio.avroracl.aias");
        ensure_ort_runtime(&base).unwrap();
        let model = models_dir(&base).join("isnetis.onnx");
        assert!(model.exists(), "测试需要已安装 isnetis.onnx");

        println!("pid={}", std::process::id());
        let compiled_with_cuda = ort::ep::CUDA::default().is_available();
        println!("ORT 编译时包含 CUDA EP：{compiled_with_cuda:?}");
        let started = std::time::Instant::now();
        let session = build_session(&model, true).unwrap();
        println!("session built in {:?}", started.elapsed());
        drop(session);

        let mut img = RgbImage::new(1024, 1024);
        for y in 0..1024 {
            for x in 0..1024 {
                let inside = (128..896).contains(&x) && (128..896).contains(&y);
                img.put_pixel(x, y, if inside { image::Rgb([244, 150, 58]) } else { image::Rgb([28, 34, 58]) });
            }
        }
        let input = base.join("cuda-diag-in.png");
        let output = base.join("cuda-diag-out.png");
        img.save(&input).unwrap();

        cutout(&base, "simple", &input, &output).unwrap(); // 预热（含 cuDNN 选核）
        use ort::ep::ExecutionProvider;
        println!(
            "预热后 CUDA EP 编译状态：{:?}（true 表示当前加载的是 GPU 版运行库）",
            ort::ep::CUDA::default().is_available()
        );
        for run in 1..=3 {
            let started = std::time::Instant::now();
            cutout(&base, "simple", &input, &output).unwrap();
            println!("run {run}: {:?}", started.elapsed());
        }

        println!("===== 15 秒内核对进程模块（命令见测试注释）=====");
        std::thread::sleep(std::time::Duration::from_secs(15));
    }
}
