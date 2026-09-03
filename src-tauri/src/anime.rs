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

pub struct ModelSpec {
    pub id: &'static str,
    pub label: &'static str,
    pub files: &'static [ModelFileSpec],
}

const ISNETIS_SIZE: u64 = 176_069_933;
const RTMDET_SIZE: u64 = 238_686_077;
const REFINER_SIZE: u64 = 176_197_192;
const TOONOUT_SIZE: u64 = 492_381_880;

pub const MODELS: &[ModelSpec] = &[
    ModelSpec {
        id: "toonout",
        label: "动漫特化（ToonOut）",
        files: &[ModelFileSpec {
            name: "birefnet-toonout-fp16.onnx",
            size: TOONOUT_SIZE,
            mirror_url: "https://hf-mirror.com/sprited/birefnet-toonout-onnx/resolve/main/birefnet-toonout-fp16.onnx",
            origin_url: "https://huggingface.co/sprited/birefnet-toonout-onnx/resolve/main/birefnet-toonout-fp16.onnx",
        }],
    },
    ModelSpec {
        id: "simple",
        label: "标准抠图（ISNet）",
        files: &[ModelFileSpec {
            name: "isnetis.onnx",
            size: ISNETIS_SIZE,
            mirror_url: "https://hf-mirror.com/skytnt/anime-seg/resolve/main/isnetis.onnx",
            origin_url: "https://huggingface.co/skytnt/anime-seg/resolve/main/isnetis.onnx",
        }],
    },
    ModelSpec {
        id: "advanced",
        label: "精细抠图（RTMDet+精修）",
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

pub fn ensure_ort_runtime(base: &Path) -> Result<(), String> {
    static ORT_READY: OnceLock<()> = OnceLock::new();
    fs::create_dir_all(base).map_err(to_string_error)?;
    let dll = ort_dll_path(base);
    if !dll.exists() {
        acquire_ort_dll(base)?;
    }
    ensure_ort_cuda_providers(base)?;
    if ORT_READY.get().is_some() {
        return Ok(());
    }
    std::env::set_var("ORT_DYLIB_PATH", &dll);
    // set 失败说明另一个线程刚刚完成了同样的初始化（相同 dll 路径），视为成功。
    let _ = ORT_READY.set(());
    Ok(())
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
            let source_dir = candidate.parent().unwrap_or(base);
            fs::copy(&candidate, ort_dll_path(base)).map_err(to_string_error)?;
            copy_ort_provider_files(source_dir, base)?;
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
    match id {
        "simple" => {
            simple_slot().lock().map_err(lock_error)?.take();
        }
        "advanced" => {
            advanced_slot().lock().map_err(lock_error)?.take();
        }
        "toonout" => {
            toonout_slot().lock().map_err(lock_error)?.take();
        }
        _ => return Err(format!("未知模型：{id}")),
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
static TOONOUT_SESSION: OnceLock<Mutex<Option<Session>>> = OnceLock::new();

fn simple_slot() -> &'static Mutex<Option<SimpleSessions>> {
    SIMPLE_SESSION.get_or_init(|| Mutex::new(None))
}

fn advanced_slot() -> &'static Mutex<Option<AdvancedSessions>> {
    ADVANCED_SESSIONS.get_or_init(|| Mutex::new(None))
}

fn toonout_slot() -> &'static Mutex<Option<Session>> {
    TOONOUT_SESSION.get_or_init(|| Mutex::new(None))
}

fn build_session(path: &Path) -> Result<Session, String> {
    let threads = std::thread::available_parallelism()
        .map(|value| value.get().clamp(1, 8))
        .unwrap_or(4);
    preload_cuda_runtime();
    // CUDA EP 注册失败时会静默回退 CPU（error_on_failure 默认 false）。
    let cuda = ort::ep::CUDA::default().build();
    Session::builder()
        .map_err(to_string_error)?
        .with_optimization_level(GraphOptimizationLevel::Level3)
        .map_err(to_string_error)?
        .with_intra_threads(threads)
        .map_err(to_string_error)?
        .with_execution_providers([cuda])
        .map_err(to_string_error)?
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

fn copy_ort_provider_files(source_dir: &Path, target_dir: &Path) -> Result<(), String> {
    for name in [
        "onnxruntime_providers_shared.dll",
        "onnxruntime_providers_cuda.dll",
    ] {
        let source = source_dir.join(name);
        let target = target_dir.join(name);
        if source.exists() && !target.exists() {
            fs::copy(source, target).map_err(to_string_error)?;
        }
    }
    Ok(())
}

fn ensure_ort_cuda_providers(base: &Path) -> Result<(), String> {
    if base.join("onnxruntime_providers_cuda.dll").exists()
        && base.join("onnxruntime_providers_shared.dll").exists()
    {
        return Ok(());
    }
    for runtime in ort_seed_candidates() {
        let Some(source_dir) = runtime.parent() else {
            continue;
        };
        if source_dir.join("onnxruntime_providers_cuda.dll").exists() {
            copy_ort_provider_files(source_dir, base)?;
            break;
        }
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
        .find(|path| CUDNN9_FILES.iter().all(|name| path.join(name).exists()));
    let _ = ort::ep::cuda::preload_dylibs(cuda_root.as_deref(), cudnn_root.as_deref());
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
    let mut guard = simple_slot().lock().map_err(lock_error)?;
    if guard.is_none() {
        let path = models_dir(base).join("isnetis.onnx");
        if !path.exists() {
            return Err("标准模型未安装，请先在参数面板下载。".into());
        }
        *guard = Some(SimpleSessions {
            session: build_session(&path)?,
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
    let mut guard = advanced_slot().lock().map_err(lock_error)?;
    if guard.is_none() {
        let seg_path = models_dir(base).join("anime_segmentor_rtmdet_e60_simplified.onnx");
        let refine_path =
            models_dir(base).join("mask_refiner_isnetdis_refine_last_simplified.onnx");
        if !seg_path.exists() || !refine_path.exists() {
            return Err("精细模型未安装，请先在参数面板下载。".into());
        }
        *guard = Some(AdvancedSessions {
            seg: build_session(&seg_path)?,
            refine: build_session(&refine_path)?,
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
    let prob_u8: Vec<u8> = cropped
        .iter()
        .map(|prob| if *prob > REFINE_THRESHOLD { 255 } else { 0 })
        .collect();
    let mask = bilinear_resize_luma(&prob_u8, crop_w as u32, crop_h as u32, w, h);
    for (index, value) in mask.into_iter().enumerate() {
        refined[index] = value;
    }
    Ok(to_f32(refined))
}

// ---------------------------------------------------------------------------
// ToonOut (BiRefNet anime fine-tune) — port of the sprited ONNX usage
// ---------------------------------------------------------------------------

const TOONOUT_MEAN: [f32; 3] = [0.485, 0.456, 0.406];
const TOONOUT_STD: [f32; 3] = [0.229, 0.224, 0.225];

fn run_toonout(base: &Path, rgb: &RgbImage) -> Result<Vec<f32>, String> {
    ensure_ort_runtime(base)?;
    let mut guard = toonout_slot().lock().map_err(lock_error)?;
    if guard.is_none() {
        let path = models_dir(base).join("birefnet-toonout-fp16.onnx");
        if !path.exists() {
            return Err("ToonOut 模型未安装，请先在参数面板下载。".into());
        }
        *guard = Some(build_session(&path)?);
    }
    let session = guard.as_mut().expect("session initialized");
    let (w, h) = rgb.dimensions();

    // Export fixes the input at 1024x1024; fall back if a rebuild is dynamic.
    let (seg_h, seg_w) = input_size(session).unwrap_or((1024, 1024));

    // BiRefNet 系模型以「等比缩放 + 居中留边」的 1024x1024 输入训练；直接拉伸
    // 会严重扭曲人物比例、显著降低分割精度（竖图尤甚）。改为先按短边等比
    // 缩放到 1024 内，再将剩余区域补成灰边，推理后裁回内容区、缩回原尺寸。
    let scale = f32::min(seg_w as f32 / w as f32, seg_h as f32 / h as f32);
    let content_w = (w as f32 * scale).round().max(1.0) as u32;
    let content_h = (h as f32 * scale).round().max(1.0) as u32;
    let mut canvas = RgbImage::from_pixel(seg_w as u32, seg_h as u32, image::Rgb([126, 126, 126]));
    let pad_x = (seg_w as u32 - content_w) / 2;
    let pad_y = (seg_h as u32 - content_h) / 2;
    if content_w != w || content_h != h {
        let scaled = image::imageops::resize(rgb, content_w, content_h, FilterType::CatmullRom);
        image::imageops::overlay(&mut canvas, &scaled, pad_x as i64, pad_y as i64);
    } else {
        image::imageops::overlay(&mut canvas, rgb, pad_x as i64, pad_y as i64);
    }
    let plane = seg_h * seg_w;
    let mut input = vec![0_f32; 3 * plane];
    for y in 0..seg_h as u32 {
        for x in 0..seg_w as u32 {
            let pixel = canvas.get_pixel(x, y);
            let dst = y as usize * seg_w + x as usize;
            input[dst] = (pixel[0] as f32 / 255.0 - TOONOUT_MEAN[0]) / TOONOUT_STD[0];
            input[plane + dst] = (pixel[1] as f32 / 255.0 - TOONOUT_MEAN[1]) / TOONOUT_STD[1];
            input[2 * plane + dst] = (pixel[2] as f32 / 255.0 - TOONOUT_MEAN[2]) / TOONOUT_STD[2];
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

    // 输入是等比缩放 + 居中留边，mask 是整块 1024 方图：先裁掉灰边区域得到
    // 内容区 mask，再缩放到原图尺寸。裁剪坐标与前面 overlay 的 pad 一一对应。
    let content_mask = crop_letterbox(mask, mask_w, mask_h, content_w, content_h, seg_w, seg_h);
    let upscaled_mask = if content_w == w && content_h == h {
        content_mask
    } else {
        let source =
            ImageBuffer::<Luma<f32>, Vec<f32>>::from_raw(content_w, content_h, content_mask)
                .ok_or("掩码缓冲无效")?;
        image::imageops::resize(&source, w, h, FilterType::Lanczos3).into_raw()
    };
    let mut result: Vec<f32> = if needs_sigmoid {
        upscaled_mask.into_iter().map(stable_sigmoid).collect()
    } else {
        upscaled_mask
    };
    // 柔化对比度：只把接近全透明/全不透明的两头压向极值，保留发丝、水花等
    // 半透明细节的中间响应，不做硬二值化（那会毁掉这些细节）。
    const MATTE_FLOOR: f32 = 0.05;
    const MATTE_CEIL: f32 = 0.95;
    for value in &mut result {
        *value =
            ((value.clamp(0.0, 1.0) - MATTE_FLOOR) / (MATTE_CEIL - MATTE_FLOOR)).clamp(0.0, 1.0);
    }
    Ok(smooth_matte_edges(&result, w, h))
}

/// 从带灰边的 1024 方图 mask 中裁出内容区域（与 preprocess 的 pad 对应）。
fn crop_letterbox(
    mask: &[f32],
    mask_w: u32,
    mask_h: u32,
    content_w: u32,
    content_h: u32,
    seg_w: usize,
    seg_h: usize,
) -> Vec<f32> {
    if (mask_w as usize, mask_h as usize) == (seg_w, seg_h) {
        let (cw, ch) = (content_w as usize, content_h as usize);
        let pad_x = (seg_w - cw) / 2;
        let pad_y = (seg_h - ch) / 2;
        let mut out = vec![0_f32; cw * ch];
        for y in 0..ch {
            for x in 0..cw {
                out[y * cw + x] = mask[(y + pad_y) * seg_w + (x + pad_x)];
            }
        }
        out
    } else {
        // mask 尺寸与画布不一致，直接返回原 mask（走 resize 分支，避免越界）。
        mask.to_vec()
    }
}

// matte 上采样后，半透明过渡带会出现锯齿和孤立噪点；只对过渡带（含 1px
// 膨胀）做 3x3 高斯平滑，实心和透明区域保持原样，避免啃掉细发丝。
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
// Public entry: cut a single image
// ---------------------------------------------------------------------------

pub fn cutout(base: &Path, model_id: &str, input: &Path, output: &Path) -> Result<(), String> {
    let mut image = image::open(input).map_err(to_string_error)?;
    if let Some(orientation) = exif_orientation(input)? {
        image.apply_orientation(orientation);
    }
    let rgb = image.to_rgb8();
    let (w, h) = rgb.dimensions();

    let mask = match model_id {
        "simple" => run_simple(base, &rgb)?,
        "advanced" => run_advanced(base, &rgb)?,
        "toonout" => run_toonout(base, &rgb)?,
        other => return Err(format!("未知模型：{other}")),
    };

    let mut result = RgbaImage::new(w, h);
    for y in 0..h {
        for x in 0..w {
            let alpha = (mask[(y * w + x) as usize] * 255.0)
                .round()
                .clamp(0.0, 255.0) as u8;
            let pixel = rgb.get_pixel(x, y);
            result.put_pixel(x, y, Rgba([pixel[0], pixel[1], pixel[2], alpha]));
        }
    }
    result
        .save_with_format(output, image::ImageFormat::Png)
        .map_err(to_string_error)
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
        let session = build_session(&path).unwrap();
        println!("inputs: {:?}", session.inputs());
        println!("outputs: {:?}", session.outputs());

        let matte = run_toonout(&base, &rgb).unwrap();
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
}
