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
const BIREFNET_GENERAL_1024_FP16_SIZE: u64 = 489_666_272;
const ANIME_SPECIALIST_SIZE: u64 = 117_239_813;

pub const MODELS: &[ModelSpec] = &[
    // 专为二次元角色分割训练的动态 ONNX。正式推理固定为经人工真值回归验证的
    // 1024 方形输入，而不是按原图尺寸无限放大；后者在复杂背景上会重拾远景角色。
    ModelSpec {
        id: "anime-specialist",
        label: "动漫专精（AnimeSeg）",
        kind: ModelKind::BiRefNet { matting: false },
        files: &[ModelFileSpec {
            name: "birefnext-aniseg-int8-v0.1.onnx",
            size: ANIME_SPECIALIST_SIZE,
            mirror_url: "https://hf-mirror.com/nkta/birefnext-aniseg-ONNX/resolve/15ad03e6479a16f02cd9e10e78de2b6639f9adbb/birefnext-aniseg-int8-v0.1.onnx",
            origin_url: "https://huggingface.co/nkta/birefnext-aniseg-ONNX/resolve/15ad03e6479a16f02cd9e10e78de2b6639f9adbb/birefnext-aniseg-int8-v0.1.onnx",
        }],
    },
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
        label: "高质量抠图（BiRefNet 1024）",
        kind: ModelKind::BiRefNet { matting: false },
        files: &[ModelFileSpec {
            name: "birefnet-general-1024-fp16.onnx",
            size: BIREFNET_GENERAL_1024_FP16_SIZE,
            mirror_url: "https://hf-mirror.com/onnx-community/BiRefNet-ONNX/resolve/534d3c82d3bb8b2f0867db6dfbc3a525b8e42f67/onnx/model_fp16.onnx",
            origin_url: "https://huggingface.co/onnx-community/BiRefNet-ONNX/resolve/534d3c82d3bb8b2f0867db6dfbc3a525b8e42f67/onnx/model_fp16.onnx",
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

/// 供调用层把实际接管的模型 id 转成人类可读日志；未知 id 仍保留原值，
/// 以免错误信息丢失关键信息。
pub fn model_label(id: &str) -> String {
    model_spec(id)
        .map(|model| model.label.to_string())
        .unwrap_or_else(|_| id.to_string())
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

// P5 GT 诊断开关：只在测试构建中允许逐项绕过后处理，用于确定性 A/B；
// 正式应用始终保持完整管线，避免环境变量改变用户产物。
#[cfg(test)]
fn ab_postprocess_stage_enabled(stage: &str) -> bool {
    !std::env::var("AIAS_AB_DISABLE_STAGES")
        .unwrap_or_default()
        .split(',')
        .map(str::trim)
        .any(|disabled| disabled.eq_ignore_ascii_case(stage))
}

#[cfg(not(test))]
fn ab_postprocess_stage_enabled(_: &str) -> bool {
    true
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

/// 背景次级孤岛移除：模型偶尔把背景里的次要元素（背景人物、飘落物件）连成
/// 独立前景块保留下来。单主体场景的判据：面积不足主件 2%、且与主件的
/// Chebyshev 间距超过 32px 的连通块整块清除；与主体相邻或面积可观的块不动。
/// GT 验证五个模型 miss 均零增加，双主体以外的常规图自门控零变化。
fn remove_background_islands(matte: &mut [f32], width: u32, height: u32) {
    let (w, h) = (width as usize, height as usize);
    if w == 0 || h == 0 || matte.len() != w.saturating_mul(h) {
        return;
    }
    const MAX_AREA_RATIO: usize = 50; // 面积 < 主件 / 50
    const GAP_PX: u32 = 32;

    // 8 连通域标记（0 = 背景，域号从 1 起）
    let mut label = vec![0usize; w * h];
    let mut areas: Vec<usize> = vec![0];
    let mut queue = Vec::new();
    for start in 0..matte.len() {
        if matte[start] <= 0.5 || label[start] != 0 {
            continue;
        }
        let id = areas.len();
        areas.push(0);
        label[start] = id;
        queue.clear();
        queue.push(start);
        let mut cursor = 0;
        while cursor < queue.len() {
            let index = queue[cursor];
            cursor += 1;
            areas[id] += 1;
            let x = index % w;
            let y = index / w;
            let y0 = y.saturating_sub(1);
            let y1 = (y + 1).min(h - 1);
            let x0 = x.saturating_sub(1);
            let x1 = (x + 1).min(w - 1);
            for ny in y0..=y1 {
                for nx in x0..=x1 {
                    let neighbor = ny * w + nx;
                    if label[neighbor] == 0 && matte[neighbor] > 0.5 {
                        label[neighbor] = id;
                        queue.push(neighbor);
                    }
                }
            }
        }
    }
    if areas.len() <= 2 {
        return;
    }
    let main_id = (1..areas.len())
        .max_by_key(|id| areas[*id])
        .expect("至少两个连通域");
    let main_area = areas[main_id];

    // 到主件的 Chebyshev 距离（双向两遍扫描，步长全为 1）
    let big = u32::MAX;
    let mut dist = vec![big; w * h];
    for (index, slot) in dist.iter_mut().enumerate() {
        if label[index] == main_id {
            *slot = 0;
        }
    }
    for y in 0..h {
        for x in 0..w {
            let index = y * w + x;
            if dist[index] == 0 {
                continue;
            }
            let mut best = dist[index];
            if y > 0 {
                best = best.min(dist[index - w].saturating_add(1));
                if x > 0 {
                    best = best.min(dist[index - w - 1].saturating_add(1));
                }
                if x + 1 < w {
                    best = best.min(dist[index - w + 1].saturating_add(1));
                }
            }
            if x > 0 {
                best = best.min(dist[index - 1].saturating_add(1));
            }
            dist[index] = best;
        }
    }
    for y in (0..h).rev() {
        for x in (0..w).rev() {
            let index = y * w + x;
            if dist[index] == 0 {
                continue;
            }
            let mut best = dist[index];
            if y + 1 < h {
                best = best.min(dist[index + w].saturating_add(1));
                if x + 1 < w {
                    best = best.min(dist[index + w + 1].saturating_add(1));
                }
                if x > 0 {
                    best = best.min(dist[index + w - 1].saturating_add(1));
                }
            }
            if x + 1 < w {
                best = best.min(dist[index + 1].saturating_add(1));
            }
            dist[index] = best;
        }
    }

    // 各小域到主件的最近距离 → 间距 = 距离 - 1
    let mut nearest = vec![big; areas.len()];
    for (index, id) in label.iter().enumerate() {
        if *id == 0 || *id == main_id {
            continue;
        }
        nearest[*id] = nearest[*id].min(dist[index]);
    }
    for (index, id) in label.iter().enumerate() {
        if *id == 0 || *id == main_id || areas[*id] * MAX_AREA_RATIO >= main_area {
            continue;
        }
        if nearest[*id].saturating_sub(1) > GAP_PX {
            matte[index] = 0.0;
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

/// RTMDet 的一个角色实例候选。默认仍选最高检测置信度；开发期可用面积与
/// 中心位置做重排序，验证「最大且最居中主体」能否减少背景角色误选。
#[derive(Debug, Clone, Copy)]
struct CharacterCandidate {
    confidence: f32,
    stride: u32,
    row: usize,
    col: usize,
    x1: f32,
    y1: f32,
    x2: f32,
    y2: f32,
}

fn main_subject_score(candidate: CharacterCandidate, width: u32, height: u32) -> f32 {
    let area = ((candidate.x2 - candidate.x1).max(0.0)
        * (candidate.y2 - candidate.y1).max(0.0)
        / (width.max(1) as f32 * height.max(1) as f32))
        .clamp(0.0, 1.0);
    let center_x = (candidate.x1 + candidate.x2) * 0.5;
    let center_y = (candidate.y1 + candidate.y2) * 0.5;
    let dx = (center_x - width as f32 * 0.5) / (width.max(1) as f32 * 0.5);
    let dy = (center_y - height as f32 * 0.5) / (height.max(1) as f32 * 0.5);
    let center = (1.0 - (dx * dx + dy * dy).sqrt() / std::f32::consts::SQRT_2).clamp(0.0, 1.0);
    // 检测分数用于过滤明显无关物，面积和中心共同决定哪个实例是画面的主角。
    0.22 * candidate.confidence + 0.48 * area.sqrt() + 0.30 * center
}

#[cfg(test)]
fn ab_main_subject_selector_enabled() -> bool {
    std::env::var("AIAS_AB_INSTANCE_SELECTOR")
        .ok()
        .is_some_and(|value| value.eq_ignore_ascii_case("main-subject"))
}

#[cfg(not(test))]
fn ab_main_subject_selector_enabled() -> bool {
    false
}

fn choose_character_candidate(
    candidates: &[CharacterCandidate],
    width: u32,
    height: u32,
) -> Option<CharacterCandidate> {
    let subject_selector = ab_main_subject_selector_enabled();
    candidates.iter().copied().max_by(|left, right| {
        let left_score = if subject_selector {
            main_subject_score(*left, width, height)
        } else {
            left.confidence
        };
        let right_score = if subject_selector {
            main_subject_score(*right, width, height)
        } else {
            right.confidence
        };
        left_score.total_cmp(&right_score)
    })
}

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

    // Decode every plausible anchor. Production keeps historical
    // "highest confidence" behaviour; the development A/B selector can rank
    // the same candidates by confidence, area, and distance to image centre.
    let mut candidates = Vec::new();
    for stride in STRIDES {
        let name = format!("scores.stride{stride}");
        let (shape, logits) = outputs[name.as_str()]
            .try_extract_tensor::<f32>()
            .map_err(to_string_error)?;
        let grid_h = (*shape.get(2).ok_or("scores shape 无效")?) as usize;
        let grid_w = (*shape.get(3).ok_or("scores shape 无效")?) as usize;
        let bbox_name = format!("bboxes.stride{stride}");
        let (bbox_shape, bboxes) = outputs[bbox_name.as_str()]
            .try_extract_tensor::<f32>()
            .map_err(to_string_error)?;
        let bbox_grid_h = (*bbox_shape.get(2).ok_or("bboxes shape 无效")?) as usize;
        let bbox_grid_w = (*bbox_shape.get(3).ok_or("bboxes shape 无效")?) as usize;
        let bbox_plane = bbox_grid_h * bbox_grid_w;
        if bbox_grid_h != grid_h || bbox_grid_w != grid_w {
            return Err("scores 与 bboxes 网格尺寸不一致".into());
        }
        for (index, &logit) in logits.iter().enumerate().take(grid_h * grid_w) {
            let prob = stable_sigmoid(logit);
            // 原正式管线从不按阈值丢弃锚点，保留该行为；开发期主体重排序才
            // 忽略低置信度候选，避免大框噪声凭面积取胜。
            if ab_main_subject_selector_enabled() && prob < DETECTION_THRESHOLD {
                continue;
            }
            let row = index / grid_w;
            let col = index % grid_w;
            let x_anchor = col as f32 * stride as f32;
            let y_anchor = row as f32 * stride as f32;
            let bx1 = x_anchor - bboxes[index];
            let by1 = y_anchor - bboxes[bbox_plane + index];
            let bx2 = x_anchor + bboxes[2 * bbox_plane + index];
            let by2 = y_anchor + bboxes[3 * bbox_plane + index];
            let eff_w = (seg_w - 2 * pad_w) as f32;
            let eff_h = (seg_h - 2 * pad_h) as f32;
            let x1 = ((bx1 - pad_w as f32) * w as f32 / eff_w).clamp(0.0, w as f32);
            let x2 = ((bx2 - pad_w as f32) * w as f32 / eff_w).clamp(0.0, w as f32);
            let y1 = ((by1 - pad_h as f32) * h as f32 / eff_h).clamp(0.0, h as f32);
            let y2 = ((by2 - pad_h as f32) * h as f32 / eff_h).clamp(0.0, h as f32);
            if x2 <= x1 + 2.0 || y2 <= y1 + 2.0 {
                continue;
            }
            candidates.push(CharacterCandidate {
                confidence: prob,
                stride,
                row,
                col,
                x1,
                y1,
                x2,
                y2,
            });
        }
    }

    let best = choose_character_candidate(&candidates, w, h).ok_or("未检测到角色实例")?;
    #[cfg(test)]
    if ab_main_subject_selector_enabled() {
        let mut ranked = candidates;
        ranked.sort_by(|left, right| {
            main_subject_score(*right, w, h).total_cmp(&main_subject_score(*left, w, h))
        });
        for candidate in ranked.iter().take(3) {
            println!(
                "[AIAS-INSTANCE] conf={:.3} main={:.3} box=({:.0},{:.0})-({:.0},{:.0})",
                candidate.confidence,
                main_subject_score(*candidate, w, h),
                candidate.x1,
                candidate.y1,
                candidate.x2,
                candidate.y2,
            );
        }
    }
    let best_stride = best.stride;
    let best_row = best.row;
    let best_col = best.col;
    let center_x = (best.x1 + best.x2) * 0.5;
    let center_y = (best.y1 + best.y2) * 0.5;

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
        image::imageops::resize(rgb, target_w, target_h, birefnet_resize_filter())
    }
}

/// 上游 `preprocessor_config.json` 的 `resample: 2` 是双线性插值；A/B 仍可
/// 显式切回其它插值验证，以避免后续调整悄悄偏离该工作流。
fn birefnet_resize_filter() -> FilterType {
    #[cfg(test)]
    if let Ok(value) = std::env::var("AIAS_AB_RESAMPLE") {
        match value.trim().to_ascii_lowercase().as_str() {
            "bilinear" | "triangle" => return FilterType::Triangle,
            "lanczos" | "lanczos3" => return FilterType::Lanczos3,
            "catmull" | "catmullrom" => return FilterType::CatmullRom,
            _ => {}
        }
    }
    FilterType::Triangle
}

/// 早期 512 官方导出需要每图 range normalization 才能避免低置信度全透明；
/// 保留开发期开关以验证新导出是否仍需这项兼容处理，正式默认保持启用。
fn birefnet_range_normalization_enabled() -> bool {
    #[cfg(test)]
    if std::env::var_os("AIAS_AB_DISABLE_BIREFNET_MINMAX").is_some() {
        return false;
    }
    true
}

/// BiRefNet 系共享推理管线入口：ToonOut（动漫微调）与官方 general/portrait/HR/lite
/// 预处理完全一致，仅输入分辨率随模型不同（1024/2048，从会话动态读取）。
/// `matting` 为 true 表示模型输出连续 alpha（人像类），后处理不做对比度拉伸。
///
/// 官方 fp32 导出在 1024² 下显存占用约为 fp16 的两倍（整图 ASPP 中间张量单笔
/// 约 784MB），桌面应用占用较多显存的卡上 GPU 推理会 OOM：此时释放 GPU 会话、
/// 改用 CPU 重建并重试一次，保证出图（慢但可用）。
fn run_birefnet(base: &Path, id: &str, matting: bool, rgb: &RgbImage) -> Result<Vec<f32>, String> {
    #[cfg(test)]
    if std::env::var_os("AIAS_AB_FORCE_CPU").is_some() {
        return run_birefnet_on_provider(base, id, matting, rgb, false);
    }
    match run_birefnet_on_provider(base, id, matting, rgb, true) {
        Ok(mask) => Ok(mask),
        Err(error) if is_gpu_oom_error(&error) => {
            release_birefnet_session(id);
            run_birefnet_on_provider(base, id, matting, rgb, false)
        }
        Err(error) => Err(error),
    }
}

/// 高质量 General 1024 使用水平翻转增强：同一会话串行推理原图和翻转图，
/// 翻回后平均 alpha。它能消除模型的左右偏置、改善动漫细发梢；串行执行，
/// 不会同时持有两份模型中间张量。其它模型保留单次推理，避免无依据的耗时增加。
fn run_birefnet_on_provider(
    base: &Path,
    id: &str,
    matting: bool,
    rgb: &RgbImage,
    use_gpu: bool,
) -> Result<Vec<f32>, String> {
    let direct = try_run_birefnet(base, id, matting, rgb, use_gpu)?;
    if id != "birefnet-general" {
        return Ok(direct);
    }
    let flipped = image::imageops::flip_horizontal(rgb);
    let flipped_mask = try_run_birefnet(base, id, matting, &flipped, use_gpu)?;
    Ok(mean_with_horizontal_flip(
        &direct,
        &flipped_mask,
        rgb.width(),
        rgb.height(),
    ))
}

fn mean_with_horizontal_flip(
    direct: &[f32],
    flipped: &[f32],
    width: u32,
    height: u32,
) -> Vec<f32> {
    let (w, h) = (width as usize, height as usize);
    assert_eq!(direct.len(), w * h, "原始 alpha 尺寸无效");
    assert_eq!(flipped.len(), w * h, "翻转 alpha 尺寸无效");
    let mut result = vec![0.0; direct.len()];
    for y in 0..h {
        for x in 0..w {
            let index = y * w + x;
            result[index] = (direct[index] + flipped[y * w + (w - 1 - x)]) * 0.5;
        }
    }
    result
}

#[cfg(test)]
fn run_birefnet_single_for_test(
    base: &Path,
    id: &str,
    matting: bool,
    rgb: &RgbImage,
) -> Result<Vec<f32>, String> {
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
    let path = models_dir(base).join(file_name);
    try_run_birefnet_path(
        base,
        id,
        &path,
        matches!(spec.kind, ModelKind::BiRefNet { .. }),
        matting,
        rgb,
        use_gpu,
    )
}

/// 按模型文件执行 BiRefNet 推理。正式路径仍由 `try_run_birefnet` 通过注册表
/// 调用；这个更底层的入口只用于开发期验证本地官方导出，避免把未验证的模型
/// 暴露到正式 UI 或下载目录。
fn try_run_birefnet_path(
    base: &Path,
    session_id: &str,
    path: &Path,
    normalize_range: bool,
    matting: bool,
    rgb: &RgbImage,
    use_gpu: bool,
) -> Result<Vec<f32>, String> {
    ensure_ort_runtime(base)?;
    prune_sessions(SessionKeep::Birefnet(session_id));
    let mut sessions = birefnet_sessions().lock().map_err(lock_error)?;
    if !sessions.iter().any(|(model_id, _)| model_id == session_id) {
        if !path.exists() {
            return Err(format!("BiRefNet 模型文件不存在：{}", path.display()));
        }
        sessions.push((session_id.to_string(), build_session(path, use_gpu)?));
    }
    let session = sessions
        .iter_mut()
        .find(|(model_id, _)| model_id == session_id)
        .map(|(_, session)| session)
        .expect("session just ensured");
    let (w, h) = rgb.dimensions();

    // Exports fix the input square; fall back if a rebuild is dynamic.
    let (mut seg_h, mut seg_w) = input_size(session).unwrap_or((1024, 1024));
    // 动态 shape 的外部候选只能在开发期按显式尺寸复核；正式模型仍严格使用
    // 各自导出声明的输入尺寸，避免环境变量悄然改变用户侧输出或显存占用。
    #[cfg(test)]
    if session_id.starts_with("ab-local-") {
        if let Some(size) = std::env::var("AIAS_AB_LOCAL_INPUT_SIZE")
            .ok()
            .and_then(|value| value.parse::<usize>().ok())
            .filter(|size| (256..=2048).contains(size) && size % 32 == 0)
        {
            seg_h = size;
            seg_w = size;
        }
    }
    #[cfg(test)]
    if session_id.starts_with("ab-local-") {
        println!(
            "[AIAS-LOCAL-MODEL] session={session_id} input={}x{} model={}",
            seg_w,
            seg_h,
            path.display()
        );
    }

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
    if normalize_range && birefnet_range_normalization_enabled() {
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

#[cfg(test)]
fn run_birefnet_local_file(
    base: &Path,
    session_id: &str,
    path: &Path,
    normalize_range: bool,
    matting: bool,
    rgb: &RgbImage,
) -> Result<Vec<f32>, String> {
    if std::env::var_os("AIAS_AB_FORCE_CPU").is_some() {
        return try_run_birefnet_path(
            base,
            session_id,
            path,
            normalize_range,
            matting,
            rgb,
            false,
        );
    }
    match try_run_birefnet_path(
        base,
        session_id,
        path,
        normalize_range,
        matting,
        rgb,
        true,
    ) {
        Ok(mask) => Ok(mask),
        Err(error) if is_gpu_oom_error(&error) => {
            release_birefnet_session(session_id);
            try_run_birefnet_path(
                base,
                session_id,
                path,
                normalize_range,
                matting,
                rgb,
                false,
            )
        }
        Err(error) => Err(error),
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

/// O(n) 积分图盒均值（f32 版）：引导滤波要用约 17 路均值，f64 版会带来
/// 数百 MB 瞬时内存；累加仍走 f64 积分图保证精度，输入输出用 f32。
fn box_mean_f32(values: &[f32], w: usize, h: usize, radius: usize) -> Vec<f32> {
    let stride = w + 1;
    let mut sat = vec![0f64; stride * (h + 1)];
    for y in 0..h {
        let mut row_sum = 0f64;
        for x in 0..w {
            row_sum += values[y * w + x] as f64;
            sat[(y + 1) * stride + (x + 1)] = sat[y * stride + (x + 1)] + row_sum;
        }
    }
    let mut out = vec![0f32; w * h];
    for y in 0..h {
        let y0 = y.saturating_sub(radius);
        let y1 = (y + radius + 1).min(h);
        for x in 0..w {
            let x0 = x.saturating_sub(radius);
            let x1 = (x + radius + 1).min(w);
            let area = ((y1 - y0) * (x1 - x0)) as f64;
            let sum = sat[y1 * stride + x1] - sat[y0 * stride + x1] - sat[y1 * stride + x0]
                + sat[y0 * stride + x0];
            out[y * w + x] = (sum / area) as f32;
        }
    }
    out
}

/// RGB 引导的快速引导滤波（He et al.）：低分辨率推理的掩码上采样后边缘
/// 软糊（512² 模型放大 4 倍时过渡带约 8px、发丝尖端糊成圆头），用原图
/// 做引导把 alpha 贴回真实结构——原图里发丝轮廓是清晰的，滤波后过渡带
/// 收窄到 1-2px，糊住的尖端重新分开。eps 越小越贴合强边缘；平坦区域
/// a→0 退化为均值，掩码不会被过度改动。
fn guided_filter_matte(rgb: &RgbImage, p: &[f32], radius: usize, eps: f64) -> Vec<f32> {
    let (w, h) = rgb.dimensions();
    let (w, h) = (w as usize, h as usize);
    let n = w * h;
    if n == 0 || p.len() != n || radius == 0 {
        return p.to_vec();
    }
    let radius = radius.min(w.saturating_sub(1)).min(h.saturating_sub(1)).max(1);

    let mut r = vec![0f32; n];
    let mut g = vec![0f32; n];
    let mut b = vec![0f32; n];
    for (i, pixel) in rgb.pixels().enumerate() {
        r[i] = pixel[0] as f32 / 255.0;
        g[i] = pixel[1] as f32 / 255.0;
        b[i] = pixel[2] as f32 / 255.0;
    }

    let mean = |v: &[f32]| box_mean_f32(v, w, h, radius);
    let mr = mean(&r);
    let mg = mean(&g);
    let mb = mean(&b);
    let mp = mean(p);

    // 协方差所需的二次项均值；逐项生成乘积数组，用完即弃控制内存。
    let mut prod = vec![0f32; n];
    let mut pair_mean =
        |a: &[f32], bch: &[f32]| -> Vec<f32> {
            for i in 0..n {
                prod[i] = a[i] * bch[i];
            }
            box_mean_f32(&prod, w, h, radius)
        };
    let mrr = pair_mean(&r, &r);
    let mrg = pair_mean(&r, &g);
    let mrb = pair_mean(&r, &b);
    let mgg = pair_mean(&g, &g);
    let mgb = pair_mean(&g, &b);
    let mbb = pair_mean(&b, &b);
    let mrp = pair_mean(&r, p);
    let mgp = pair_mean(&g, p);
    let mbp = pair_mean(&b, p);

    // 逐像素解 3×3 线性方程 (cov_II + eps·I)·a = cov_Ip，再 b = p̄ − a·Ī。
    let mut a1 = vec![0f32; n];
    let mut a2 = vec![0f32; n];
    let mut a3 = vec![0f32; n];
    let mut bb = vec![0f32; n];
    for i in 0..n {
        let vr = (mrr[i] - mr[i] * mr[i]) as f64 + eps;
        let vg = (mgg[i] - mg[i] * mg[i]) as f64 + eps;
        let vb = (mbb[i] - mb[i] * mb[i]) as f64 + eps;
        let vrg = (mrg[i] - mr[i] * mg[i]) as f64;
        let vrb = (mrb[i] - mr[i] * mb[i]) as f64;
        let vgb = (mgb[i] - mg[i] * mb[i]) as f64;
        let crp = (mrp[i] - mr[i] * mp[i]) as f64;
        let cgp = (mgp[i] - mg[i] * mp[i]) as f64;
        let cbp = (mbp[i] - mb[i] * mp[i]) as f64;
        // 余因子法求逆（对称矩阵，C 与其转置相同）
        let c00 = vg * vb - vgb * vgb;
        let c01 = vrb * vgb - vrg * vb;
        let c02 = vrg * vgb - vg * vrb;
        let c11 = vr * vb - vrb * vrb;
        let c12 = vrg * vrb - vr * vgb;
        let c22 = vr * vg - vrg * vrg;
        let det = vr * c00 + vrg * c01 + vrb * c02;
        if det.abs() < 1e-20 {
            a1[i] = 0.0;
            a2[i] = 0.0;
            a3[i] = 0.0;
        } else {
            let inv = 1.0 / det;
            a1[i] = ((c00 * crp + c01 * cgp + c02 * cbp) * inv) as f32;
            a2[i] = ((c01 * crp + c11 * cgp + c12 * cbp) * inv) as f32;
            a3[i] = ((c02 * crp + c12 * cgp + c22 * cbp) * inv) as f32;
        }
        bb[i] = mp[i] - a1[i] * mr[i] - a2[i] * mg[i] - a3[i] * mb[i];
    }

    // 标准 fast guided filter 第二步：对 a、b 做盒均值后再合成，避免贴边振铃。
    let ma1 = mean(&a1);
    let ma2 = mean(&a2);
    let ma3 = mean(&a3);
    let mb2 = mean(&bb);
    let mut q = vec![0f32; n];
    for i in 0..n {
        q[i] = (ma1[i] * r[i] + ma2[i] * g[i] + ma3[i] * b[i] + mb2[i]).clamp(0.0, 1.0);
    }
    q
}

/// 局部颜色证据整定：512 级模型对细渐变结构（淡紫发丝、薄纱）系统性输出
/// 中间 alpha——下采样时细结构与背景混在一格，模型给不出确定置信度，
/// 换底后整个结构呈半透明“幽灵”。全分辨率上有模型没有的颜色证据：
/// 用 a²/(1-a)² 加权估计局部前景色 F 与背景色 B，再按未混色距离分类——
/// 中间像素颜色明显接近 F → 推到实心；明显接近 B → 归零；都接近/都远
/// → 维持原值。窗口内的实心像素自动主导颜色估计（权重是平方）。
fn solidify_subject(rgb: &RgbImage, mask: &mut [f32], w: u32, h: u32) {
    let (w, h) = (w as usize, h as usize);
    let n = w * h;
    if n == 0 || mask.len() != n {
        return;
    }
    const RADIUS: usize = 12;
    const MIN_A: f32 = 0.25;
    const MAX_KEEP: f32 = 0.92;
    const MIN_KEEP: f32 = 0.12;

    let weight_fg: Vec<f32> = mask.iter().map(|a| a * a).collect();
    let weight_bg: Vec<f32> = mask.iter().map(|a| (1.0 - a) * (1.0 - a)).collect();

    let mut product = vec![0f32; n];
    let mut weighted_mean = |weight: &[f32], channel: &[f32]| -> Vec<f32> {
        for i in 0..n {
            product[i] = weight[i] * channel[i];
        }
        box_mean_f32(&product, w, h, RADIUS)
    };
    let sum = |weight: &[f32]| -> Vec<f32> { box_mean_f32(weight, w, h, RADIUS) };

    let mut channels = [vec![0f32; n], vec![0f32; n], vec![0f32; n]];
    for (i, pixel) in rgb.pixels().enumerate() {
        channels[0][i] = pixel[0] as f32;
        channels[1][i] = pixel[1] as f32;
        channels[2][i] = pixel[2] as f32;
    }

    let wsum_fg = sum(&weight_fg);
    let wsum_bg = sum(&weight_bg);
    let mut fg: [Vec<f32>; 3] = Default::default();
    let mut bg: [Vec<f32>; 3] = Default::default();
    for ch in 0..3 {
        fg[ch] = weighted_mean(&weight_fg, &channels[ch]);
        bg[ch] = weighted_mean(&weight_bg, &channels[ch]);
    }

    for i in 0..n {
        let a = mask[i];
        if a <= MIN_KEEP || a >= MAX_KEEP {
            continue;
        }
        let (sf, sb) = (wsum_fg[i], wsum_bg[i]);
        if sf < 1e-4 || sb < 1e-4 {
            continue;
        }
        let mut df = 0.0f32;
        let mut db = 0.0f32;
        for ch in 0..3 {
            let c = channels[ch][i];
            let f = fg[ch][i] / sf;
            let b = bg[ch][i] / sb;
            df += (c - f) * (c - f);
            db += (c - b) * (c - b);
        }
        if a > MIN_A && df * 1.1 < db {
            // 颜色站在前景一边：细结构推到实心，换底后不再透底。
            mask[i] = 1.0;
        } else if a < 0.85 && db * 0.8 < df {
            // 颜色站在背景一边：中间置信度的背景残迹归零。
            mask[i] = 0.0;
        }
    }
}

/// 1024 General 已在原始图尺寸保留足够的边界细节；再做引导滤波会沿画面中的
/// 线稿偏移 alpha，实测增加边界误差。其它低分辨率模型仍依赖引导滤波贴回边缘。
fn model_uses_native_edge_alpha(model_id: &str) -> bool {
    matches!(model_id, "birefnet-general" | "anime-specialist")
}

/// 开发期可通过 `AIAS_AB_ALPHA_FLOOR` 量化不同的极淡 alpha 清理阈值；
/// 正式流程固定使用经回归验证的默认值，避免用户环境变量意外改变产品输出。
fn low_alpha_floor() -> f32 {
    #[cfg(test)]
    if let Some(value) = std::env::var("AIAS_AB_ALPHA_FLOOR")
        .ok()
        .and_then(|value| value.parse::<f32>().ok())
        .filter(|value| value.is_finite() && (0.0..=0.25).contains(value))
    {
        return value;
    }
    0.08
}

/// 将任意模型的全尺寸 alpha 套入统一的正式后处理与去污染步骤。
/// 单独抽出是为了让开发期的本地模型验证也能与正式输出逐像素同口径比较；
/// 它不改变任何正式模型选择、下载或 UI 行为。
fn finalize_cutout_image(
    rgb: &RgbImage,
    mask: Vec<f32>,
    preserve_native_edges: bool,
) -> RgbaImage {
    finalize_cutout_image_with_alpha_gamma(rgb, mask, preserve_native_edges, 1.0)
}

/// `alpha_gamma` 是开发期边缘校准实验的显式入口。正式流程固定传入 1.0；
/// 只有人工真值证明某个曲线能在保住主体的同时减少边缘外溢，才会讨论产品化。
fn finalize_cutout_image_with_alpha_gamma(
    rgb: &RgbImage,
    mask: Vec<f32>,
    preserve_native_edges: bool,
    alpha_gamma: f32,
) -> RgbaImage {
    let (w, h) = rgb.dimensions();
    // 引导滤波：低分辨率推理的软边掩码贴回原图结构，过渡带收窄、糊住的
    // 发丝尖端分开；先于残留清理执行，滤波沿背景线条的微溢出由后续清理兜底。
    let mut mask = if !preserve_native_edges && ab_postprocess_stage_enabled("guided") {
        guided_filter_matte(rgb, &mask, 8, 5e-4)
    } else {
        mask
    };
    // 颜色证据整定：把模型低置信度的细结构按全分辨率颜色归类到实心/透明。
    if ab_postprocess_stage_enabled("solidify") {
        solidify_subject(rgb, &mut mask, w, h);
    }

    // 幽灵残留抑制先于去污染：碎屑清除后，过渡带背景色估计更准。
    if ab_postprocess_stage_enabled("ghost") {
        suppress_background_ghosts(&mut mask, w, h);
    }
    // 极淡残雾归零：上采样振铃和滤波残余的极低 alpha 在换底上呈灰雾。
    if ab_postprocess_stage_enabled("threshold") {
        let floor = low_alpha_floor();
        for value in mask.iter_mut() {
            if *value < floor {
                *value = 0.0;
            }
        }
    }
    // 再清一轮孤岛：残留中与主体不连通的小碎块（线稿笔触、噪点）整块移除，
    // 与 advanced 管线共用同一面积尺度。
    if ab_postprocess_stage_enabled("components") {
        remove_small_foreground_components(&mut mask, w, h, advanced_min_component_area(w, h));
    }
    // 背景次级孤岛：面积小且远离主体的独立前景块（背景人物等）整块移除。
    if ab_postprocess_stage_enabled("islands") {
        remove_background_islands(&mut mask, w, h);
    }

    // 只压低半透明过渡带，完全不透明主体保持不变。gamma > 1 会收紧边缘，
    // gamma < 1 则扩展边缘；正式默认 1.0 是恒等映射。
    let alpha_gamma = alpha_gamma.clamp(0.25, 4.0);
    if (alpha_gamma - 1.0).abs() > f32::EPSILON {
        for value in mask.iter_mut() {
            *value = value.clamp(0.0, 1.0).powf(alpha_gamma);
        }
    }

    // 边缘去污染：把过渡带颜色从「前景+背景混合」解混回纯前景色。
    // 实测对发丝、皮肤边缘的粉色/蓝色 fringe 有明显改善，且深/浅背景图都稳健。
    let colors = decontaminate_colors(rgb, &mask);
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
/// 优先由 AnimeSeg 专精模型复核，再由 BiRefNet 通用模型兜底；只有复核结果
/// 确实更干净时才切换输出。
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

    let (mask, fallback_model) = match model_spec(model_id)?.kind {
        ModelKind::Simple => (run_simple(base, &rgb)?, model_id),
        ModelKind::Advanced => (run_advanced(base, &rgb)?, model_id),
        ModelKind::BiRefNet { matting } => {
            (run_birefnet(base, model_id, matting, &rgb)?, model_id)
        }
        ModelKind::Toonout => {
            let mask = run_birefnet(base, "toonout", false, &rgb)?;
            if toonout_likely_failed(&mask, w, h) {
                // 先释放 ToonOut 会话，避免两套大模型在显存中重叠。AnimeSeg 专精
                // 模型优先处理「主体与背景同为动漫线稿」的误保留，通用 BiRefNet
                // 仍保留为专精模型未安装或未通过客观清理门槛时的兜底。
                release_birefnet_session("toonout");
                let specialist_candidate = if is_model_ready(base, "anime-specialist") {
                    match run_birefnet(base, "anime-specialist", false, &rgb) {
                        Ok(candidate) if matte_is_substantially_cleaner(&candidate, &mask, w, h) => {
                            Some(candidate)
                        }
                        _ => None,
                    }
                } else {
                    None
                };
                if let Some(candidate) = specialist_candidate {
                    (candidate, "anime-specialist")
                } else {
                    // 专精候选没有接管时才能继续加载 General；否则两次 1024 推理
                    // 既无质量收益，也会在小显存显卡上制造不必要的峰值占用。
                    release_birefnet_session("anime-specialist");
                    let general_candidate = if is_model_ready(base, "birefnet-general") {
                        match run_birefnet(base, "birefnet-general", false, &rgb) {
                            Ok(candidate)
                                if matte_is_substantially_cleaner(&candidate, &mask, w, h) =>
                            {
                                Some(candidate)
                            }
                            _ => None,
                        }
                    } else {
                        None
                    };
                    if let Some(candidate) = general_candidate {
                        (candidate, "birefnet-general")
                    } else if is_model_ready(base, "advanced") {
                        // General 复核未接管时不保留它的会话，避免和后续两阶段
                        // 动漫模型重叠占用显存。
                        release_birefnet_session("birefnet-general");
                        match run_advanced(base, &rgb) {
                            Ok(candidate)
                                if matte_is_substantially_cleaner(&candidate, &mask, w, h) =>
                            {
                                (candidate, "advanced")
                            }
                            _ => (mask, "toonout"),
                        }
                    } else if is_model_ready(base, "simple") {
                        release_birefnet_session("birefnet-general");
                        (run_simple(base, &rgb)?, "simple")
                    } else {
                        release_birefnet_session("birefnet-general");
                        (mask, "toonout")
                    }
                }
            } else {
                (mask, "toonout")
            }
        }
    };

    let fallback = model_id == "toonout" && fallback_model != "toonout";

    // AB 回归可视化：引导滤波前的原始模型掩码，供滤波参数对比。
    if std::env::var_os("AIAS_AB_DEBUG").is_some() {
        let stem = output
            .file_stem()
            .and_then(|value| value.to_str())
            .unwrap_or("matte")
            .to_string();
        let mut raw_image = ImageBuffer::<Luma<u8>, Vec<u8>>::new(w, h);
        for (index, slot) in raw_image.pixels_mut().enumerate() {
            *slot = image::Luma([(mask[index] * 255.0).round().clamp(0.0, 255.0) as u8]);
        }
        let _ = raw_image.save(output.with_file_name(format!("{stem}_matte_raw.png")));
    }

    let result = finalize_cutout_image(
        &rgb,
        mask,
        model_uses_native_edge_alpha(fallback_model),
    );
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
        for (slot, pixel) in matte_image.pixels_mut().zip(result.pixels()) {
            *slot = image::Luma([pixel[3]]);
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
    // 复杂场景里 ToonOut 可能只漏留背景的一侧，前景占比未必达到旧阈值 0.67；
    // 边框高残留已是更强的异常信号，0.52 仍远高于常规居中单人图的背景占比。
    const FG_RATIO_MIN: f32 = 0.52;

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
    // 复核仅在两者都大幅降低边框残留、且收缩至少 4% 的前景时接管，避免把
    // ToonOut 的独立细节误换成更小的通用遮罩。
    border_improvement >= 0.10 && foreground_improvement >= 0.04
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
    fn background_islands_are_removed_only_when_small_and_far_from_the_subject() {
        // 44×8：左上 8×8 主体（64px，2% 阈值 = 面积 1 的块才可删）。
        let mut matte = vec![0.0; 44 * 8];
        let fill = |matte: &mut [f32], points: &[(usize, usize)]| {
            for (x, y) in points {
                matte[y * 44 + x] = 1.0;
            }
        };
        let main: Vec<(usize, usize)> = (0..8)
            .flat_map(|y| (0..8).map(move |x| (x, y)))
            .collect();
        fill(&mut matte, &main);
        fill(&mut matte, &[(42, 7)]); // 间距 34 > 32 且面积达标 → 移除
        fill(&mut matte, &[(10, 4)]); // 间距 2，紧贴 → 保留
        fill(&mut matte, &[(42, 0), (43, 0)]); // 间距 34 但面积超主件 2% → 保留

        remove_background_islands(&mut matte, 44, 8);

        assert_eq!(matte[7 * 44 + 42], 0.0, "远处单像素孤岛应被移除");
        assert_eq!(matte[4 * 44 + 10], 1.0, "紧贴主体的块不能被移除");
        assert_eq!(matte[44], 1.0, "面积超阈值的远处块不能被移除");
        for (x, y) in &main {
            assert_eq!(matte[y * 44 + x], 1.0, "主体不能被侵蚀");
        }
    }

    #[test]
    fn alpha_metrics_split_interior_boundary_and_exterior_errors() {
        let mut gt = RgbaImage::from_pixel(21, 21, image::Rgba([0, 0, 0, 0]));
        for y in 6..15 {
            for x in 6..15 {
                gt.put_pixel(x, y, image::Rgba([255, 255, 255, 255]));
            }
        }
        let mut result = gt.clone();
        result.get_pixel_mut(10, 10)[3] = 0; // 主体内部漏抠
        result.get_pixel_mut(6, 10)[3] = 0; // 轮廓带漏抠
        result.get_pixel_mut(0, 0)[3] = 255; // 远处背景残留

        let metrics = alpha_metrics(&gt, &result);

        assert!(metrics.interior_miss_mean > 0.0, "主体内部漏抠应被计入");
        assert!(metrics.boundary_mae > 0.0, "轮廓带漏抠应被计入");
        assert!(metrics.exterior_leak_mean > 0.0, "远处背景残留应被计入");
    }

    #[test]
    fn instance_gate_keeps_only_the_requested_chebyshev_radius() {
        let mut matte = vec![0.0; 7 * 7];
        matte[3 * 7 + 3] = 1.0;
        let gate = dilated_instance_gate(&matte, 7, 7, 2);

        assert!(gate[3 * 7 + 3], "实例中心必须保留");
        assert!(gate[1 * 7 + 1], "Chebyshev 距离 2 的像素必须保留");
        assert!(!gate[0], "Chebyshev 距离 3 的像素必须排除");
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
    fn toonout_complex_background_signal_also_catches_one_sided_leaks() {
        let (width, height) = (512, 512);
        let mut matte = vec![0.35; (width * height) as usize];
        // 前景约 53%，模拟背景只从一侧/四周漏进来而非整图误判。
        for y in 70..442 {
            for x in 70..442 {
                matte[(y * width + x) as usize] = 1.0;
            }
        }

        assert!(toonout_likely_failed(&matte, width, height));
    }

    #[test]
    fn cleaner_fallback_accepts_a_clear_but_not_excessive_foreground_reduction() {
        let (width, height) = (512, 512);
        let mut toonout = vec![0.35; (width * height) as usize];
        let mut general = vec![0.05; (width * height) as usize];
        for y in 64..448 {
            for x in 64..448 {
                toonout[(y * width + x) as usize] = 1.0;
            }
        }
        for y in 72..440 {
            for x in 72..440 {
                general[(y * width + x) as usize] = 1.0;
            }
        }

        assert!(matte_is_substantially_cleaner(&general, &toonout, width, height));
    }

    #[test]
    fn main_subject_score_prefers_a_large_centered_candidate_over_a_tiny_high_confidence_one() {
        let centered = CharacterCandidate {
            confidence: 0.86,
            stride: 8,
            row: 0,
            col: 0,
            x1: 180.0,
            y1: 120.0,
            x2: 820.0,
            y2: 880.0,
        };
        let edge = CharacterCandidate {
            confidence: 0.99,
            stride: 8,
            row: 0,
            col: 0,
            x1: 0.0,
            y1: 40.0,
            x2: 240.0,
            y2: 360.0,
        };

        assert!(
            main_subject_score(centered, 1000, 1000) > main_subject_score(edge, 1000, 1000),
            "主体排序应让大且居中的候选胜过边缘小候选"
        );
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

    #[test]
    fn general_catalog_is_the_verified_1024_profile() {
        let general = model_spec("birefnet-general").expect("General 模型必须已注册");
        let file = general.files.first().expect("General 必须有模型文件");

        assert_eq!(file.name, "birefnet-general-1024-fp16.onnx");
        assert_eq!(file.size, BIREFNET_GENERAL_1024_FP16_SIZE);
        assert!(model_uses_native_edge_alpha(general.id));
        assert!(!model_uses_native_edge_alpha("toonout"));
    }

    #[test]
    fn anime_specialist_catalog_is_the_verified_animeseg_profile() {
        let specialist = model_spec("anime-specialist").expect("AnimeSeg 模型必须已注册");
        let file = specialist.files.first().expect("AnimeSeg 必须有模型文件");

        assert_eq!(file.name, "birefnext-aniseg-int8-v0.1.onnx");
        assert_eq!(file.size, ANIME_SPECIALIST_SIZE);
        assert!(model_uses_native_edge_alpha(specialist.id));
        assert!(matches!(specialist.kind, ModelKind::BiRefNet { matting: false }));
    }

    #[test]
    fn horizontal_flip_mean_restores_the_original_pixel_order() {
        let direct = [1.0, 0.0, 0.2, 0.8];
        // flipped[0..2] 属于原图第二列，flipped[2..4] 属于原图第一列。
        let flipped = [0.4, 0.6, 0.7, 0.3];
        let combined = mean_with_horizontal_flip(&direct, &flipped, 2, 2);

        assert_eq!(combined, vec![0.8, 0.2, 0.25, 0.75]);
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

    /// A/B 基准：对 AIAS_AB_INPUT（图像或目录）跑指定模型；若同时给出
    /// AIAS_AB_GT，则以其 alpha 作为人工真值，输出可复核的指标和五联预览。
    /// 例如：
    /// `AIAS_AB_INPUT=F:\\...\\原图.png AIAS_AB_GT=F:\\...\\人工抠图版.png \
    ///   AIAS_AB_OUTPUT=F:\\...\\AB测试结果 AIAS_AB_TAG=P5_baseline \
    ///   cargo test --release anime::toonout_tests::ab_reference -- --ignored --exact --nocapture`
    /// 可用 `AIAS_AB_MODELS=advanced` 限定单模型，避免 A/B 验证被其他模型的
    /// 显存需求阻断；默认仍依次运行 toonout、simple、advanced。
    #[test]
    #[ignore = "手动执行：真实模型推理"]
    fn ab_reference() {
        let base = dirs::home_dir()
            .expect("无法定位用户目录")
            .join(r"AppData\Roaming\studio.avroracl.aias");
        let input_path = std::env::var_os("AIAS_AB_INPUT")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(r"F:\战争雷霆涂装\贴图素材\F15E 塞雷娅\测试"));
        let gt_path = std::env::var_os("AIAS_AB_GT")
            .map(PathBuf::from)
            .filter(|path| path.is_file());
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
        let out_dir = std::env::var_os("AIAS_AB_OUTPUT")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(r"F:\AIAS\ab").join(&tag));
        fs::create_dir_all(&out_dir).unwrap();
        ensure_ort_runtime(&base).unwrap();

        let mut inputs = if input_path.is_file() {
            vec![input_path]
        } else {
            fs::read_dir(&input_path)
                .unwrap()
                .flatten()
                .map(|entry| entry.path())
                .collect::<Vec<_>>()
        };
        inputs.sort();
        let gt_alpha = gt_path.as_ref().map(|path| {
            image::open(path)
                .unwrap_or_else(|error| panic!("无法读取人工抠图真值 {}：{error}", path.display()))
                .to_rgba8()
        });
        let mut report = String::from(
            "AIAS alpha A/B report\n"
        );
        report.push_str(&format!("tag={tag}\n"));
        if let Some(path) = &gt_path {
            report.push_str(&format!("ground_truth={}\n", path.display()));
        }

        for path in inputs {
            let ext = path
                .extension()
                .and_then(|value| value.to_str())
                .unwrap_or("")
                .to_ascii_lowercase();
            if !matches!(ext.as_str(), "png" | "jpg" | "jpeg" | "webp") {
                continue;
            }
            if gt_path.as_ref().is_some_and(|gt| gt == &path) {
                continue;
            }
            let stem = path
                .file_stem()
                .and_then(|value| value.to_str())
                .unwrap_or("image")
                .to_string();
            let source = gt_alpha.as_ref().map(|_| {
                image::open(&path)
                    .unwrap_or_else(|error| panic!("无法读取原图 {}：{error}", path.display()))
                    .to_rgb8()
            });
            if let (Some(gt), Some(source)) = (&gt_alpha, &source) {
                assert_eq!(
                    gt.dimensions(),
                    source.dimensions(),
                    "人工真值与原图尺寸必须一致：{}",
                    path.display()
                );
                let alignment = source_gt_alignment(source, gt);
                let alignment_line = format!(
                    "{stem}\talignment\tsolid_pixels={}\trgb_mae={:.4}\trgb_changed_ratio={:.5}\n",
                    alignment.solid_pixels,
                    alignment.rgb_mae,
                    alignment.rgb_changed_ratio,
                );
                print!("{alignment_line}");
                report.push_str(&alignment_line);
            }
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
                if let Some(gt) = &gt_alpha {
                    assert_eq!(
                        gt.dimensions(),
                        result.dimensions(),
                        "人工真值与输入尺寸必须一致：{}",
                        path.display()
                    );
                    let metrics = alpha_metrics(gt, &result);
                    let line = format!(
                        "{stem}\t{model}\tactual={}\tmae={:.5}\tbg_leak_mean={:.5}\tbg_leak_ratio={:.5}\tfg_miss_mean={:.5}\tfg_miss_ratio={:.5}\tedge_mae={:.5}\tiou={:.5}\n",
                        outcome.model_used,
                        metrics.mae,
                        metrics.background_leak_mean,
                        metrics.background_leak_ratio,
                        metrics.foreground_miss_mean,
                        metrics.foreground_miss_ratio,
                        metrics.edge_mae,
                        metrics.iou,
                    );
                    print!("{line}");
                    report.push_str(&line);
                    let region_line = format!(
                        "{stem}\t{model}\tregions\tinterior_miss_mean={:.5}\tboundary_mae={:.5}\texterior_leak_mean={:.5}\n",
                        metrics.interior_miss_mean,
                        metrics.boundary_mae,
                        metrics.exterior_leak_mean,
                    );
                    print!("{region_line}");
                    report.push_str(&region_line);
                    if let Some(source) = &source {
                        let heatmap = out_dir.join(format!("{stem}_{model}_alpha_error.png"));
                        alpha_error_heatmap(source, gt, &result, &heatmap);
                    }
                    if std::env::var_os("AIAS_AB_DEBUG").is_some() {
                        let raw_path = output.with_file_name(format!("{stem}_{model}_matte_raw.png"));
                        if raw_path.is_file() {
                            let raw = image::open(&raw_path)
                                .expect("原始遮罩可读")
                                .to_luma8();
                            let raw_metrics = alpha_metrics_luma(gt, &raw);
                            let raw_line = format!(
                                "{stem}\t{model}\tstage=raw\tmae={:.5}\tbg_leak_mean={:.5}\tbg_leak_ratio={:.5}\tfg_miss_mean={:.5}\tfg_miss_ratio={:.5}\tedge_mae={:.5}\tiou={:.5}\tinterior_miss_mean={:.5}\tboundary_mae={:.5}\texterior_leak_mean={:.5}\n",
                                raw_metrics.mae,
                                raw_metrics.background_leak_mean,
                                raw_metrics.background_leak_ratio,
                                raw_metrics.foreground_miss_mean,
                                raw_metrics.foreground_miss_ratio,
                                raw_metrics.edge_mae,
                                raw_metrics.iou,
                                raw_metrics.interior_miss_mean,
                                raw_metrics.boundary_mae,
                                raw_metrics.exterior_leak_mean,
                            );
                            print!("{raw_line}");
                            report.push_str(&raw_line);
                        }
                    }
                    ab_gt_preview(&path, gt, &result, &preview);
                } else {
                    ab_preview(&path, &result, &preview);
                }
            }
        }
        fs::write(out_dir.join(format!("{tag}_metrics.txt")), report)
            .expect("A/B 指标报告写出");
    }

    /// 开发期翻转增强对照：同一 General 1024 分别推理原图与水平翻转图，将后者
    /// 翻回后做均值融合。该方法不接入正式流程，先确认双倍推理时间是否换来
    /// 可量化的边缘收益，避免凭直觉牺牲批处理速度。
    #[test]
    #[ignore = "手动执行：General 1024 翻转增强对照"]
    fn ab_general_1024_flip_tta() {
        let base = dirs::home_dir()
            .expect("无法定位用户目录")
            .join(r"AppData\Roaming\studio.avroracl.aias");
        let input = std::env::var_os("AIAS_AB_INPUT")
            .map(PathBuf::from)
            .filter(|path| path.is_file())
            .expect("AIAS_AB_INPUT 必须指向一张原图");
        let gt_path = std::env::var_os("AIAS_AB_GT")
            .map(PathBuf::from)
            .filter(|path| path.is_file())
            .expect("AIAS_AB_GT 必须指向人工 alpha 真值");
        let out_dir = std::env::var_os("AIAS_AB_OUTPUT")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(r"F:\AIAS\ab\flip-tta"));
        let tag = std::env::var("AIAS_AB_TAG").unwrap_or_else(|_| "flip-tta".into());
        fs::create_dir_all(&out_dir).expect("创建翻转增强输出目录");
        assert!(is_model_ready(&base, "birefnet-general"), "测试需要 General 1024 模型");

        let original = image::open(&input).expect("原图可读").to_rgb8();
        let gt = image::open(&gt_path).expect("人工真值可读").to_rgba8();
        assert_eq!(original.dimensions(), gt.dimensions(), "原图与 GT 尺寸必须一致");
        let (w, h) = original.dimensions();
        let stem = input.file_stem().and_then(|value| value.to_str()).unwrap_or("image");

        let started = std::time::Instant::now();
        let direct_mask = run_birefnet_single_for_test(&base, "birefnet-general", false, &original)
            .expect("原图 General 1024 推理");
        let flipped = image::imageops::flip_horizontal(&original);
        let flipped_mask = run_birefnet_single_for_test(&base, "birefnet-general", false, &flipped)
            .expect("翻转图 General 1024 推理");
        let (width, height) = (w as usize, h as usize);
        let mut restored_flipped_mask = vec![0.0; direct_mask.len()];
        for y in 0..height {
            for x in 0..width {
                restored_flipped_mask[y * width + x] = flipped_mask[y * width + (width - 1 - x)];
            }
        }
        let blend = |flipped_weight: f32| {
            direct_mask
                .iter()
                .zip(restored_flipped_mask.iter())
                .map(|(direct, mirrored)| direct * (1.0 - flipped_weight) + mirrored * flipped_weight)
                .collect::<Vec<f32>>()
        };
        let tta_mask = mean_with_horizontal_flip(&direct_mask, &flipped_mask, w, h);
        let flip_quarter_mask = blend(0.25);
        let flip_sixty_mask = blend(0.60);
        let flip_three_quarter_mask = blend(0.75);
        let flip_ninety_mask = blend(0.90);
        let flip_only_mask = restored_flipped_mask;
        println!("{tag} flip TTA inference elapsed {:?}", started.elapsed());

        let direct = finalize_cutout_image(&original, direct_mask, true);
        let tta = finalize_cutout_image(&original, tta_mask, true);
        let flip_quarter = finalize_cutout_image(&original, flip_quarter_mask, true);
        let flip_sixty = finalize_cutout_image(&original, flip_sixty_mask, true);
        let flip_three_quarter = finalize_cutout_image(&original, flip_three_quarter_mask, true);
        let flip_ninety = finalize_cutout_image(&original, flip_ninety_mask, true);
        let flip_only = finalize_cutout_image(&original, flip_only_mask, true);
        let direct_label = "birefnet-general-1024-direct";
        let flip_quarter_label = "birefnet-general-1024-flip-025";
        let tta_label = "birefnet-general-1024-flip-mean";
        let flip_sixty_label = "birefnet-general-1024-flip-060";
        let flip_three_quarter_label = "birefnet-general-1024-flip-075";
        let flip_ninety_label = "birefnet-general-1024-flip-090";
        let flip_only_label = "birefnet-general-1024-flip-only";
        for (label, result) in [
            (direct_label, &direct),
            (flip_quarter_label, &flip_quarter),
            (tta_label, &tta),
            (flip_sixty_label, &flip_sixty),
            (flip_three_quarter_label, &flip_three_quarter),
            (flip_ninety_label, &flip_ninety),
            (flip_only_label, &flip_only),
        ] {
            result
                .save(out_dir.join(format!("{stem}_{label}.png")))
                .expect("翻转增强结果写出");
            ab_gt_preview(
                &input,
                &gt,
                result,
                &out_dir.join(format!("{stem}_{label}_preview.jpg")),
            );
            alpha_error_heatmap(
                &original,
                &gt,
                result,
                &out_dir.join(format!("{stem}_{label}_alpha_error.png")),
            );
        }
        let mut report = format!(
            "AIAS General 1024 flip TTA A/B report\ntag={tag}\ninput={}\nground_truth={}\nstrategy=direct vs 25% / 50% / 60% / 75% / 90% / 100% horizontal-flip alpha blend; development only\n",
            input.display(),
            gt_path.display(),
        );
        append_instance_metrics(&mut report, direct_label, &gt, &direct);
        append_instance_metrics(&mut report, flip_quarter_label, &gt, &flip_quarter);
        append_instance_metrics(&mut report, tta_label, &gt, &tta);
        append_instance_metrics(&mut report, flip_sixty_label, &gt, &flip_sixty);
        append_instance_metrics(&mut report, flip_three_quarter_label, &gt, &flip_three_quarter);
        append_instance_metrics(&mut report, flip_ninety_label, &gt, &flip_ninety);
        append_instance_metrics(&mut report, flip_only_label, &gt, &flip_only);
        fs::write(out_dir.join(format!("{tag}_metrics.txt")), report)
            .expect("翻转增强指标报告写出");
    }

    /// 开发期方向增强复核：在已经验证有效的左右翻转之外，额外测试上下翻转和
    /// 四向均值。动漫人物通常具有明显的竖直朝向，上下翻转可能反而破坏语义；
    /// 因此这里只产生 A/B 证据，绝不在未胜出的情况下增加正式处理耗时。
    #[test]
    #[ignore = "手动执行：General 1024 方向增强对照"]
    fn ab_general_1024_orientation_tta() {
        let base = dirs::home_dir()
            .expect("无法定位用户目录")
            .join(r"AppData\Roaming\studio.avroracl.aias");
        let input = std::env::var_os("AIAS_AB_INPUT")
            .map(PathBuf::from)
            .filter(|path| path.is_file())
            .expect("AIAS_AB_INPUT 必须指向一张原图");
        let gt_path = std::env::var_os("AIAS_AB_GT")
            .map(PathBuf::from)
            .filter(|path| path.is_file())
            .expect("AIAS_AB_GT 必须指向人工 alpha 真值");
        let out_dir = std::env::var_os("AIAS_AB_OUTPUT")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(r"F:\AIAS\ab\orientation-tta"));
        let tag = std::env::var("AIAS_AB_TAG").unwrap_or_else(|_| "orientation-tta".into());
        fs::create_dir_all(&out_dir).expect("创建方向增强输出目录");
        assert!(is_model_ready(&base, "birefnet-general"), "测试需要 General 1024 模型");

        let original = image::open(&input).expect("原图可读").to_rgb8();
        let gt = image::open(&gt_path).expect("人工真值可读").to_rgba8();
        assert_eq!(original.dimensions(), gt.dimensions(), "原图与 GT 尺寸必须一致");
        let (w, h) = original.dimensions();
        let stem = input.file_stem().and_then(|value| value.to_str()).unwrap_or("image");
        let restore = |mask: &[f32], flip_x: bool, flip_y: bool| {
            let (width, height) = (w as usize, h as usize);
            assert_eq!(mask.len(), width * height, "翻转 alpha 尺寸无效");
            let mut restored = vec![0.0; mask.len()];
            for y in 0..height {
                for x in 0..width {
                    let source_x = if flip_x { width - 1 - x } else { x };
                    let source_y = if flip_y { height - 1 - y } else { y };
                    restored[y * width + x] = mask[source_y * width + source_x];
                }
            }
            restored
        };
        let average = |masks: &[&[f32]]| {
            assert!(!masks.is_empty(), "至少需要一张 alpha");
            let mut merged = vec![0.0; masks[0].len()];
            for mask in masks {
                assert_eq!(mask.len(), merged.len(), "alpha 尺寸必须一致");
                for (target, value) in merged.iter_mut().zip(mask.iter()) {
                    *target += value;
                }
            }
            let divisor = masks.len() as f32;
            for value in &mut merged {
                *value /= divisor;
            }
            merged
        };

        let started = std::time::Instant::now();
        let direct_mask = run_birefnet_single_for_test(&base, "birefnet-general", false, &original)
            .expect("原图 General 1024 推理");
        let horizontal_mask = restore(
            &run_birefnet_single_for_test(
                &base,
                "birefnet-general",
                false,
                &image::imageops::flip_horizontal(&original),
            )
            .expect("左右翻转 General 1024 推理"),
            true,
            false,
        );
        let vertical_mask = restore(
            &run_birefnet_single_for_test(
                &base,
                "birefnet-general",
                false,
                &image::imageops::flip_vertical(&original),
            )
            .expect("上下翻转 General 1024 推理"),
            false,
            true,
        );
        let both_mask = restore(
            &run_birefnet_single_for_test(
                &base,
                "birefnet-general",
                false,
                &image::imageops::flip_vertical(&image::imageops::flip_horizontal(&original)),
            )
            .expect("双向翻转 General 1024 推理"),
            true,
            true,
        );
        println!("{tag} orientation TTA inference elapsed {:?}", started.elapsed());

        // 先完成所有融合，随后才把 direct_mask 移交给正式后处理。
        let horizontal_mean = average(&[&direct_mask, &horizontal_mask]);
        let vertical_mean = average(&[&direct_mask, &vertical_mask]);
        let four_way_mean = average(&[&direct_mask, &horizontal_mask, &vertical_mask, &both_mask]);
        let direct = finalize_cutout_image(&original, direct_mask, true);
        let horizontal = finalize_cutout_image(&original, horizontal_mean, true);
        let vertical = finalize_cutout_image(&original, vertical_mean, true);
        let four_way = finalize_cutout_image(&original, four_way_mean, true);
        let results = [
            ("birefnet-general-1024-direct", &direct),
            ("birefnet-general-1024-horizontal-mean", &horizontal),
            ("birefnet-general-1024-vertical-mean", &vertical),
            ("birefnet-general-1024-four-way-mean", &four_way),
        ];
        let mut report = format!(
            "AIAS General 1024 orientation TTA A/B report\ntag={tag}\ninput={}\nground_truth={}\nstrategy=direct vs horizontal vs vertical vs four-way mean; development only\n",
            input.display(),
            gt_path.display(),
        );
        for (label, result) in results {
            result
                .save(out_dir.join(format!("{stem}_{label}.png")))
                .expect("方向增强结果写出");
            ab_gt_preview(
                &input,
                &gt,
                result,
                &out_dir.join(format!("{stem}_{label}_preview.jpg")),
            );
            alpha_error_heatmap(
                &original,
                &gt,
                result,
                &out_dir.join(format!("{stem}_{label}_alpha_error.png")),
            );
            append_instance_metrics(&mut report, label, &gt, result);
        }
        fs::write(out_dir.join(format!("{tag}_metrics.txt")), report)
            .expect("方向增强指标报告写出");
    }

    /// 开发期边缘收缩对照：人工真值显示当前残差主要落在外轮廓，故以已经胜出
    /// 的水平翻转 alpha 为共同输入，比较温和的 alpha 曲线和激进的 1px 最小值
    /// 腐蚀。没有跨图/真值回归支持时，任何一种都不得进入正式链路。
    #[test]
    #[ignore = "手动执行：General 1024 边缘收缩对照"]
    fn ab_general_1024_edge_contract() {
        let base = dirs::home_dir()
            .expect("无法定位用户目录")
            .join(r"AppData\Roaming\studio.avroracl.aias");
        let input = std::env::var_os("AIAS_AB_INPUT")
            .map(PathBuf::from)
            .filter(|path| path.is_file())
            .expect("AIAS_AB_INPUT 必须指向一张原图");
        let gt_path = std::env::var_os("AIAS_AB_GT")
            .map(PathBuf::from)
            .filter(|path| path.is_file())
            .expect("AIAS_AB_GT 必须指向人工 alpha 真值");
        let out_dir = std::env::var_os("AIAS_AB_OUTPUT")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(r"F:\AIAS\ab\edge-contract"));
        let tag = std::env::var("AIAS_AB_TAG").unwrap_or_else(|_| "edge-contract".into());
        fs::create_dir_all(&out_dir).expect("创建边缘收缩输出目录");
        assert!(is_model_ready(&base, "birefnet-general"), "测试需要 General 1024 模型");

        let original = image::open(&input).expect("原图可读").to_rgb8();
        let gt = image::open(&gt_path).expect("人工真值可读").to_rgba8();
        assert_eq!(original.dimensions(), gt.dimensions(), "原图与 GT 尺寸必须一致");
        let (w, h) = original.dimensions();
        let stem = input.file_stem().and_then(|value| value.to_str()).unwrap_or("image");

        let started = std::time::Instant::now();
        let direct_mask = run_birefnet_single_for_test(&base, "birefnet-general", false, &original)
            .expect("原图 General 1024 推理");
        let flipped_mask = run_birefnet_single_for_test(
            &base,
            "birefnet-general",
            false,
            &image::imageops::flip_horizontal(&original),
        )
        .expect("翻转图 General 1024 推理");
        let tta_mask = mean_with_horizontal_flip(&direct_mask, &flipped_mask, w, h);
        println!("{tag} edge-contract inference elapsed {:?}", started.elapsed());

        let (width, height) = (w as usize, h as usize);
        let mut min3_mask = vec![0.0; tta_mask.len()];
        for y in 0..height {
            for x in 0..width {
                let mut minimum = 1.0f32;
                for yy in y.saturating_sub(1)..=(y + 1).min(height - 1) {
                    for xx in x.saturating_sub(1)..=(x + 1).min(width - 1) {
                        minimum = minimum.min(tta_mask[yy * width + xx]);
                    }
                }
                min3_mask[y * width + x] = minimum;
            }
        }

        let baseline = finalize_cutout_image_with_alpha_gamma(&original, tta_mask.clone(), true, 1.0);
        let gamma_110 = finalize_cutout_image_with_alpha_gamma(&original, tta_mask.clone(), true, 1.10);
        let gamma_125 = finalize_cutout_image_with_alpha_gamma(&original, tta_mask, true, 1.25);
        let min3 = finalize_cutout_image_with_alpha_gamma(&original, min3_mask, true, 1.0);
        // `finalize_cutout_image` 的正式阈值发生在同一后处理阶段；这里在最终
        // alpha 上模拟更高阈值，先快速测出主角完整度和背景幽灵的取舍，再决定
        // 是否值得调整正式阈值。后续组件清理按 >0.5 工作，对这些候选无额外影响。
        let raise_alpha_floor = |source: &RgbaImage, floor: f32| {
            let mut result = source.clone();
            for pixel in result.pixels_mut() {
                if pixel[3] as f32 / 255.0 < floor {
                    pixel[3] = 0;
                }
            }
            result
        };
        let floor_020 = raise_alpha_floor(&baseline, 0.20);
        let floor_030 = raise_alpha_floor(&baseline, 0.30);
        let floor_040 = raise_alpha_floor(&baseline, 0.40);
        let floor_050 = raise_alpha_floor(&baseline, 0.50);
        let results = [
            ("birefnet-general-1024-horizontal-baseline", &baseline),
            ("birefnet-general-1024-horizontal-gamma-110", &gamma_110),
            ("birefnet-general-1024-horizontal-gamma-125", &gamma_125),
            ("birefnet-general-1024-horizontal-min3", &min3),
            ("birefnet-general-1024-horizontal-floor-020", &floor_020),
            ("birefnet-general-1024-horizontal-floor-030", &floor_030),
            ("birefnet-general-1024-horizontal-floor-040", &floor_040),
            ("birefnet-general-1024-horizontal-floor-050", &floor_050),
        ];
        let mut report = format!(
            "AIAS General 1024 edge contract A/B report\ntag={tag}\ninput={}\nground_truth={}\nstrategy=horizontal TTA baseline vs alpha gamma vs 1px min erosion vs alpha floors; development only\n",
            input.display(),
            gt_path.display(),
        );
        for (label, result) in results {
            result
                .save(out_dir.join(format!("{stem}_{label}.png")))
                .expect("边缘收缩结果写出");
            ab_gt_preview(
                &input,
                &gt,
                result,
                &out_dir.join(format!("{stem}_{label}_preview.jpg")),
            );
            alpha_error_heatmap(
                &original,
                &gt,
                result,
                &out_dir.join(format!("{stem}_{label}_alpha_error.png")),
            );
            append_instance_metrics(&mut report, label, &gt, result);
        }
        fs::write(out_dir.join(format!("{tag}_metrics.txt")), report)
            .expect("边缘收缩指标报告写出");
    }

    /// 对已胜出的 AnimeSeg 专精模型单独校准轮廓。General 的边缘收缩结论不能
    /// 外推给它：两者的残差形态不同，必须在修正后的人工真值上独立比较。
    #[test]
    #[ignore = "手动执行：AnimeSeg 边缘收缩对照"]
    fn ab_anime_specialist_edge_contract() {
        let base = dirs::home_dir()
            .expect("无法定位用户目录")
            .join(r"AppData\Roaming\studio.avroracl.aias");
        let input = std::env::var_os("AIAS_AB_INPUT")
            .map(PathBuf::from)
            .filter(|path| path.is_file())
            .expect("AIAS_AB_INPUT 必须指向一张原图");
        let gt_path = std::env::var_os("AIAS_AB_GT")
            .map(PathBuf::from)
            .filter(|path| path.is_file())
            .expect("AIAS_AB_GT 必须指向人工 alpha 真值");
        let out_dir = std::env::var_os("AIAS_AB_OUTPUT")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(r"F:\AIAS\ab\anime-specialist-edge-contract"));
        let tag = std::env::var("AIAS_AB_TAG")
            .unwrap_or_else(|_| "anime-specialist-edge-contract".into());
        fs::create_dir_all(&out_dir).expect("创建边缘收缩输出目录");
        assert!(
            is_model_ready(&base, "anime-specialist"),
            "测试需要 AnimeSeg 专精模型"
        );

        let original = image::open(&input).expect("原图可读").to_rgb8();
        let gt = image::open(&gt_path).expect("人工真值可读").to_rgba8();
        assert_eq!(original.dimensions(), gt.dimensions(), "原图与 GT 尺寸必须一致");
        let (w, h) = original.dimensions();
        let stem = input.file_stem().and_then(|value| value.to_str()).unwrap_or("image");

        let started = std::time::Instant::now();
        let mask = run_birefnet(&base, "anime-specialist", false, &original)
            .expect("AnimeSeg 专精模型推理");
        println!("{tag} edge-contract inference elapsed {:?}", started.elapsed());

        let (width, height) = (w as usize, h as usize);
        let mut min3_mask = vec![0.0; mask.len()];
        for y in 0..height {
            for x in 0..width {
                let mut minimum = 1.0f32;
                for yy in y.saturating_sub(1)..=(y + 1).min(height - 1) {
                    for xx in x.saturating_sub(1)..=(x + 1).min(width - 1) {
                        minimum = minimum.min(mask[yy * width + xx]);
                    }
                }
                min3_mask[y * width + x] = minimum;
            }
        }

        let baseline = finalize_cutout_image_with_alpha_gamma(&original, mask.clone(), true, 1.0);
        let gamma_102 = finalize_cutout_image_with_alpha_gamma(&original, mask.clone(), true, 1.02);
        let gamma_105 = finalize_cutout_image_with_alpha_gamma(&original, mask.clone(), true, 1.05);
        let gamma_110 = finalize_cutout_image_with_alpha_gamma(&original, mask, true, 1.10);
        let min3 = finalize_cutout_image_with_alpha_gamma(&original, min3_mask, true, 1.0);
        let raise_alpha_floor = |source: &RgbaImage, floor: f32| {
            let mut result = source.clone();
            for pixel in result.pixels_mut() {
                if pixel[3] as f32 / 255.0 < floor {
                    pixel[3] = 0;
                }
            }
            result
        };
        let floor_010 = raise_alpha_floor(&baseline, 0.10);
        let floor_020 = raise_alpha_floor(&baseline, 0.20);
        let results = [
            ("anime-specialist-baseline", &baseline),
            ("anime-specialist-gamma-102", &gamma_102),
            ("anime-specialist-gamma-105", &gamma_105),
            ("anime-specialist-gamma-110", &gamma_110),
            ("anime-specialist-min3", &min3),
            ("anime-specialist-floor-010", &floor_010),
            ("anime-specialist-floor-020", &floor_020),
        ];
        let mut report = format!(
            "AIAS AnimeSeg edge contract A/B report\ntag={tag}\ninput={}\nground_truth={}\nstrategy=single-pass baseline vs gentle alpha gamma vs 1px min erosion vs alpha floors; development only\n",
            input.display(),
            gt_path.display(),
        );
        for (label, result) in results {
            result
                .save(out_dir.join(format!("{stem}_{label}.png")))
                .expect("边缘收缩结果写出");
            ab_gt_preview(
                &input,
                &gt,
                result,
                &out_dir.join(format!("{stem}_{label}_preview.jpg")),
            );
            alpha_error_heatmap(
                &original,
                &gt,
                result,
                &out_dir.join(format!("{stem}_{label}_alpha_error.png")),
            );
            append_instance_metrics(&mut report, label, &gt, result);
        }
        fs::write(out_dir.join(format!("{tag}_metrics.txt")), report)
            .expect("边缘收缩指标报告写出");
    }

    /// 开发期高分辨率复核：全图 1024 擅长理解「哪个才是主体」，重叠分块则能
    /// 以更高的有效像素密度看轮廓。这里故意同时输出纯分块和 50% 融合版；
    /// 若分块带来背景误检，指标会直接暴露，不能仅凭局部发丝看起来更锐利就上线。
    #[test]
    #[ignore = "手动执行：General 1024 重叠分块对照"]
    fn ab_general_1024_tiled_refine() {
        let base = dirs::home_dir()
            .expect("无法定位用户目录")
            .join(r"AppData\Roaming\studio.avroracl.aias");
        let input = std::env::var_os("AIAS_AB_INPUT")
            .map(PathBuf::from)
            .filter(|path| path.is_file())
            .expect("AIAS_AB_INPUT 必须指向一张原图");
        let gt_path = std::env::var_os("AIAS_AB_GT")
            .map(PathBuf::from)
            .filter(|path| path.is_file())
            .expect("AIAS_AB_GT 必须指向人工 alpha 真值");
        let out_dir = std::env::var_os("AIAS_AB_OUTPUT")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(r"F:\AIAS\ab\tiled-refine"));
        let tag = std::env::var("AIAS_AB_TAG").unwrap_or_else(|_| "tiled-refine".into());
        fs::create_dir_all(&out_dir).expect("创建分块复核输出目录");
        assert!(is_model_ready(&base, "birefnet-general"), "测试需要 General 1024 模型");

        let original = image::open(&input).expect("原图可读").to_rgb8();
        let gt = image::open(&gt_path).expect("人工真值可读").to_rgba8();
        assert_eq!(original.dimensions(), gt.dimensions(), "原图与 GT 尺寸必须一致");
        let (w, h) = original.dimensions();
        let stem = input.file_stem().and_then(|value| value.to_str()).unwrap_or("image");
        let starts = |length: u32, side: u32, stride: u32| {
            if length <= side {
                return vec![0];
            }
            let mut positions = vec![0];
            let last = length - side;
            while *positions.last().expect("至少一个分块起点") + stride < last {
                positions.push(*positions.last().expect("分块起点") + stride);
            }
            if *positions.last().expect("分块起点") != last {
                positions.push(last);
            }
            positions
        };

        // 2048px 原图块送入固定 1024 输入，提供约 2 倍于全图的局部有效密度；
        // 512px 重叠配合 256px 线性羽化，避免直接拼接出现接缝。
        const TILE: u32 = 2048;
        const STRIDE: u32 = 1536;
        const FEATHER: u32 = 256;
        let xs = starts(w, TILE, STRIDE);
        let ys = starts(h, TILE, STRIDE);
        let started = std::time::Instant::now();
        let direct_mask = run_birefnet_single_for_test(&base, "birefnet-general", false, &original)
            .expect("原图 General 1024 推理");
        let flipped_mask = run_birefnet_single_for_test(
            &base,
            "birefnet-general",
            false,
            &image::imageops::flip_horizontal(&original),
        )
        .expect("翻转图 General 1024 推理");
        let global_mask = mean_with_horizontal_flip(&direct_mask, &flipped_mask, w, h);

        let mut tiled_sum = vec![0.0f32; (w * h) as usize];
        let mut tiled_weight = vec![0.0f32; (w * h) as usize];
        for &top in &ys {
            for &left in &xs {
                let tile_w = TILE.min(w - left);
                let tile_h = TILE.min(h - top);
                let tile = image::imageops::crop_imm(&original, left, top, tile_w, tile_h).to_image();
                let tile_mask = run_birefnet_single_for_test(&base, "birefnet-general", false, &tile)
                    .expect("分块 General 1024 推理");
                for y in 0..tile_h {
                    let fy = if top > 0 && y < FEATHER {
                        y as f32 / FEATHER as f32
                    } else if top + tile_h < h && tile_h - 1 - y < FEATHER {
                        (tile_h - 1 - y) as f32 / FEATHER as f32
                    } else {
                        1.0
                    };
                    for x in 0..tile_w {
                        let fx = if left > 0 && x < FEATHER {
                            x as f32 / FEATHER as f32
                        } else if left + tile_w < w && tile_w - 1 - x < FEATHER {
                            (tile_w - 1 - x) as f32 / FEATHER as f32
                        } else {
                            1.0
                        };
                        let index = ((top + y) * w + left + x) as usize;
                        let weight = fx * fy;
                        tiled_sum[index] += tile_mask[(y * tile_w + x) as usize] * weight;
                        tiled_weight[index] += weight;
                    }
                }
            }
        }
        let tiled_mask: Vec<f32> = tiled_sum
            .iter()
            .zip(tiled_weight.iter())
            .map(|(sum, weight)| if *weight > 0.0 { sum / weight } else { 0.0 })
            .collect();
        println!(
            "{tag} tiled refine inference elapsed {:?}; grid={}x{}",
            started.elapsed(),
            xs.len(),
            ys.len()
        );

        let blended_mask: Vec<f32> = global_mask
            .iter()
            .zip(tiled_mask.iter())
            .map(|(global, tiled)| (global + tiled) * 0.5)
            .collect();
        let global = finalize_cutout_image(&original, global_mask, true);
        let tiled = finalize_cutout_image(&original, tiled_mask, true);
        let blended = finalize_cutout_image(&original, blended_mask, true);
        let results = [
            ("birefnet-general-1024-horizontal-global", &global),
            ("birefnet-general-1024-tiled", &tiled),
            ("birefnet-general-1024-global-tiled-mean", &blended),
        ];
        let mut report = format!(
            "AIAS General 1024 tiled refine A/B report\ntag={tag}\ninput={}\nground_truth={}\nstrategy=global horizontal TTA vs 2048px overlapping tiles vs 50% alpha mean; development only\n",
            input.display(),
            gt_path.display(),
        );
        for (label, result) in results {
            result
                .save(out_dir.join(format!("{stem}_{label}.png")))
                .expect("分块复核结果写出");
            ab_gt_preview(
                &input,
                &gt,
                result,
                &out_dir.join(format!("{stem}_{label}_preview.jpg")),
            );
            alpha_error_heatmap(
                &original,
                &gt,
                result,
                &out_dir.join(format!("{stem}_{label}_alpha_error.png")),
            );
            append_instance_metrics(&mut report, label, &gt, result);
        }
        fs::write(out_dir.join(format!("{tag}_metrics.txt")), report)
            .expect("分块复核指标报告写出");
    }

    /// 开发期本地官方导出验证：不注册、不下载、不暴露到正式 UI。将同一张原图
    /// 分别跑当前正式 general 完整管线和本机 HR 导出的原始 alpha，确认 HR 的
    /// 实际输入尺寸、显存可行性与遮罩质量后，才决定是否值得进入后续正式实验。
    #[test]
    #[ignore = "手动执行：本地 HR BiRefNet 对照"]
    fn ab_local_birefnet_reference() {
        let base = dirs::home_dir()
            .expect("无法定位用户目录")
            .join(r"AppData\Roaming\studio.avroracl.aias");
        let input = std::env::var_os("AIAS_AB_INPUT")
            .map(PathBuf::from)
            .filter(|path| path.is_file())
            .expect("AIAS_AB_INPUT 必须指向一张原图");
        let gt_path = std::env::var_os("AIAS_AB_GT")
            .map(PathBuf::from)
            .filter(|path| path.is_file())
            .expect("AIAS_AB_GT 必须指向人工 alpha 真值");
        let out_dir = std::env::var_os("AIAS_AB_OUTPUT")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(r"F:\AIAS\ab\local-hr"));
        let tag = std::env::var("AIAS_AB_TAG").unwrap_or_else(|_| "local-hr".into());
        let local_label = std::env::var("AIAS_AB_LOCAL_LABEL").unwrap_or_else(|_| "hr".into());
        assert!(
            !local_label.is_empty()
                && local_label
                    .chars()
                    .all(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_')),
            "AIAS_AB_LOCAL_LABEL 只能包含 ASCII 字母、数字、-、_"
        );
        let model_path = std::env::var_os("AIAS_AB_LOCAL_MODEL")
            .map(PathBuf::from)
            .unwrap_or_else(|| models_dir(&base).join("BiRefNet_HR-general-epoch_130.onnx"));
        assert!(model_path.is_file(), "本地 HR 模型不可读：{}", model_path.display());
        fs::create_dir_all(&out_dir).expect("创建 HR 对照输出目录");

        let original = image::open(&input).expect("原图可读").to_rgb8();
        let gt = image::open(&gt_path).expect("人工真值可读").to_rgba8();
        assert_eq!(original.dimensions(), gt.dimensions(), "原图与 GT 尺寸必须一致");
        let (w, h) = original.dimensions();
        let stem = input.file_stem().and_then(|value| value.to_str()).unwrap_or("image");

        let started = std::time::Instant::now();
        let hr_alpha = run_birefnet_local_file(
            &base,
            &format!("ab-local-{local_label}"),
            &model_path,
            true,
            false,
            &original,
        )
        .unwrap_or_else(|error| panic!("本地 HR BiRefNet 推理失败：{error}"));
        println!("{tag} local HR inference elapsed {:?}", started.elapsed());
        assert_eq!(hr_alpha.len(), (w * h) as usize, "HR alpha 尺寸无效");

        let mut hr_result = RgbaImage::new(w, h);
        let mut hr_matte = ImageBuffer::<Luma<u8>, Vec<u8>>::new(w, h);
        for (index, (pixel, matte_pixel)) in hr_result
            .pixels_mut()
            .zip(hr_matte.pixels_mut())
            .enumerate()
        {
            let color = original.get_pixel((index as u32) % w, (index as u32) / w);
            let alpha = (hr_alpha[index] * 255.0).round().clamp(0.0, 255.0) as u8;
            *pixel = Rgba([color[0], color[1], color[2], alpha]);
            *matte_pixel = image::Luma([alpha]);
        }
        let local_result_label = format!("birefnet-{local_label}_raw");
        let hr_path = out_dir.join(format!("{stem}_{local_result_label}.png"));
        hr_result.save(&hr_path).expect("HR 原始 alpha 写出");
        hr_matte
            .save(out_dir.join(format!("{stem}_{local_result_label}_matte.png")))
            .expect("HR alpha 遮罩写出");
        let local_final_label = format!("birefnet-{local_label}_final");
        // 本开发用例的本地导出均为 1024 输入，使用与正式 General 1024 相同的
        // 原生边缘策略，避免再引入低分辨率模型才需要的引导滤波。
        let local_final = finalize_cutout_image(&original, hr_alpha, true);
        local_final
            .save(out_dir.join(format!("{stem}_{local_final_label}.png")))
            .expect("本地模型正式后处理结果写出");

        // 同目录重跑当前正式结果，让本地模型与完整正式管线可独立复核。
        let general_path = out_dir.join(format!("{stem}_birefnet-general_final.png"));
        cutout_with_fallback(&base, "birefnet-general", &input, &general_path)
            .expect("正式 general 参照推理");
        let general = image::open(&general_path).expect("正式 general 可读").to_rgba8();

        let alignment = source_gt_alignment(&original, &gt);
        let mut report = format!(
            "AIAS local BiRefNet A/B report\ntag={tag}\ninput={}\nground_truth={}\nlocal_model={}\ncomparison=local raw/final alpha vs current general final pipeline\nalignment_rgb_mae={:.4}\nalignment_changed_ratio={:.5}\n",
            input.display(),
            gt_path.display(),
            model_path.display(),
            alignment.rgb_mae,
            alignment.rgb_changed_ratio,
        );
        append_instance_metrics(&mut report, "birefnet-general_final", &gt, &general);
        append_instance_metrics(
            &mut report,
            &local_result_label,
            &gt,
            &hr_result,
        );
        append_instance_metrics(&mut report, &local_final_label, &gt, &local_final);
        fs::write(out_dir.join(format!("{tag}_metrics.txt")), report)
            .expect("HR 对照指标报告写出");

        for (label, result) in [
            ("birefnet-general_final", &general),
            (local_result_label.as_str(), &hr_result),
            (local_final_label.as_str(), &local_final),
        ] {
            ab_gt_preview(
                &input,
                &gt,
                result,
                &out_dir.join(format!("{stem}_{label}_preview.jpg")),
            );
            alpha_error_heatmap(
                &original,
                &gt,
                result,
                &out_dir.join(format!("{stem}_{label}_alpha_error.png")),
            );
        }
    }

    /// 开发期原型：固定 BiRefNet general 的 alpha，只用经「主角重排序」后的
    /// 动漫实例遮罩限制其有效范围。`clean` 与 `preserve` 只改变同一实例遮罩的
    /// 膨胀半径，借此量化背景残留和主体误删的取舍；不接入正式 UI 或输出流程。
    #[test]
    #[ignore = "手动执行：主角实例约束 A/B 原型"]
    fn ab_instance_constraint() {
        assert!(
            ab_main_subject_selector_enabled(),
            "请设置 AIAS_AB_INSTANCE_SELECTOR=main-subject"
        );
        let base = dirs::home_dir()
            .expect("无法定位用户目录")
            .join(r"AppData\Roaming\studio.avroracl.aias");
        let input = std::env::var_os("AIAS_AB_INPUT")
            .map(PathBuf::from)
            .filter(|path| path.is_file())
            .expect("AIAS_AB_INPUT 必须指向一张原图");
        let gt_path = std::env::var_os("AIAS_AB_GT")
            .map(PathBuf::from)
            .filter(|path| path.is_file())
            .expect("AIAS_AB_GT 必须指向人工 alpha 真值");
        let out_dir = std::env::var_os("AIAS_AB_OUTPUT")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(r"F:\AIAS\ab\instance-constraint"));
        let tag = std::env::var("AIAS_AB_TAG").unwrap_or_else(|_| "instance-constraint".into());
        let clean_radius = ab_env_usize("AIAS_AB_INSTANCE_CLEAN_RADIUS", 48);
        let preserve_radius = ab_env_usize("AIAS_AB_INSTANCE_PRESERVE_RADIUS", 192);
        let primary_component_only = std::env::var("AIAS_AB_INSTANCE_COMPONENT")
            .ok()
            .is_some_and(|value| value.eq_ignore_ascii_case("primary"));
        let radii: Vec<(String, usize)> = std::env::var("AIAS_AB_INSTANCE_RADII")
            .ok()
            .map(|value| {
                value
                    .split(',')
                    .filter_map(|part| part.trim().parse::<usize>().ok())
                    .filter(|radius| *radius <= u16::MAX as usize - 1)
                    .map(|radius| (format!("r{radius}"), radius))
                    .collect()
            })
            .filter(|values: &Vec<(String, usize)>| !values.is_empty())
            .unwrap_or_else(|| {
                vec![
                    ("clean".to_string(), clean_radius),
                    ("preserve".to_string(), preserve_radius),
                ]
            });
        let adaptive: Vec<(usize, usize, f32)> = std::env::var("AIAS_AB_INSTANCE_ADAPTIVE")
            .ok()
            .map(|value| {
                value
                    .split(',')
                    .filter_map(|part| {
                        let mut values = part.trim().split(':');
                        let inner = values.next()?.parse::<usize>().ok()?;
                        let outer = values.next()?.parse::<usize>().ok()?;
                        let confidence = values.next()?.parse::<f32>().ok()?;
                        (inner <= outer
                            && outer <= u16::MAX as usize - 1
                            && (0.0..=1.0).contains(&confidence))
                            .then_some((inner, outer, confidence))
                    })
                    .collect()
            })
            .unwrap_or_default();
        fs::create_dir_all(&out_dir).expect("创建实例约束 A/B 输出目录");

        let original = image::open(&input).expect("原图可读").to_rgb8();
        let gt = image::open(&gt_path).expect("人工真值可读").to_rgba8();
        assert_eq!(original.dimensions(), gt.dimensions(), "原图与 GT 尺寸必须一致");
        assert!(is_model_ready(&base, "birefnet-general"), "测试需要 BiRefNet general");
        assert!(is_model_ready(&base, "advanced"), "测试需要动漫精细模型");

        let stem = input.file_stem().and_then(|value| value.to_str()).unwrap_or("image");
        let general_path = out_dir.join(format!("{stem}_instance_general.png"));
        cutout_with_fallback(&base, "birefnet-general", &input, &general_path)
            .expect("通用高质量 alpha 推理");
        let general = image::open(&general_path).expect("通用结果可读").to_rgba8();
        let raw_instance = run_advanced(&base, &original).expect("主角实例推理");
        let (w, h) = original.dimensions();
        let instance = if primary_component_only {
            main_instance_component(&raw_instance, w, h)
        } else {
            raw_instance.clone()
        };
        let mut instance_mask = ImageBuffer::<Luma<u8>, Vec<u8>>::new(w, h);
        for (index, pixel) in instance_mask.pixels_mut().enumerate() {
            *pixel = image::Luma([(instance[index] * 255.0).round().clamp(0.0, 255.0) as u8]);
        }
        instance_mask
            .save(out_dir.join(format!("{stem}_instance_mask.png")))
            .expect("实例遮罩写出");

        let alignment = source_gt_alignment(&original, &gt);
        let mut report = format!(
            "AIAS instance-constraint A/B report\ntag={tag}\ninput={}\nground_truth={}\nselector=main-subject\nprimary_component_only={primary_component_only}\nradii={}\nadaptive={}\nalignment_rgb_mae={:.4}\nalignment_changed_ratio={:.5}\n",
            input.display(),
            gt_path.display(),
            radii.iter().map(|(_, radius)| radius.to_string()).collect::<Vec<_>>().join(","),
            adaptive.iter().map(|(inner, outer, threshold)| format!("{inner}:{outer}:{threshold:.2}")).collect::<Vec<_>>().join(","),
            alignment.rgb_mae,
            alignment.rgb_changed_ratio,
        );
        append_instance_metrics(&mut report, "general", &gt, &general);
        for (label, radius) in &radii {
            let gate = dilated_instance_gate(&instance, w, h, *radius);
            let mut gate_image = ImageBuffer::<Luma<u8>, Vec<u8>>::new(w, h);
            for (pixel, keep) in gate_image.pixels_mut().zip(gate.iter()) {
                *pixel = image::Luma([if *keep { 255 } else { 0 }]);
            }
            gate_image
                .save(out_dir.join(format!("{stem}_instance_gate_{label}.png")))
                .expect("实例约束写出");
            let constrained = apply_instance_gate(&general, &gate);
            let result_path = out_dir.join(format!("{stem}_instance_{label}.png"));
            constrained.save(&result_path).expect("实例约束结果写出");
            append_instance_metrics(&mut report, label, &gt, &constrained);
            ab_gt_preview(
                &input,
                &gt,
                &constrained,
                &out_dir.join(format!("{stem}_instance_{label}_preview.jpg")),
            );
            alpha_error_heatmap(
                &original,
                &gt,
                &constrained,
                &out_dir.join(format!("{stem}_instance_{label}_alpha_error.png")),
            );
        }
        if let Some(max_outer) = adaptive.iter().map(|(_, outer, _)| *outer).max() {
            let distance = instance_distance(&instance, w, h, max_outer);
            for (inner, outer, threshold) in adaptive {
                let label = format!(
                    "adaptive_i{inner}_o{outer}_t{:02}",
                    (threshold * 100.0).round() as u8
                );
                let constrained = apply_adaptive_instance_gate(
                    &general,
                    &distance,
                    inner,
                    outer,
                    threshold,
                );
                constrained
                    .save(out_dir.join(format!("{stem}_instance_{label}.png")))
                    .expect("自适应实例约束结果写出");
                append_instance_metrics(&mut report, &label, &gt, &constrained);
                ab_gt_preview(
                    &input,
                    &gt,
                    &constrained,
                    &out_dir.join(format!("{stem}_instance_{label}_preview.jpg")),
                );
                alpha_error_heatmap(
                    &original,
                    &gt,
                    &constrained,
                    &out_dir.join(format!("{stem}_instance_{label}_alpha_error.png")),
                );
            }
        }
        fs::write(out_dir.join(format!("{tag}_metrics.txt")), report)
            .expect("实例约束指标报告写出");
    }

    fn ab_env_usize(name: &str, default: usize) -> usize {
        std::env::var(name)
            .ok()
            .and_then(|value| value.parse::<usize>().ok())
            .filter(|value| *value > 0 && *value <= u16::MAX as usize - 1)
            .unwrap_or(default)
    }

    /// 选中实例的 Chebyshev 膨胀：`true` 是允许 BiRefNet alpha 保留的区域。
    fn dilated_instance_gate(matte: &[f32], width: u32, height: u32, radius: usize) -> Vec<bool> {
        let (w, h) = (width as usize, height as usize);
        assert_eq!(matte.len(), w * h);
        let limit = (radius + 1) as u16;
        let mut distance: Vec<u16> = matte
            .iter()
            .map(|alpha| if *alpha > 0.5 { 0 } else { limit })
            .collect();
        let step = |value: u16| value.saturating_add(1).min(limit);
        for y in 0..h {
            for x in 0..w {
                let index = y * w + x;
                if distance[index] == 0 {
                    continue;
                }
                let mut best = distance[index];
                if x > 0 {
                    best = best.min(step(distance[index - 1]));
                }
                if y > 0 {
                    best = best.min(step(distance[index - w]));
                    if x > 0 {
                        best = best.min(step(distance[index - w - 1]));
                    }
                    if x + 1 < w {
                        best = best.min(step(distance[index - w + 1]));
                    }
                }
                distance[index] = best;
            }
        }
        for y in (0..h).rev() {
            for x in (0..w).rev() {
                let index = y * w + x;
                if distance[index] == 0 {
                    continue;
                }
                let mut best = distance[index];
                if x + 1 < w {
                    best = best.min(step(distance[index + 1]));
                }
                if y + 1 < h {
                    best = best.min(step(distance[index + w]));
                    if x > 0 {
                        best = best.min(step(distance[index + w - 1]));
                    }
                    if x + 1 < w {
                        best = best.min(step(distance[index + w + 1]));
                    }
                }
                distance[index] = best;
            }
        }
        distance.into_iter().map(|value| value <= radius as u16).collect()
    }

    fn apply_instance_gate(general: &RgbaImage, gate: &[bool]) -> RgbaImage {
        assert_eq!(general.width() as usize * general.height() as usize, gate.len());
        let mut constrained = general.clone();
        for (pixel, keep) in constrained.pixels_mut().zip(gate) {
            if !*keep {
                pixel[3] = 0;
            }
        }
        constrained
    }

    /// 计算像素到实例实心区域的 Chebyshev 距离；超过 `limit` 的值饱和，便于
    /// 同一张距离图派生多种门控半径。与 `dilated_instance_gate` 的距离定义一致。
    fn instance_distance(matte: &[f32], width: u32, height: u32, limit: usize) -> Vec<u16> {
        let (w, h) = (width as usize, height as usize);
        assert_eq!(matte.len(), w * h);
        let limit = (limit + 1) as u16;
        let mut distance: Vec<u16> = matte
            .iter()
            .map(|alpha| if *alpha > 0.5 { 0 } else { limit })
            .collect();
        let step = |value: u16| value.saturating_add(1).min(limit);
        for y in 0..h {
            for x in 0..w {
                let index = y * w + x;
                if distance[index] == 0 {
                    continue;
                }
                let mut best = distance[index];
                if x > 0 {
                    best = best.min(step(distance[index - 1]));
                }
                if y > 0 {
                    best = best.min(step(distance[index - w]));
                    if x > 0 {
                        best = best.min(step(distance[index - w - 1]));
                    }
                    if x + 1 < w {
                        best = best.min(step(distance[index - w + 1]));
                    }
                }
                distance[index] = best;
            }
        }
        for y in (0..h).rev() {
            for x in (0..w).rev() {
                let index = y * w + x;
                if distance[index] == 0 {
                    continue;
                }
                let mut best = distance[index];
                if x + 1 < w {
                    best = best.min(step(distance[index + 1]));
                }
                if y + 1 < h {
                    best = best.min(step(distance[index + w]));
                    if x > 0 {
                        best = best.min(step(distance[index + w - 1]));
                    }
                    if x + 1 < w {
                        best = best.min(step(distance[index + w + 1]));
                    }
                }
                distance[index] = best;
            }
        }
        distance
    }

    /// 近区完全信任主实例门控；远区只允许高置信度 General alpha 通过。它是
    /// 为长发/衣摆等离实例粗遮罩较远的细结构预留的受控通道，绝不会无边界地
    /// 放回整个背景。
    fn apply_adaptive_instance_gate(
        general: &RgbaImage,
        distance: &[u16],
        inner: usize,
        outer: usize,
        alpha_threshold: f32,
    ) -> RgbaImage {
        assert_eq!(general.width() as usize * general.height() as usize, distance.len());
        let mut constrained = general.clone();
        for (pixel, distance) in constrained.pixels_mut().zip(distance) {
            let keep = *distance as usize <= inner
                || (*distance as usize <= outer
                    && pixel[3] as f32 / 255.0 >= alpha_threshold);
            if !keep {
                pixel[3] = 0;
            }
        }
        constrained
    }

    /// 只留下实例遮罩中面积最大、也最靠近画面中心的连通块。RTMDet 已经选中
    /// 中心角色，但其 refinement mask 偶尔仍会在远处花瓶或装饰处溢出独立小块；
    /// 用这一步作为“门控的门控”，不会直接写入最终 alpha，细发丝仍由后续
    /// 膨胀范围内的 BiRefNet alpha 决定。
    fn main_instance_component(matte: &[f32], width: u32, height: u32) -> Vec<f32> {
        let (w, h) = (width as usize, height as usize);
        assert_eq!(matte.len(), w * h, "实例遮罩尺寸无效");
        let mut labels = vec![u32::MAX; matte.len()];
        let mut components: Vec<(usize, usize, usize, usize, usize)> = Vec::new();
        let mut stack = Vec::new();
        for start in 0..matte.len() {
            if matte[start] <= 0.5 || labels[start] != u32::MAX {
                continue;
            }
            let component = components.len() as u32;
            labels[start] = component;
            stack.push(start);
            let (mut count, mut min_x, mut min_y, mut max_x, mut max_y) = (0usize, w, h, 0usize, 0usize);
            while let Some(index) = stack.pop() {
                let (x, y) = (index % w, index / w);
                count += 1;
                min_x = min_x.min(x);
                min_y = min_y.min(y);
                max_x = max_x.max(x);
                max_y = max_y.max(y);
                for yy in y.saturating_sub(1)..=(y + 1).min(h - 1) {
                    for xx in x.saturating_sub(1)..=(x + 1).min(w - 1) {
                        let next = yy * w + xx;
                        if matte[next] > 0.5 && labels[next] == u32::MAX {
                            labels[next] = component;
                            stack.push(next);
                        }
                    }
                }
            }
            components.push((count, min_x, min_y, max_x, max_y));
        }
        let Some((best, _)) = components.iter().enumerate().max_by(|(_, left), (_, right)| {
            let score = |component: &(usize, usize, usize, usize, usize)| {
                let (count, min_x, min_y, max_x, max_y) = *component;
                let area = count as f32 / (w * h).max(1) as f32;
                let cx = (min_x + max_x) as f32 * 0.5 / w.max(1) as f32;
                let cy = (min_y + max_y) as f32 * 0.5 / h.max(1) as f32;
                let center = 1.0 - ((cx - 0.5).powi(2) + (cy - 0.5).powi(2)).sqrt() / 0.707_106_77;
                area.sqrt() * 0.75 + center.clamp(0.0, 1.0) * 0.25
            };
            score(left).total_cmp(&score(right))
        }) else {
            return vec![0.0; matte.len()];
        };
        matte
            .iter()
            .zip(labels.iter())
            .map(|(alpha, label)| if *label == best as u32 { *alpha } else { 0.0 })
            .collect()
    }

    fn append_instance_metrics(report: &mut String, label: &str, gt: &RgbaImage, result: &RgbaImage) {
        let metrics = alpha_metrics(gt, result);
        report.push_str(&format!(
            "{label}\tmae={:.5}\tiou={:.5}\tinterior_miss_mean={:.5}\tboundary_mae={:.5}\texterior_leak_mean={:.5}\n",
            metrics.mae,
            metrics.iou,
            metrics.interior_miss_mean,
            metrics.boundary_mae,
            metrics.exterior_leak_mean,
        ));
    }

    /// 与人工 alpha 真值的可解释差异：背景残留、主体漏抠、半透明边缘和硬遮罩 IoU。
    #[derive(Debug, Clone, Copy)]
    struct AlphaMetrics {
        mae: f32,
        background_leak_mean: f32,
        background_leak_ratio: f32,
        foreground_miss_mean: f32,
        foreground_miss_ratio: f32,
        edge_mae: f32,
        iou: f32,
        interior_miss_mean: f32,
        boundary_mae: f32,
        exterior_leak_mean: f32,
    }

    #[derive(Debug, Clone, Copy)]
    struct AlignmentMetrics {
        solid_pixels: usize,
        rgb_mae: f32,
        rgb_changed_ratio: f32,
    }

    /// 验证人工版是否仍与原图同一像素网格。只读取 alpha ≥ 0.98 的区域，
    /// 避免透明背景色和边缘去污染影响判断；这里报告而非强制断言，因为
    /// 人工版有可能有正当的前景修色。
    fn source_gt_alignment(source: &RgbImage, gt: &RgbaImage) -> AlignmentMetrics {
        assert_eq!(source.dimensions(), gt.dimensions());
        let mut total = 0u64;
        let mut changed = 0usize;
        let mut pixels = 0usize;
        for (source, truth) in source.pixels().zip(gt.pixels()) {
            if truth[3] < 250 {
                continue;
            }
            pixels += 1;
            let mut delta = 0u32;
            for channel in 0..3 {
                delta += (source[channel] as i16 - truth[channel] as i16).unsigned_abs() as u32;
            }
            total += delta as u64;
            if delta > 12 {
                changed += 1;
            }
        }
        AlignmentMetrics {
            solid_pixels: pixels,
            rgb_mae: total as f32 / (pixels.max(1) * 3) as f32,
            rgb_changed_ratio: changed as f32 / pixels.max(1) as f32,
        }
    }

    fn alpha_metrics(gt: &RgbaImage, result: &RgbaImage) -> AlphaMetrics {
        assert_eq!(gt.dimensions(), result.dimensions());
        let alpha: Vec<u8> = result.pixels().map(|pixel| pixel[3]).collect();
        alpha_metrics_bytes(gt, &alpha)
    }

    fn alpha_metrics_luma(gt: &RgbaImage, result: &image::GrayImage) -> AlphaMetrics {
        assert_eq!(gt.dimensions(), result.dimensions());
        alpha_metrics_bytes(gt, result.as_raw())
    }

    fn alpha_metrics_bytes(gt: &RgbaImage, predicted_alpha: &[u8]) -> AlphaMetrics {
        assert_eq!(gt.width() as usize * gt.height() as usize, predicted_alpha.len());
        let (width, height) = gt.dimensions();
        let solid: Vec<bool> = gt.pixels().map(|pixel| pixel[3] > 127).collect();
        let dist_to_foreground = chebyshev_distance_clamped(&solid, width, height, 4);
        let background: Vec<bool> = solid.iter().map(|value| !value).collect();
        let dist_to_background = chebyshev_distance_clamped(&background, width, height, 4);
        let mut total_error = 0.0f64;
        let mut bg_alpha = 0.0f64;
        let mut bg_count = 0usize;
        let mut bg_leak_count = 0usize;
        let mut fg_loss = 0.0f64;
        let mut fg_count = 0usize;
        let mut fg_miss_count = 0usize;
        let mut edge_error = 0.0f64;
        let mut edge_count = 0usize;
        let mut intersection = 0usize;
        let mut union = 0usize;
        let mut interior_loss = 0.0f64;
        let mut interior_count = 0usize;
        let mut boundary_error = 0.0f64;
        let mut boundary_count = 0usize;
        let mut exterior_alpha = 0.0f64;
        let mut exterior_count = 0usize;

        for (index, (truth, predicted)) in gt.pixels().zip(predicted_alpha).enumerate() {
            let (truth, predicted) = (truth[3] as f32 / 255.0, *predicted as f32 / 255.0);
            total_error += f64::from((truth - predicted).abs());
            if truth <= 0.02 {
                bg_count += 1;
                bg_alpha += f64::from(predicted);
                if predicted > 0.10 {
                    bg_leak_count += 1;
                }
            }
            if truth >= 0.98 {
                fg_count += 1;
                fg_loss += f64::from(1.0 - predicted);
                if predicted < 0.50 {
                    fg_miss_count += 1;
                }
            }
            if (0.02..0.98).contains(&truth) {
                edge_count += 1;
                edge_error += f64::from((truth - predicted).abs());
            }
            let truth_solid = truth > 0.5;
            let predicted_solid = predicted > 0.5;
            if truth_solid || predicted_solid {
                union += 1;
            }
            if truth_solid && predicted_solid {
                intersection += 1;
            }
            // 空间区域不取决于 GT 是否有抗锯齿 alpha：轮廓带是二值轮廓的两侧各 3px。
            if truth >= 0.98 && dist_to_background[index] > 3 {
                interior_count += 1;
                interior_loss += f64::from(1.0 - predicted);
            }
            if dist_to_foreground[index] <= 3 && dist_to_background[index] <= 3 {
                boundary_count += 1;
                boundary_error += f64::from((truth - predicted).abs());
            }
            if truth <= 0.02 && dist_to_foreground[index] > 3 {
                exterior_count += 1;
                exterior_alpha += f64::from(predicted);
            }
        }
        let n = (gt.width() as usize * gt.height() as usize).max(1) as f64;
        AlphaMetrics {
            mae: (total_error / n) as f32,
            background_leak_mean: (bg_alpha / bg_count.max(1) as f64) as f32,
            background_leak_ratio: bg_leak_count as f32 / bg_count.max(1) as f32,
            foreground_miss_mean: (fg_loss / fg_count.max(1) as f64) as f32,
            foreground_miss_ratio: fg_miss_count as f32 / fg_count.max(1) as f32,
            edge_mae: (edge_error / edge_count.max(1) as f64) as f32,
            iou: intersection as f32 / union.max(1) as f32,
            interior_miss_mean: (interior_loss / interior_count.max(1) as f64) as f32,
            boundary_mae: (boundary_error / boundary_count.max(1) as f64) as f32,
            exterior_leak_mean: (exterior_alpha / exterior_count.max(1) as f64) as f32,
        }
    }

    /// 到最近 seed 的 8 邻域（Chebyshev）距离，最大只需区分到 4px 以外。
    fn chebyshev_distance_clamped(seeds: &[bool], width: u32, height: u32, max: u8) -> Vec<u8> {
        let (w, h) = (width as usize, height as usize);
        assert_eq!(seeds.len(), w * h);
        let mut distance: Vec<u8> = seeds
            .iter()
            .map(|seed| if *seed { 0 } else { max })
            .collect();
        let step = |value: u8| value.saturating_add(1).min(max);
        for y in 0..h {
            for x in 0..w {
                let index = y * w + x;
                if distance[index] == 0 {
                    continue;
                }
                let mut best = distance[index];
                if x > 0 {
                    best = best.min(step(distance[index - 1]));
                }
                if y > 0 {
                    best = best.min(step(distance[index - w]));
                    if x > 0 {
                        best = best.min(step(distance[index - w - 1]));
                    }
                    if x + 1 < w {
                        best = best.min(step(distance[index - w + 1]));
                    }
                }
                distance[index] = best;
            }
        }
        for y in (0..h).rev() {
            for x in (0..w).rev() {
                let index = y * w + x;
                if distance[index] == 0 {
                    continue;
                }
                let mut best = distance[index];
                if x + 1 < w {
                    best = best.min(step(distance[index + 1]));
                }
                if y + 1 < h {
                    best = best.min(step(distance[index + w]));
                    if x > 0 {
                        best = best.min(step(distance[index + w - 1]));
                    }
                    if x + 1 < w {
                        best = best.min(step(distance[index + w + 1]));
                    }
                }
                distance[index] = best;
            }
        }
        distance
    }

    /// 误差热图：红色表示 AI 多保留（背景残留），蓝色表示 AI 漏抠（主体缺失）。
    /// 原图降亮度作底，误差强度与 alpha 差成比例，可直接定位需要优化的空间区域。
    fn alpha_error_heatmap(source: &RgbImage, gt: &RgbaImage, result: &RgbaImage, out: &Path) {
        assert_eq!(source.dimensions(), gt.dimensions());
        assert_eq!(gt.dimensions(), result.dimensions());
        let mut heatmap = RgbImage::new(source.width(), source.height());
        for (((source, truth), predicted), target) in source
            .pixels()
            .zip(gt.pixels())
            .zip(result.pixels())
            .zip(heatmap.pixels_mut())
        {
            let delta = predicted[3] as f32 / 255.0 - truth[3] as f32 / 255.0;
            let base = [
                (source[0] as f32 * 0.22).round(),
                (source[1] as f32 * 0.22).round(),
                (source[2] as f32 * 0.22).round(),
            ];
            let intensity = ((delta.abs() - 0.05) / 0.95).clamp(0.0, 1.0);
            let tint = if delta >= 0.0 { [255.0, 40.0, 40.0] } else { [45.0, 130.0, 255.0] };
            *target = image::Rgb([
                (base[0] * (1.0 - intensity) + tint[0] * intensity).round() as u8,
                (base[1] * (1.0 - intensity) + tint[1] * intensity).round() as u8,
                (base[2] * (1.0 - intensity) + tint[2] * intensity).round() as u8,
            ]);
        }
        heatmap.save(out).expect("alpha 误差热图写出");
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
        let over_white = ab_composite(&r_small, [255.0, 255.0, 255.0]);
        let over_dark = ab_composite(&r_small, [38.0, 42.0, 50.0]);
        let gap = 8;
        let mut canvas = RgbImage::new(tw * 3 + gap * 2, th);
        image::imageops::overlay(&mut canvas, &o_small, 0, 0);
        image::imageops::overlay(&mut canvas, &over_white, (tw + gap) as i64, 0);
        image::imageops::overlay(&mut canvas, &over_dark, ((tw + gap) * 2) as i64, 0);
        canvas.save(out).expect("预览图写出");
    }

    /// GT 五联预览：原图 | 人工抠图（白/深） | 当前结果（白/深）。
    fn ab_gt_preview(input: &Path, gt: &RgbaImage, result: &RgbaImage, out: &Path) {
        let (w, h) = result.dimensions();
        let scale = (1200.0 / h as f32).min(1.0);
        let (tw, th) = (
            ((w as f32 * scale).round() as u32).max(1),
            ((h as f32 * scale).round() as u32).max(1),
        );
        let original = image::open(input).expect("原图可读").to_rgb8();
        let o_small = image::imageops::resize(&original, tw, th, FilterType::Triangle);
        let gt_small = image::imageops::resize(gt, tw, th, FilterType::Triangle);
        let result_small = image::imageops::resize(result, tw, th, FilterType::Triangle);
        let gt_white = ab_composite(&gt_small, [255.0, 255.0, 255.0]);
        let gt_dark = ab_composite(&gt_small, [38.0, 42.0, 50.0]);
        let result_white = ab_composite(&result_small, [255.0, 255.0, 255.0]);
        let result_dark = ab_composite(&result_small, [38.0, 42.0, 50.0]);
        let gap = 8;
        let mut canvas = RgbImage::new(tw * 5 + gap * 4, th);
        for (index, panel) in [o_small, gt_white, gt_dark, result_white, result_dark]
            .into_iter()
            .enumerate()
        {
            image::imageops::overlay(&mut canvas, &panel, (index as u32 * (tw + gap)) as i64, 0);
        }
        canvas.save(out).expect("GT 预览图写出");
    }

    fn ab_composite(result: &RgbaImage, bg: [f32; 3]) -> RgbImage {
        let (tw, th) = result.dimensions();
        let mut composite = RgbImage::new(tw, th);
        for y in 0..th {
            for x in 0..tw {
                let pixel = result.get_pixel(x, y);
                let a = pixel[3] as f32 / 255.0;
                composite.put_pixel(
                    x,
                    y,
                    image::Rgb([
                        (pixel[0] as f32 * a + bg[0] * (1.0 - a)).round() as u8,
                        (pixel[1] as f32 * a + bg[1] * (1.0 - a)).round() as u8,
                        (pixel[2] as f32 * a + bg[2] * (1.0 - a)).round() as u8,
                    ]),
                );
            }
        }
        composite
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
