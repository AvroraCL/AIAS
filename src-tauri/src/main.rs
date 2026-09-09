#![windows_subsystem = "windows"]

mod anime;
mod safety;
mod material_maps;
mod superres;
mod updater;
use updater::updater_check_mirror;

use image::{DynamicImage, ImageBuffer, Luma, Rgba, RgbaImage};
use image_dds::ddsfile::Dds;
use image_dds::{image_from_dds, mip_dimension};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs,
    io::BufReader,
    path::{Path, PathBuf},
    process::Command,
};
use tauri::{AppHandle, Emitter, Manager, State};
use texpresso::{Algorithm, Format as BcFormat, Params as BcParams};

#[derive(Clone)]
struct AppState {
    settings_path: PathBuf,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
struct Settings {
    auto_update: bool,
    #[serde(default)]
    material_maps: serde_json::Value,
    pbr_input_path: String,
    pbr_output_path: String,
    pbr_alpha: String,
    pbr_format: String,
    split_output_path: String,
    split_export_format: String,
    split_export_alpha: bool,
    mipmap_input_path: String,
    mipmap_output_path: String,
    mipmap_format: String,
    mipmap_alpha: String,
    #[serde(default)]
    mipmap_intermediate: bool,
    image_to_dds_output_path: String,
    image_to_dds_alpha: String,
    image_to_dds_format: String,
    scale_target: String,
    skin_manager_path: String,
    // 保留历史设置键，前端不再使用；新增 anime_model 供抠图选择器
    #[serde(default = "default_comfyui_address")]
    comfyui_address: String,
    anime_cutout_output_path: String,
    #[serde(default = "default_anime_model")]
    anime_model: String,
    #[serde(default)]
    anime_hair_refiner: bool,
    #[serde(default)]
    anime_detail_recovery: bool,
    #[serde(default)]
    superres_output_path: String,
}

fn default_comfyui_address() -> String {
    "127.0.0.1:8188".into()
}

fn default_anime_model() -> String {
    "anime-specialist".into()
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            auto_update: false,
            material_maps: serde_json::Value::Null,
            pbr_input_path: String::new(),
            pbr_output_path: String::new(),
            pbr_alpha: "black".into(),
            pbr_format: "DXT5".into(),
            split_output_path: String::new(),
            split_export_format: "png".into(),
            split_export_alpha: true,
            mipmap_input_path: String::new(),
            mipmap_output_path: String::new(),
            mipmap_format: "DXT5".into(),
            mipmap_alpha: "keep".into(),
            mipmap_intermediate: false,
            image_to_dds_output_path: String::new(),
            image_to_dds_alpha: "keep".into(),
            image_to_dds_format: "DXT5".into(),
            scale_target: "none".into(),
            skin_manager_path: String::new(),
            comfyui_address: default_comfyui_address(),
            anime_cutout_output_path: String::new(),
            anime_model: default_anime_model(),
            anime_hair_refiner: false,
            anime_detail_recovery: false,
            superres_output_path: String::new(),
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
struct TextureGroup {
    prefix: String,
    files: TextureGroupFiles,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
struct TextureGroupFiles {
    basecolor: String,
    roughness: String,
    metallic: String,
    normal: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct TaskResult {
    completed: usize,
    total: usize,
    logs: Vec<String>,
    #[serde(default)]
    outputs: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MergePbrOptions {
    input_path: String,
    output_path: String,
    alpha: Option<String>,
    format: Option<String>,
    scale: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SplitPbrOptions {
    files: Vec<String>,
    output_path: String,
    export_format: Option<String>,
    export_alpha: Option<bool>,
    scale: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MipmapOptions {
    input_path: String,
    output_path: String,
    alpha: Option<String>,
    format: Option<String>,
    scale: Option<String>,
    #[serde(default)]
    intermediate: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ConvertImagesOptions {
    files: Vec<String>,
    output_path: String,
    alpha: Option<String>,
    format: Option<String>,
    scale: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AnimeCutoutOptions {
    files: Vec<String>,
    output_path: String,
    model: Option<String>,
    #[serde(default)]
    refine_hair: bool,
    #[serde(default)]
    recover_details: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SuperResRunOptions {
    files: Vec<String>,
    output_path: String,
    /// "anime"（动漫超分）或 "general"（通用超分）。
    model: String,
    /// 目标倍率 2–8；缺省 4（模型原生倍率）。
    #[serde(default)]
    scale: Option<u32>,
}

/// 输出文件名统一为 `{stem}_{倍率}x_{模型id}.png`。
fn superres_output_name(stem: &str, model: &str, scale: u32) -> String {
    format!("{stem}_{scale}x_{model}.png")
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SystemStats {
    cpu_usage: f32,
    memory_used: u64,
    memory_total: u64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct GpuStats {
    available: bool,
    name: String,
    utilization: f32,
    memory_used: u64,
    memory_total: u64,
}

impl GpuStats {
    fn unavailable() -> Self {
        Self {
            available: false,
            name: String::new(),
            utilization: 0.0,
            memory_used: 0,
            memory_total: 0,
        }
    }
}

static MONITOR_SYSTEM: std::sync::OnceLock<std::sync::Mutex<sysinfo::System>> =
    std::sync::OnceLock::new();
static GPU_FAILURES: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
static GPU_LAST_PROBE_MS: std::sync::atomic::AtomicI64 = std::sync::atomic::AtomicI64::new(0);

#[tauri::command]
async fn system_stats() -> Result<SystemStats, String> {
    tauri::async_runtime::spawn_blocking(|| {
        let system =
            MONITOR_SYSTEM.get_or_init(|| std::sync::Mutex::new(sysinfo::System::new_all()));
        // CPU 占用需要两次采样之间的时间差。
        std::thread::sleep(sysinfo::MINIMUM_CPU_UPDATE_INTERVAL);
        let mut guard = system.lock().map_err(|error| error.to_string())?;
        guard.refresh_cpu_usage();
        guard.refresh_memory();
        Ok(SystemStats {
            cpu_usage: guard.global_cpu_usage(),
            memory_used: guard.used_memory(),
            memory_total: guard.total_memory(),
        })
    })
    .await
    .map_err(to_string_error)?
}

fn query_gpu_stats() -> GpuStats {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let output = std::process::Command::new("nvidia-smi")
        .args([
            "--query-gpu=name,utilization.gpu,memory.used,memory.total",
            "--format=csv,noheader,nounits",
        ])
        .creation_flags(CREATE_NO_WINDOW)
        .output();
    let Ok(output) = output else {
        return GpuStats::unavailable();
    };
    if !output.status.success() {
        return GpuStats::unavailable();
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let Some(line) = text.lines().next() else {
        return GpuStats::unavailable();
    };
    let fields: Vec<&str> = line.split(',').map(str::trim).collect();
    if fields.len() < 4 {
        return GpuStats::unavailable();
    }
    let mib_to_bytes =
        |value: &str| -> u64 { (value.parse::<f64>().unwrap_or(0.0) * 1024.0 * 1024.0) as u64 };
    GpuStats {
        available: true,
        name: fields[0].to_string(),
        utilization: fields[1].parse::<f32>().unwrap_or(0.0),
        memory_used: mib_to_bytes(fields[2]),
        memory_total: mib_to_bytes(fields[3]),
    }
}

#[tauri::command]
async fn gpu_stats() -> GpuStats {
    // 连续失败只做退避（60 秒重探一次），不再永久粘死「无 GPU」状态。
    use std::sync::atomic::Ordering;
    let failures = GPU_FAILURES.load(Ordering::Relaxed);
    if failures >= 5 {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|value| value.as_millis() as i64)
            .unwrap_or(0);
        let last = GPU_LAST_PROBE_MS.load(Ordering::Relaxed);
        if last > 0 && now - last < 60_000 {
            return GpuStats::unavailable();
        }
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|value| value.as_millis() as i64)
        .unwrap_or(0);
    GPU_LAST_PROBE_MS.store(now, Ordering::Relaxed);
    let stats = tauri::async_runtime::spawn_blocking(query_gpu_stats)
        .await
        .unwrap_or_else(|_| GpuStats::unavailable());
    if stats.available {
        GPU_FAILURES.store(0, Ordering::Relaxed);
    } else {
        GPU_FAILURES.fetch_add(1, Ordering::Relaxed);
    }
    stats
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SkinEntry {
    name: String,
    path: String,
    disabled: bool,
    file_count: u64,
    modified_at: u128,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ImportSkinOptions {
    sources: Vec<String>,
    target_directory: String,
}

#[derive(Debug, Serialize)]
struct ImportSkinResult {
    imported: usize,
    errors: Vec<String>,
}

#[derive(Debug, Serialize)]
struct PathResult {
    path: String,
}

#[derive(Debug, Serialize)]
struct DeleteResult {
    deleted: bool,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct TaskProgress {
    completed: usize,
    total: usize,
    message: String,
    /// 文件内细分进度（0–100）；缺省时前端按 completed/total 计算。
    #[serde(skip_serializing_if = "Option::is_none")]
    percent: Option<f64>,
}

#[derive(Debug, Clone)]
struct MipmapLevel {
    width: u32,
    height: u32,
    payload: Vec<u8>,
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .setup(|app| {
            let app_data = app
                .path()
                .app_data_dir()
                .map_err(|error| format!("Cannot resolve app data directory: {error}"))?;
            let settings_path = app_data.join("settings.json");
            app.manage(AppState { settings_path });

            // Apply dark title bar on Windows 10/11
            #[cfg(target_os = "windows")]
            if let Some(window) = app.get_webview_window("main") {
                if let Ok(hwnd) = window.hwnd() {
                    unsafe {
                        apply_dark_titlebar(hwnd.0);
                    }
                }
                window.show().map_err(to_string_error)?;
            }

            #[cfg(not(target_os = "windows"))]
            if let Some(window) = app.get_webview_window("main") {
                window.show().map_err(to_string_error)?;
            }

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            updater_check_mirror,
            settings_get,
            material_maps::material_maps_preview,
            material_maps::material_maps_generate,
            settings_set,
            texture_find_groups,
            texture_merge_pbr,
            texture_split_pbr,
            texture_create_mipmap,
            texture_convert_images_to_dds,
            anime_models_status,
            anime_model_download,
            anime_model_uninstall,
            anime_hair_refiner_status,
            anime_hair_refiner_download,
            anime_hair_refiner_uninstall,
            anime_cutout,
            superres_models_status,
            superres_model_download,
            superres_model_uninstall,
            superres_run,
            gpu_runtime_state,
            install_gpu_runtime,
            skin_auto_detect,
            skin_list,
            skin_import,
            skin_toggle,
            skin_delete,
            system_stats,
            gpu_stats
        ])
        .run(tauri::generate_context!())
        .expect("error while running AIAS");
}

#[cfg(target_os = "windows")]
unsafe fn apply_dark_titlebar(hwnd: *mut std::ffi::c_void) {
    #[link(name = "dwmapi")]
    extern "system" {
        fn DwmSetWindowAttribute(
            hwnd: *mut std::ffi::c_void,
            dwattribute: u32,
            pvattribute: *const std::ffi::c_void,
            cbattribute: u32,
        ) -> i32;
    }
    #[link(name = "uxtheme")]
    extern "system" {
        fn SetWindowTheme(
            hwnd: *mut std::ffi::c_void,
            subapp: *const u16,
            idlist: *const u16,
        ) -> i32;
    }
    // Windows 10 uses attribute 19; Windows 11 uses 20.
    const DWMWA_USE_IMMERSIVE_DARK_MODE_BEFORE_20H1: u32 = 19;
    const DWMWA_USE_IMMERSIVE_DARK_MODE: u32 = 20;
    let dark: i32 = 1;
    let _ = DwmSetWindowAttribute(
        hwnd,
        DWMWA_USE_IMMERSIVE_DARK_MODE_BEFORE_20H1,
        &dark as *const _ as *const _,
        4,
    );
    let _ = DwmSetWindowAttribute(
        hwnd,
        DWMWA_USE_IMMERSIVE_DARK_MODE,
        &dark as *const _ as *const _,
        4,
    );

    // Force dark window frame via uxtheme
    let dark_explorer: Vec<u16> = "DarkMode_Explorer"
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    let _ = SetWindowTheme(hwnd, dark_explorer.as_ptr(), std::ptr::null());
}

#[tauri::command]
fn settings_get(state: State<AppState>) -> Result<Settings, String> {
    load_settings(&state.settings_path)
}

#[tauri::command]
fn settings_set(state: State<AppState>, patch: serde_json::Value) -> Result<Settings, String> {
    let mut settings = load_settings(&state.settings_path)?;
    merge_settings(&mut settings, patch)?;
    save_settings(&state.settings_path, &settings)?;
    Ok(settings)
}

fn load_settings(path: &Path) -> Result<Settings, String> {
    if !path.exists() {
        return Ok(Settings::default());
    }
    let content = fs::read_to_string(path).map_err(to_string_error)?;
    serde_json::from_str::<Settings>(&content).or_else(|_| {
        let mut settings = Settings::default();
        if let Ok(value) = serde_json::from_str::<serde_json::Value>(&content) {
            merge_settings(&mut settings, value)?;
        }
        Ok(settings)
    })
}

fn save_settings(path: &Path, settings: &Settings) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(to_string_error)?;
    }
    let content = serde_json::to_string_pretty(settings).map_err(to_string_error)?;
    fs::write(path, format!("{content}\n")).map_err(to_string_error)
}

fn merge_settings(settings: &mut Settings, patch: serde_json::Value) -> Result<(), String> {
    let mut value = serde_json::to_value(settings.clone()).map_err(to_string_error)?;
    let target = value
        .as_object_mut()
        .ok_or_else(|| "设置结构无效。".to_string())?;
    if let Some(object) = patch.as_object() {
        for (key, patch_value) in object {
            target.insert(key.clone(), patch_value.clone());
        }
    }
    *settings = serde_json::from_value(value).map_err(to_string_error)?;
    Ok(())
}

#[tauri::command]
fn texture_find_groups(input_path: String) -> Result<Vec<TextureGroup>, String> {
    find_texture_groups(Path::new(&input_path))
}

#[tauri::command]
async fn texture_merge_pbr(app: AppHandle, options: MergePbrOptions) -> Result<TaskResult, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let _task = safety::task_guard()?;
        texture_merge_pbr_inner(&app, options)
    })
    .await
    .map_err(to_string_error)?
}

fn texture_merge_pbr_inner(
    app: &AppHandle,
    options: MergePbrOptions,
) -> Result<TaskResult, String> {
    require_directory(&options.input_path, "输入目录")?;
    fs::create_dir_all(&options.output_path).map_err(to_string_error)?;
    let groups = find_texture_groups(Path::new(&options.input_path))?;
    let mut logs = vec![format!("找到 {} 组完整 PBR 贴图。", groups.len())];
    let format = options.format.as_deref().unwrap_or("DXT5");
    let alpha = options.alpha.as_deref().unwrap_or("black");
    let scale = options.scale.as_deref().unwrap_or("none");
    let mut completed = 0;

    for group in &groups {
        let c_path = Path::new(&options.output_path).join(format!("{}_c.dds", group.prefix));
        let n_path = Path::new(&options.output_path).join(format!("{}_n.dds", group.prefix));
        process_base_color(
            Path::new(&group.files.basecolor),
            &c_path,
            alpha,
            format,
            scale,
        )?;
        process_roughness_metallic_normal(
            Path::new(&group.files.roughness),
            Path::new(&group.files.metallic),
            Path::new(&group.files.normal),
            &n_path,
            format,
            scale,
        )?;
        completed += 1;
        logs.push(format!("完成 {}", group.prefix));
        emit_task_progress(
            app,
            completed,
            groups.len(),
            format!("完成 {}", group.prefix),
        );
    }

    Ok(TaskResult {
        completed,
        total: groups.len(),
        logs,
        outputs: Vec::new(),
    })
}

#[tauri::command]
async fn texture_split_pbr(app: AppHandle, options: SplitPbrOptions) -> Result<TaskResult, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let _task = safety::task_guard()?;
        texture_split_pbr_inner(&app, options)
    })
    .await
    .map_err(to_string_error)?
}

fn texture_split_pbr_inner(
    app: &AppHandle,
    options: SplitPbrOptions,
) -> Result<TaskResult, String> {
    safety::unique_stems(&options.files)?;
    fs::create_dir_all(&options.output_path).map_err(to_string_error)?;
    let export_format = options.export_format.as_deref().unwrap_or("png");
    let export_alpha = options.export_alpha.unwrap_or(true);
    let scale = options.scale.as_deref().unwrap_or("none");
    let output_dir = Path::new(&options.output_path);
    let mut logs = Vec::new();
    let mut completed = 0;

    for file in &options.files {
        let file_path = Path::new(file);
        let stem = file_path
            .file_stem()
            .and_then(|value| value.to_str())
            .ok_or_else(|| format!("文件名无效：{file}"))?;
        let prefix = stem
            .trim_end_matches("_c")
            .trim_end_matches("_C")
            .trim_end_matches("_n")
            .trim_end_matches("_N");
        let image = dds_to_image(file_path)?;
        let rgba = apply_scale(image.to_rgba8(), scale);
        let (width, height) = rgba.dimensions();
        let lower = stem.to_lowercase();

        if lower.ends_with("_c") {
            let mut rgb = Vec::with_capacity((width * height * 3) as usize);
            let mut alpha = Vec::with_capacity((width * height) as usize);
            for pixel in rgba.pixels() {
                rgb.extend_from_slice(&[pixel[0], pixel[1], pixel[2]]);
                alpha.push(pixel[3]);
            }
            save_rgb_image(
                &rgb,
                width,
                height,
                output_dir.join(format!("{prefix}_BaseColor.{export_format}")),
                export_format,
            )?;
            if export_alpha {
                save_luma_image(
                    &alpha,
                    width,
                    height,
                    output_dir.join(format!("{prefix}_Alpha.{export_format}")),
                    export_format,
                )?;
            }
            logs.push(format!(
                "拆分 {stem}: BaseColor{}",
                if export_alpha { " / Alpha" } else { "" }
            ));
        } else if lower.ends_with("_n") {
            let mut roughness = Vec::with_capacity((width * height) as usize);
            let mut metallic = Vec::with_capacity((width * height) as usize);
            let mut normal = RgbaImage::new(width, height);
            for (x, y, pixel) in rgba.enumerate_pixels() {
                roughness.push(255 - pixel[0]);
                metallic.push(pixel[2]);
                normal.put_pixel(x, y, Rgba([pixel[3], pixel[1], 255, 255]));
            }
            save_luma_image(
                &roughness,
                width,
                height,
                output_dir.join(format!("{prefix}_Roughness.{export_format}")),
                export_format,
            )?;
            save_luma_image(
                &metallic,
                width,
                height,
                output_dir.join(format!("{prefix}_Metallic.{export_format}")),
                export_format,
            )?;
            save_dynamic_image(
                &DynamicImage::ImageRgba8(normal),
                output_dir.join(format!("{prefix}_Normal.{export_format}")),
                export_format,
            )?;
            logs.push(format!("拆分 {stem}: Roughness / Metallic / Normal"));
        }
        completed += 1;
        emit_task_progress(app, completed, options.files.len(), format!("完成 {stem}"));
    }

    Ok(TaskResult {
        completed,
        total: options.files.len(),
        logs,
        outputs: Vec::new(),
    })
}

#[tauri::command]
async fn texture_create_mipmap(
    app: AppHandle,
    options: MipmapOptions,
) -> Result<TaskResult, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let _task = safety::task_guard()?;
        texture_create_mipmap_inner(&app, options)
    })
    .await
    .map_err(to_string_error)?
}

fn texture_create_mipmap_inner(
    app: &AppHandle,
    options: MipmapOptions,
) -> Result<TaskResult, String> {
    require_directory(&options.input_path, "输入目录")?;
    fs::create_dir_all(&options.output_path).map_err(to_string_error)?;
    let alpha = options.alpha.as_deref().unwrap_or("keep");
    let format = options.format.as_deref().unwrap_or("DXT5");
    let scale = options.scale.as_deref().unwrap_or("none");
    if options.intermediate {
        let input = image_exts().iter().map(|ext| Path::new(&options.input_path).join(format!("p0{ext}")))
            .find(|path| path.is_file()).ok_or("实验模式需要 p0 原图")?;
        let (w, h) = image::image_dimensions(&input).map_err(to_string_error)?;
        safety::memory_budget(w, h, 64)?;
        let base = prepare_image(&input, alpha, scale)?;
        let mut outputs = Vec::new();
        for (index, intermediate) in [false, true].into_iter().enumerate() {
            emit_task_progress_percent(app, index, 2, if intermediate { "生成中间尺寸预滤波链" } else { "生成直接缩小对照链" }.into(), Some(index as f64 * 50.0));
            let levels = generate_experimental_mips(&base, intermediate);
            let name = if intermediate { "Mipmap_intermediate.dds" } else { "Mipmap_reference.dds" };
            let output = Path::new(&options.output_path).join(name);
            write_dds_with_mipmaps(&levels, &output, format)?;
            outputs.push(output.display().to_string());
        }
        return Ok(TaskResult { completed: 2, total: 2, logs: vec![
            "实验模式仅使用 p0，自动生成到 1×1；p1 等自定义层不参与。".into(),
            "Mipmap_reference.dds：直接缩小对照；Mipmap_intermediate.dds：每层先缩至 75% 再缩至 50%。".into(),
            "两份 DDS 均使用标准减半层级；不改变游戏视距或采样器设置。".into(),
        ], outputs });
    }
    let mut files = Vec::new();

    for index in 0..1000 {
        if let Some(path) = image_exts()
            .iter()
            .map(|ext| Path::new(&options.input_path).join(format!("p{index}{ext}")))
            .find(|path| path.exists())
        {
            files.push(path);
        }
    }

    if files.is_empty() {
        return Err("未找到 p0、p1、p2... mipmap 文件。".into());
    }

    let mut images = Vec::new();
    for (index, file) in files.iter().enumerate() {
        let image = prepare_image(file, alpha, scale)?;
        validate_mipmap_dimensions(&image, images.first(), index as u32)?;
        images.push(image);
        emit_task_progress(
            app,
            index + 1,
            files.len(),
            format!(
                "处理 {}",
                file.file_name()
                    .and_then(|value| value.to_str())
                    .unwrap_or("mipmap")
            ),
        );
    }

    let output_file = Path::new(&options.output_path).join("Mipmap.dds");
    write_dds_with_mipmaps(&images, &output_file, format)?;
    Ok(TaskResult {
        completed: files.len(),
        total: files.len(),
        logs: vec![format!("生成 {}", output_file.display())],
        outputs: Vec::new(),
    })
}

#[tauri::command]
async fn texture_convert_images_to_dds(
    app: AppHandle,
    options: ConvertImagesOptions,
) -> Result<TaskResult, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let _task = safety::task_guard()?;
        texture_convert_images_to_dds_inner(&app, options)
    })
    .await
    .map_err(to_string_error)?
}

fn texture_convert_images_to_dds_inner(
    app: &AppHandle,
    options: ConvertImagesOptions,
) -> Result<TaskResult, String> {
    safety::unique_stems(&options.files)?;
    fs::create_dir_all(&options.output_path).map_err(to_string_error)?;
    let alpha = options.alpha.as_deref().unwrap_or("keep");
    let format = options.format.as_deref().unwrap_or("DXT5");
    let scale = options.scale.as_deref().unwrap_or("none");
    let mut logs = Vec::new();

    for file in &options.files {
        let input = Path::new(file);
        let output_file = Path::new(&options.output_path)
            .join(
                input
                    .file_stem()
                    .and_then(|value| value.to_str())
                    .unwrap_or("output"),
            )
            .with_extension("dds");
        image_to_dds(input, &output_file, alpha, format, scale)?;
        logs.push(format!(
            "转换 {} -> {}",
            input
                .file_name()
                .and_then(|value| value.to_str())
                .unwrap_or(file),
            output_file
                .file_name()
                .and_then(|value| value.to_str())
                .unwrap_or("output.dds")
        ));
        emit_task_progress(
            app,
            logs.len(),
            options.files.len(),
            format!(
                "完成 {}",
                input
                    .file_name()
                    .and_then(|value| value.to_str())
                    .unwrap_or(file)
            ),
        );
    }

    Ok(TaskResult {
        completed: options.files.len(),
        total: options.files.len(),
        logs,
        outputs: Vec::new(),
    })
}

// ---------------------------------------------------------------------------
// 动漫抠图：本地 ONNX 推理 + 模型下载/卸载管理
// ---------------------------------------------------------------------------

fn anime_base_dir(app: &AppHandle) -> Result<PathBuf, String> {
    app.path()
        .app_data_dir()
        .map_err(|error| format!("无法定位应用数据目录：{error}"))
}

#[tauri::command]
fn anime_models_status(app: AppHandle) -> Result<Vec<anime::ModelStatus>, String> {
    let base = anime_base_dir(&app)?;
    Ok(anime::models_status(&base))
}

#[tauri::command]
fn anime_hair_refiner_status(app: AppHandle) -> Result<anime::ModelStatus, String> {
    Ok(anime::hair_refiner_status(&anime_base_dir(&app)?))
}

#[tauri::command]
async fn anime_hair_refiner_download(app: AppHandle) -> Result<anime::ModelStatus, String> {
    let base = anime_base_dir(&app)?;
    tauri::async_runtime::spawn_blocking(move || {
        anime::download_hair_refiner(Some(&app), &base)?;
        Ok(anime::hair_refiner_status(&base))
    })
    .await
    .map_err(to_string_error)?
}

#[tauri::command]
async fn anime_hair_refiner_uninstall(app: AppHandle) -> Result<anime::ModelStatus, String> {
    let base = anime_base_dir(&app)?;
    tauri::async_runtime::spawn_blocking(move || {
        anime::uninstall_hair_refiner(&base)?;
        Ok(anime::hair_refiner_status(&base))
    })
    .await
    .map_err(to_string_error)?
}

#[tauri::command]
async fn anime_model_download(
    app: AppHandle,
    model_id: String,
) -> Result<Vec<anime::ModelStatus>, String> {
    let base = anime_base_dir(&app)?;
    tauri::async_runtime::spawn_blocking(move || {
        anime::download_model(Some(&app), &base, &model_id)?;
        Ok(anime::models_status(&base))
    })
    .await
    .map_err(to_string_error)?
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct GpuRuntimeState {
    nvidia_gpu: bool,
    runtime_installed: bool,
    ort_initialized: bool,
    cuda_active: bool,
}

#[tauri::command]
async fn gpu_runtime_state(app: AppHandle) -> Result<GpuRuntimeState, String> {
    let base = anime_base_dir(&app)?;
    let nvidia_gpu = tauri::async_runtime::spawn_blocking(query_gpu_stats)
        .await
        .map_err(to_string_error)?
        .available;
    let runtime_installed = anime::gpu_ort_ready(&base);
    let ort_initialized = anime::ort_initialized();
    // cuda_active 需要 ORT 已加载才准确；未初始化时不强行加载 dll。
    let cuda_active = ort_initialized && anime::cuda_ep_compiled();
    Ok(GpuRuntimeState {
        nvidia_gpu,
        runtime_installed,
        ort_initialized,
        cuda_active,
    })
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct GpuRuntimeInstallResult {
    requires_restart: bool,
}

#[tauri::command]
async fn install_gpu_runtime(app: AppHandle) -> Result<GpuRuntimeInstallResult, String> {
    let base = anime_base_dir(&app)?;
    let already_installed = anime::gpu_ort_ready(&base);
    // 若本会话已经加载过 CPU 版 ORT，新装的 GPU dll 要重启应用才会生效。
    let was_initialized = anime::ort_initialized();
    tauri::async_runtime::spawn_blocking(move || anime::install_gpu_ort(Some(&app), &base))
        .await
        .map_err(to_string_error)??;
    Ok(GpuRuntimeInstallResult {
        requires_restart: was_initialized && !already_installed,
    })
}

#[tauri::command]
async fn anime_model_uninstall(
    app: AppHandle,
    model_id: String,
) -> Result<Vec<anime::ModelStatus>, String> {
    let base = anime_base_dir(&app)?;
    tauri::async_runtime::spawn_blocking(move || {
        anime::uninstall_model(&base, &model_id)?;
        Ok(anime::models_status(&base))
    })
    .await
    .map_err(to_string_error)?
}

fn anime_supported_extension(extension: &str) -> bool {
    matches!(extension, "png" | "jpg" | "jpeg" | "webp" | "tga")
}

fn anime_cutout_inner(
    app: Option<&AppHandle>,
    options: AnimeCutoutOptions,
) -> Result<TaskResult, String> {
    safety::unique_stems(&options.files)?;
    fs::create_dir_all(&options.output_path).map_err(to_string_error)?;
    let base = match app {
        Some(handle) => anime_base_dir(handle)?,
        None => dirs::data_dir()
            .map(|dir| dir.join("studio.avroracl.aias"))
            .ok_or_else(|| "无法定位应用数据目录".to_string())?,
    };
    let model_id = options
        .model
        .as_deref()
        .unwrap_or("anime-specialist")
        .trim()
        .to_string();
    if !anime::is_model_ready(&base, &model_id) {
        return Err("抠图模型未安装，请先在「抠图模型」中下载。".into());
    }
    if options.refine_hair && model_id != "anime-specialist" {
        return Err("精细发丝边缘目前仅支持动漫专精（AnimeSeg）。".into());
    }
    if options.refine_hair && !anime::is_hair_refiner_ready(&base) {
        return Err("请先在右侧栏下载精细发丝边缘模型。".into());
    }
    if options.recover_details && model_id != "anime-specialist" {
        return Err("高分辨率细节补全目前仅支持动漫专精（AnimeSeg）。".into());
    }
    anime::ensure_ort_runtime(&base)?;

    let mut logs = Vec::new();
    logs.push(if anime::cuda_ep_compiled() {
        "推理后端：CUDA（GPU 加速）".to_string()
    } else if anime::gpu_ort_ready(&base) {
        "推理后端：CPU（GPU 运行库未生效，重启应用后再试）".to_string()
    } else {
        "推理后端：CPU（检测到 NVIDIA 显卡时可在「GPU 加速」中下载运行库）".to_string()
    });
    if options.recover_details {
        logs.push("实验功能：已启用高分辨率细节补全（双局部裁切一致时才补回边缘）。".to_string());
    }
    let mut outputs = Vec::new();
    let mut completed = 0usize;

    for file in &options.files {
        let input = Path::new(file);
        if !input.exists() {
            logs.push(format!("跳过（文件不存在）：{file}"));
            continue;
        }
        let Some(stem) = input.file_stem().and_then(|value| value.to_str()) else {
            logs.push(format!("跳过（文件名无效）：{file}"));
            continue;
        };
        let extension = input
            .extension()
            .and_then(|value| value.to_str())
            .map(|value| value.to_lowercase())
            .unwrap_or_default();
        if !anime_supported_extension(&extension) {
            logs.push(format!("跳过（暂不支持 {extension} 格式）：{stem}"));
            continue;
        }

        // 用请求的模型 id 先成临时名；若 ToonOut 回退，再按实际模型改名。
        let suffix = match (options.recover_details, options.refine_hair) {
            (true, true) => "_detail-hair",
            (true, false) => "_detail",
            (false, true) => "_hair",
            (false, false) => "",
        };
        let target = Path::new(&options.output_path).join(format!("{stem}_{model_id}{suffix}.png"));
        let label = input
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or(file);
        if let Some(handle) = app {
            emit_task_progress_percent(
                handle,
                completed,
                options.files.len(),
                format!("推理中 {label}"),
                Some(completed as f64 / options.files.len() as f64 * 100.0),
            );
        }

        // 单图内部按管线阶段细分：把已完成的文件数 + 当前文件的阶段比例
        // 折算成整体百分比，进度条在单张图推理期间也能真实移动。
        let phase_progress = |fraction: f64, phase: &str| {
            if let Some(handle) = app {
                let percent =
                    (completed as f64 + fraction.clamp(0.0, 1.0)) / options.files.len() as f64 * 100.0;
                emit_task_progress_percent(
                    handle,
                    completed,
                    options.files.len(),
                    format!("{phase} {label}"),
                    Some(percent),
                );
            }
        };

        match anime::cutout_with_options(
            &base,
            &model_id,
            input,
            &target,
            options.refine_hair,
            options.recover_details,
            &phase_progress,
        ) {
            Ok(outcome) => {
                completed += 1;
                // 回退时文件其实是 simple 抠的，把最终输出名统一为实际模型，便于前端
                // 用「原图 stem + 模型 id」匹配到结果，也避免残留 toonout 后缀的误导文件。
                let final_path = if outcome.fallback {
                    let actual = format!("{stem}_{}.png", outcome.model_used);
                    let path = Path::new(&options.output_path).join(&actual);
                    // 同一输入重复运行时允许以最新结果覆盖旧的实际模型文件；若改名
                    // 失败必须返回错误，不能悄悄把旧文件当成本次的回退结果展示给前端。
                    safety::atomic_write(&path, |writer| {
                        let mut source = fs::File::open(&target).map_err(to_string_error)?;
                        std::io::copy(&mut source, writer).map_err(to_string_error)?;
                        Ok(())
                    })?;
                    // The committed fallback result is valid even if cleanup fails.
                    let _ = fs::remove_file(&target);
                    let fallback_label = anime::model_label(&outcome.model_used);
                    logs.push(format!(
                        "完成 {} → {}（ToonOut 在此复杂背景上失效，已自动改用 {}）",
                        stem, actual, fallback_label
                    ));
                    path
                } else {
                    logs.push(format!(
                        "完成 {} → {}",
                        stem,
                        target
                            .file_name()
                            .and_then(|value| value.to_str())
                            .unwrap_or("output.png")
                    ));
                    target
                };
                outputs.push(final_path.display().to_string());
                if let Some(handle) = app {
                    emit_task_progress(
                        handle,
                        completed,
                        options.files.len(),
                        format!("完成 {stem}"),
                    );
                }
            }
            Err(error) => {
                logs.push(format!("失败 {stem}：{error}"));
            }
        }
    }

    if completed == 0 {
        let detail = logs.last().cloned().unwrap_or_default();
        return Err(if detail.is_empty() {
            "没有图片被处理。".into()
        } else {
            format!("没有图片被处理。{detail}")
        });
    }

    Ok(TaskResult {
        completed,
        total: options.files.len(),
        logs,
        outputs,
    })
}

#[tauri::command]
async fn anime_cutout(app: AppHandle, options: AnimeCutoutOptions) -> Result<TaskResult, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let _task = safety::task_guard()?;
        anime_cutout_inner(Some(&app), options)
    })
    .await
    .map_err(to_string_error)?
}

// ---------------------------------------------------------------------------
// AI 超分：立绘 / 素材 4x 放大（RealESRGAN 通用 + 动漫特化）
// ---------------------------------------------------------------------------

fn superres_base_dir(app: &AppHandle) -> Result<PathBuf, String> {
    app.path()
        .app_data_dir()
        .map_err(|error| format!("无法定位应用数据目录：{error}"))
}

fn superres_supported_extension(extension: &str) -> bool {
    matches!(extension, "png" | "jpg" | "jpeg" | "webp" | "tga")
}

#[tauri::command]
fn superres_models_status(app: AppHandle) -> Result<Vec<superres::SuperResModelStatus>, String> {
    let base = superres_base_dir(&app)?;
    Ok(superres::models_status(&base))
}

#[tauri::command]
async fn superres_model_download(app: AppHandle, model: String) -> Result<Vec<superres::SuperResModelStatus>, String> {
    let base = superres_base_dir(&app)?;
    tauri::async_runtime::spawn_blocking(move || {
        superres::download_model(Some(&app), &base, &model)?;
        Ok(superres::models_status(&base))
    })
    .await
    .map_err(to_string_error)?
}

#[tauri::command]
async fn superres_model_uninstall(app: AppHandle, model: String) -> Result<Vec<superres::SuperResModelStatus>, String> {
    let base = superres_base_dir(&app)?;
    tauri::async_runtime::spawn_blocking(move || {
        superres::uninstall_model(&base, &model)?;
        Ok(superres::models_status(&base))
    })
    .await
    .map_err(to_string_error)?
}

fn superres_run_inner(
    app: Option<&AppHandle>,
    options: SuperResRunOptions,
) -> Result<TaskResult, String> {
    safety::unique_stems(&options.files)?;
    fs::create_dir_all(&options.output_path).map_err(to_string_error)?;
    let base = match app {
        Some(handle) => superres_base_dir(handle)?,
        None => dirs::data_dir()
            .map(|dir| dir.join("studio.avroracl.aias"))
            .ok_or_else(|| "无法定位应用数据目录".to_string())?,
    };
    if !superres::is_model_ready(&base, &options.model) {
        return Err("超分模型未安装，请先在右侧栏下载。".into());
    }
    anime::ensure_ort_runtime(&base)?;

    let mut logs = Vec::new();
    logs.push(if anime::cuda_ep_compiled() {
        "推理后端：CUDA（GPU 加速）".to_string()
    } else {
        "推理后端：CPU（通用模型较慢，NVIDIA 显卡可在 AI 抠图页下载 GPU 运行库）".to_string()
    });
    let mut outputs = Vec::new();
    let mut completed = 0usize;
    let scale = options.scale.unwrap_or(4).clamp(2, 8);

    for (file_index, file) in options.files.iter().enumerate() {
        let input = Path::new(file);
        if !input.exists() {
            logs.push(format!("跳过（文件不存在）：{file}"));
            continue;
        }
        let Some(stem) = input.file_stem().and_then(|value| value.to_str()) else {
            logs.push(format!("跳过（文件名无效）：{file}"));
            continue;
        };
        let extension = input
            .extension()
            .and_then(|value| value.to_str())
            .map(|value| value.to_lowercase())
            .unwrap_or_default();
        if !superres_supported_extension(&extension) {
            logs.push(format!("跳过（暂不支持 {extension} 格式）：{stem}"));
            continue;
        }
        let target = Path::new(&options.output_path).join(superres_output_name(stem, &options.model, scale));
        let label = input
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or(file);
        if let Some(handle) = app {
            emit_task_progress_percent(
                handle,
                completed,
                options.files.len(),
                format!("超分中 {label}"),
                Some(file_index as f64 / options.files.len() as f64 * 100.0),
            );
        }
        // 图块级真实进度：每完成一个 256px 图块推理回调一次，把文件内比例
        // 折算进整体百分比（带 Alpha 的图两遍推理，单位数自动翻倍）。
        let highest_percent = std::cell::Cell::new(file_index as f64 / options.files.len() as f64 * 100.0);
        let tile_progress = |done_units: usize, total_units: usize, phase: &str| {
            if let Some(handle) = app {
                let fraction = if total_units > 0 {
                    done_units as f64 / total_units as f64
                } else {
                    0.0
                };
                let percent =
                    (file_index as f64 + fraction.clamp(0.0, 1.0)) / options.files.len() as f64 * 100.0;
                let percent = percent.max(highest_percent.get());
                highest_percent.set(percent);
                emit_task_progress_percent(
                    handle,
                    completed,
                    options.files.len(),
                    format!("第 {}/{} 张 · {label} · {phase}", file_index + 1, options.files.len()),
                    Some(percent),
                );
            }
        };
        match superres::upscale_with_progress(&base, &options.model, input, &target, scale, &tile_progress) {
            Ok(()) => {
                completed += 1;
                let name = target
                    .file_name()
                    .and_then(|value| value.to_str())
                    .unwrap_or("output.png")
                    .to_string();
                logs.push(format!("完成 {} → {}", stem, name));
                outputs.push(target.display().to_string());
                if let Some(handle) = app {
                    emit_task_progress_percent(
                        handle,
                        completed,
                        options.files.len(),
                        format!("完成 {stem}"),
                        Some((file_index + 1) as f64 / options.files.len() as f64 * 100.0),
                    );
                }
            }
            Err(error) => {
                logs.push(format!("失败 {stem}：{error}"));
            }
        }
    }

    if completed == 0 {
        let detail = logs.last().cloned().unwrap_or_default();
        return Err(if detail.is_empty() {
            "没有图片被处理。".into()
        } else {
            format!("没有图片被处理。{detail}")
        });
    }

    Ok(TaskResult {
        completed,
        total: options.files.len(),
        logs,
        outputs,
    })
}

#[tauri::command]
async fn superres_run(app: AppHandle, options: SuperResRunOptions) -> Result<TaskResult, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let _task = safety::task_guard()?;
        superres_run_inner(Some(&app), options)
    })
        .await
        .map_err(to_string_error)?
}

#[tauri::command]
fn skin_auto_detect() -> Result<Option<String>, String> {
    let Some(steam_path) = find_steam_path()? else {
        return Ok(None);
    };
    for library in find_steam_libraries(&steam_path)? {
        let candidate = library
            .join("steamapps")
            .join("common")
            .join("War Thunder")
            .join("UserSkins");
        if candidate.exists() {
            return Ok(Some(path_to_string(candidate)));
        }
    }
    Ok(None)
}

fn dir_size(path: &Path) -> u64 {
    let mut size = 0;
    if let Ok(entries) = fs::read_dir(path) {
        for entry in entries.flatten() {
            let path = entry.path();
            if entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                size += dir_size(&path);
            } else if let Ok(meta) = entry.metadata() {
                size += meta.len();
            }
        }
    }
    size
}

#[tauri::command]
fn skin_list(directory: String) -> Result<Vec<SkinEntry>, String> {
    require_directory(&directory, "涂装目录")?;
    let mut items = Vec::new();
    for entry in fs::read_dir(directory).map_err(to_string_error)? {
        let entry = entry.map_err(to_string_error)?;
        let metadata = entry.metadata().map_err(to_string_error)?;
        if !metadata.is_dir() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_string();
        let modified_at = metadata
            .modified()
            .ok()
            .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|duration| duration.as_millis())
            .unwrap_or_default();
        items.push(SkinEntry {
            disabled: name.ends_with(".disabled"),
            name,
            path: path_to_string(entry.path()),
            file_count: dir_size(&entry.path()),
            modified_at,
        });
    }
    items.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(items)
}

fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<(), String> {
    fs::create_dir_all(dst).map_err(to_string_error)?;
    for entry in fs::read_dir(src).map_err(to_string_error)? {
        let entry = entry.map_err(to_string_error)?;
        let target = dst.join(entry.file_name());
        if entry.file_type().map_err(to_string_error)?.is_dir() {
            copy_dir_recursive(&entry.path(), &target)?;
        } else {
            fs::copy(entry.path(), &target).map_err(to_string_error)?;
        }
    }
    Ok(())
}

#[tauri::command]
fn skin_import(options: ImportSkinOptions) -> Result<ImportSkinResult, String> {
    require_directory(&options.target_directory, "涂装目录")?;
    let mut imported = 0;
    let mut errors = Vec::new();
    for source_path in &options.sources {
        let source = Path::new(source_path);
        let Some(name) = source.file_name() else {
            errors.push(format!("无效路径: {source_path}"));
            continue;
        };
        let target = Path::new(&options.target_directory).join(name);
        if source.is_dir() {
            match copy_dir_recursive(source, &target) {
                Ok(()) => imported += 1,
                Err(e) => errors.push(format!("{name:?}: {e}")),
            }
        } else if source.is_file() {
            fs::copy(source, &target).map_err(to_string_error)?;
            imported += 1;
        }
    }
    Ok(ImportSkinResult { imported, errors })
}

#[tauri::command]
fn skin_toggle(file_path: String) -> Result<PathResult, String> {
    let source = Path::new(&file_path);
    if !source.exists() {
        return Err("文件不存在。".into());
    }
    let target = if file_path.ends_with(".disabled") {
        PathBuf::from(file_path.trim_end_matches(".disabled"))
    } else {
        PathBuf::from(format!("{file_path}.disabled"))
    };
    fs::rename(source, &target).map_err(to_string_error)?;
    Ok(PathResult {
        path: path_to_string(target),
    })
}

#[tauri::command]
fn skin_delete(file_path: String) -> Result<DeleteResult, String> {
    let source = Path::new(&file_path);
    eprintln!("skin_delete called with: {file_path}");
    if !source.exists() {
        eprintln!("skin_delete: path does not exist");
        return Ok(DeleteResult { deleted: false });
    }
    if source.is_dir() {
        eprintln!("skin_delete: removing directory");
        fs::remove_dir_all(source).map_err(|e| {
            let msg = format!("删除目录失败: {e}");
            eprintln!("{msg}");
            msg
        })?;
    } else {
        eprintln!("skin_delete: removing file");
        fs::remove_file(source).map_err(to_string_error)?;
    }
    Ok(DeleteResult { deleted: true })
}

fn find_texture_groups(folder: &Path) -> Result<Vec<TextureGroup>, String> {
    if !folder.exists() {
        return Ok(Vec::new());
    }
    let mut groups: BTreeMap<String, BTreeMap<String, String>> = BTreeMap::new();
    for entry in fs::read_dir(folder).map_err(to_string_error)? {
        let entry = entry.map_err(to_string_error)?;
        if !entry.file_type().map_err(to_string_error)?.is_file() {
            continue;
        }
        let path = entry.path();
        let Some(ext) = path
            .extension()
            .and_then(|value| value.to_str())
            .map(|value| format!(".{}", value.to_lowercase()))
        else {
            continue;
        };
        if !image_exts().contains(&ext.as_str()) {
            continue;
        }
        let Some(stem) = path.file_stem().and_then(|value| value.to_str()) else {
            continue;
        };
        let lower = stem.to_lowercase();
        for kind in ["basecolor", "roughness", "metallic", "normal"] {
            if lower.ends_with(kind) {
                let prefix = stem[..stem.len() - kind.len()]
                    .trim_end_matches(['_', '-', ' '])
                    .to_string();
                groups
                    .entry(prefix)
                    .or_default()
                    .insert(kind.into(), path_to_string(&path));
            }
        }
    }

    Ok(groups
        .into_iter()
        .filter_map(|(prefix, files)| {
            Some(TextureGroup {
                prefix,
                files: TextureGroupFiles {
                    basecolor: files.get("basecolor")?.clone(),
                    roughness: files.get("roughness")?.clone(),
                    metallic: files.get("metallic")?.clone(),
                    normal: files.get("normal")?.clone(),
                },
            })
        })
        .collect())
}

fn process_base_color(
    base_color: &Path,
    output: &Path,
    alpha: &str,
    format: &str,
    scale: &str,
) -> Result<(), String> {
    let mut image = apply_scale(
        image::open(base_color).map_err(to_string_error)?.to_rgba8(),
        scale,
    );
    let alpha_value = if alpha == "white" { 255 } else { 0 };
    for pixel in image.pixels_mut() {
        pixel[3] = alpha_value;
    }
    write_dds(&image, output, format)
}

fn emit_task_progress(app: &AppHandle, completed: usize, total: usize, message: String) {
    emit_task_progress_percent(app, completed, total, message, None);
}

/// percent：0–100 的整体百分比。批量任务把文件内的细分进度折算进总数，
/// 让单文件长时间推理时进度条也能连续移动。
fn emit_task_progress_percent(
    app: &AppHandle,
    completed: usize,
    total: usize,
    message: String,
    percent: Option<f64>,
) {
    let _ = app.emit(
        "task-progress",
        TaskProgress {
            completed,
            total,
            message,
            percent,
        },
    );
}

fn process_roughness_metallic_normal(
    roughness_path: &Path,
    metallic_path: &Path,
    normal_path: &Path,
    output: &Path,
    format: &str,
    scale: &str,
) -> Result<(), String> {
    let normal = apply_scale(
        image::open(normal_path)
            .map_err(to_string_error)?
            .to_rgba8(),
        scale,
    );
    let (width, height) = normal.dimensions();
    let roughness = image::open(roughness_path)
        .map_err(to_string_error)?
        .resize_exact(width, height, image::imageops::FilterType::Triangle)
        .to_luma8();
    let metallic = image::open(metallic_path)
        .map_err(to_string_error)?
        .resize_exact(width, height, image::imageops::FilterType::Triangle)
        .to_luma8();
    let mut combined = RgbaImage::new(width, height);
    for y in 0..height {
        for x in 0..width {
            let normal_pixel = normal.get_pixel(x, y);
            combined.put_pixel(
                x,
                y,
                Rgba([
                    255 - roughness.get_pixel(x, y)[0],
                    normal_pixel[1],
                    metallic.get_pixel(x, y)[0],
                    normal_pixel[0],
                ]),
            );
        }
    }
    write_dds(&combined, output, format)
}

fn image_to_dds(
    input: &Path,
    output: &Path,
    alpha: &str,
    format: &str,
    scale: &str,
) -> Result<(), String> {
    let image = prepare_image(input, alpha, scale)?;
    write_dds(&image, output, format)
}

fn apply_scale(image: RgbaImage, scale: &str) -> RgbaImage {
    let target = match scale {
        "6k" => 6144,
        "4k" => 4096,
        _ => return image,
    };
    let (width, height) = image.dimensions();
    let longest = width.max(height);
    if longest == 0 || longest <= target {
        return image;
    }
    let new_width = ((width as f64 * target as f64) / longest as f64).round() as u32;
    let new_height = ((height as f64 * target as f64) / longest as f64).round() as u32;
    image::imageops::resize(
        &image,
        new_width.max(1),
        new_height.max(1),
        image::imageops::FilterType::Lanczos3,
    )
}

fn prepare_image(input: &Path, alpha: &str, scale: &str) -> Result<RgbaImage, String> {
    let (w, h) = image::image_dimensions(input).map_err(to_string_error)?;
    safety::memory_budget(w, h, 64)?;
    let mut image = apply_scale(
        image::open(input).map_err(to_string_error)?.to_rgba8(),
        scale,
    );
    if alpha == "black" || alpha == "white" {
        let alpha_value = if alpha == "white" { 255 } else { 0 };
        for pixel in image.pixels_mut() {
            pixel[3] = alpha_value;
        }
    }
    Ok(image)
}

fn is_rgba8_format(format: &str) -> bool {
    format == "8.8.8.8" || format == "R8G8B8A8_UNORM"
}

fn encode_image_payload(image: &RgbaImage, format: &str) -> Vec<u8> {
    if is_rgba8_format(format) {
        return image.as_raw().clone();
    }
    let mut payload =
        vec![0; BcFormat::Bc3.compressed_size(image.width() as usize, image.height() as usize)];
    BcFormat::Bc3.compress(
        image.as_raw(),
        image.width() as usize,
        image.height() as usize,
        BcParams {
            algorithm: Algorithm::RangeFit,
            ..BcParams::default()
        },
        &mut payload,
    );
    payload
}

fn build_dds(levels: &[MipmapLevel], format: &str) -> Result<Vec<u8>, String> {
    let first = levels
        .first()
        .ok_or_else(|| "没有可写入的 mipmap 层级。".to_string())?;
    let compressed = !is_rgba8_format(format);
    let mut header = vec![0_u8; 128];
    header[0..4].copy_from_slice(b"DDS ");
    write_u32(&mut header, 4, 124);
    write_u32(
        &mut header,
        8,
        0x1 | 0x2 | 0x4 | 0x1000 | 0x20000 | if compressed { 0x80000 } else { 0x8 },
    );
    write_u32(&mut header, 12, first.height);
    write_u32(&mut header, 16, first.width);
    write_u32(
        &mut header,
        20,
        if compressed {
            calculate_bc3_size(first.width, first.height)
        } else {
            first.width * 4
        },
    );
    write_u32(&mut header, 28, levels.len() as u32);
    write_u32(&mut header, 76, 32);
    if compressed {
        write_u32(&mut header, 80, 0x4);
        header[84..88].copy_from_slice(b"DXT5");
    } else {
        write_u32(&mut header, 80, 0x40 | 0x1);
        write_u32(&mut header, 88, 32);
        write_u32(&mut header, 92, 0x000000ff);
        write_u32(&mut header, 96, 0x0000ff00);
        write_u32(&mut header, 100, 0x00ff0000);
        write_u32(&mut header, 104, 0xff000000);
    }
    write_u32(
        &mut header,
        108,
        0x1000 | if levels.len() > 1 { 0x400000 | 0x8 } else { 0 },
    );
    for level in levels {
        header.extend_from_slice(&level.payload);
    }
    Ok(header)
}

fn encode_dds(image: &RgbaImage, format: &str) -> Result<Vec<u8>, String> {
    build_dds(
        &[MipmapLevel {
            width: image.width(),
            height: image.height(),
            payload: encode_image_payload(image, format),
        }],
        format,
    )
}

fn write_dds(image: &RgbaImage, output: &Path, format: &str) -> Result<(), String> {
    let bytes = encode_dds(image, format)?;
    safety::atomic_write(output, |writer| { use std::io::Write; writer.write_all(&bytes).map_err(to_string_error) })
}

// Work in linear light with premultiplied alpha, avoiding gamma darkening
// and color leakage from invisible texels during either comparison path.
fn resize_mip_color(image: &RgbaImage, w: u32, h: u32) -> RgbaImage {
    let linear = image::Rgba32FImage::from_fn(image.width(), image.height(), |x, y| {
        let p = image.get_pixel(x, y); let a = p[3] as f32 / 255.0;
        image::Rgba(std::array::from_fn(|c| if c == 3 { a } else {
            let v = p[c] as f32 / 255.0;
            (if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }) * a
        }))
    });
    let resized = image::imageops::resize(&linear, w, h, image::imageops::FilterType::Triangle);
    RgbaImage::from_fn(w, h, |x, y| {
        let p = resized.get_pixel(x, y); let a = p[3].clamp(0.0, 1.0);
        image::Rgba(std::array::from_fn(|c| {
            let v = if c == 3 { a } else if a <= 1e-6 { 0.0 } else {
                let v = (p[c] / a).clamp(0.0, 1.0);
                if v <= 0.0031308 { v * 12.92 } else { 1.055 * v.powf(1.0 / 2.4) - 0.055 }
            };
            (v * 255.0).round() as u8
        }))
    })
}

fn generate_experimental_mips(base: &RgbaImage, intermediate: bool) -> Vec<RgbaImage> {
    let mut levels = vec![base.clone()];
    while levels.last().unwrap().dimensions() != (1, 1) {
        let previous = levels.last().unwrap();
        let (w, h) = previous.dimensions();
        let target = ((w / 2).max(1), (h / 2).max(1));
        let next = if intermediate {
            let bridge = resize_mip_color(previous, (w - w / 4).max(1), (h - h / 4).max(1));
            resize_mip_color(&bridge, target.0, target.1)
        } else { resize_mip_color(previous, target.0, target.1) };
        levels.push(next);
    }
    levels
}

fn validate_mipmap_dimensions(
    image: &RgbaImage,
    base: Option<&RgbaImage>,
    level: u32,
) -> Result<(), String> {
    let Some(base) = base else {
        return Ok(());
    };
    let expected_width = mip_dimension(base.width(), level);
    let expected_height = mip_dimension(base.height(), level);
    if image.width() != expected_width || image.height() != expected_height {
        return Err(format!(
            "p{level} 尺寸应为 {expected_width}x{expected_height}，实际为 {}x{}。",
            image.width(),
            image.height()
        ));
    }
    Ok(())
}

fn write_dds_with_mipmaps(images: &[RgbaImage], output: &Path, format: &str) -> Result<(), String> {
    let levels = images
        .iter()
        .map(|image| MipmapLevel {
            width: image.width(),
            height: image.height(),
            payload: encode_image_payload(image, format),
        })
        .collect::<Vec<_>>();
    let bytes = build_dds(&levels, format)?;
    safety::atomic_write(output, |writer| { use std::io::Write; writer.write_all(&bytes).map_err(to_string_error) })
}

fn dds_to_image(dds_path: &Path) -> Result<DynamicImage, String> {
    let file = fs::File::open(dds_path).map_err(to_string_error)?;
    let dds = Dds::read(&mut BufReader::new(file)).map_err(to_string_error)?;
    image_from_dds(&dds, 0)
        .map(DynamicImage::ImageRgba8)
        .map_err(to_string_error)
}

fn save_rgb_image(
    bytes: &[u8],
    width: u32,
    height: u32,
    output: PathBuf,
    format: &str,
) -> Result<(), String> {
    let image = image::RgbImage::from_raw(width, height, bytes.to_vec())
        .ok_or_else(|| "RGB 数据无效。".to_string())?;
    save_dynamic_image(&DynamicImage::ImageRgb8(image), output, format)
}

fn save_luma_image(
    bytes: &[u8],
    width: u32,
    height: u32,
    output: PathBuf,
    format: &str,
) -> Result<(), String> {
    let image: ImageBuffer<Luma<u8>, Vec<u8>> =
        ImageBuffer::from_raw(width, height, bytes.to_vec())
            .ok_or_else(|| "灰度数据无效。".to_string())?;
    save_dynamic_image(&DynamicImage::ImageLuma8(image), output, format)
}

fn save_dynamic_image(image: &DynamicImage, output: PathBuf, format: &str) -> Result<(), String> {
    let format = if format.eq_ignore_ascii_case("tga") { image::ImageFormat::Tga } else { image::ImageFormat::Png };
    safety::atomic_write(&output, |writer| image.write_to(writer, format).map_err(to_string_error))
}

fn find_steam_path() -> Result<Option<PathBuf>, String> {
    if cfg!(target_os = "windows") {
        if let Ok(output) = Command::new("reg")
            .args(["query", "HKCU\\Software\\Valve\\Steam", "/v", "SteamPath"])
            .output()
        {
            let stdout = String::from_utf8_lossy(&output.stdout);
            for line in stdout.lines() {
                if line.contains("SteamPath") {
                    if let Some(path) = line.split_whitespace().last() {
                        let path = PathBuf::from(path);
                        if path.exists() {
                            return Ok(Some(path));
                        }
                    }
                }
            }
        }
    }
    for candidate in [
        "C:\\Program Files (x86)\\Steam",
        "C:\\Program Files\\Steam",
        "D:\\Steam",
        "E:\\Steam",
    ] {
        let path = PathBuf::from(candidate);
        if path.exists() {
            return Ok(Some(path));
        }
    }
    Ok(None)
}

fn find_steam_libraries(steam_path: &Path) -> Result<Vec<PathBuf>, String> {
    let mut libraries = vec![steam_path.to_path_buf()];
    let vdf_path = steam_path.join("steamapps").join("libraryfolders.vdf");
    if !vdf_path.exists() {
        return Ok(libraries);
    }
    let content = fs::read_to_string(vdf_path).map_err(to_string_error)?;
    for line in content.lines() {
        let trimmed = line.trim();
        if !trimmed.starts_with("\"path\"") {
            continue;
        }
        let parts: Vec<_> = trimmed.split('"').collect();
        if parts.len() >= 4 {
            let path = PathBuf::from(parts[3].replace("\\\\", "\\"));
            if path.exists() && !libraries.contains(&path) {
                libraries.push(path);
            }
        }
    }
    Ok(libraries)
}

fn require_directory(directory: &str, label: &str) -> Result<(), String> {
    let path = Path::new(directory);
    if directory.is_empty() || !path.exists() || !path.is_dir() {
        Err(format!("{label}无效。"))
    } else {
        Ok(())
    }
}

fn image_exts() -> Vec<&'static str> {
    vec![".png", ".tga", ".jpg", ".jpeg"]
}

fn write_u32(buffer: &mut [u8], offset: usize, value: u32) {
    buffer[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

fn calculate_bc3_size(width: u32, height: u32) -> u32 {
    width.div_ceil(4) * height.div_ceil(4) * 16
}

fn path_to_string(path: impl AsRef<Path>) -> String {
    path.as_ref().to_string_lossy().to_string()
}

fn to_string_error(error: impl std::fmt::Display) -> String {
    error.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn anime_experimental_options_are_opt_in_and_backward_compatible() {
        let legacy: AnimeCutoutOptions =
            serde_json::from_str(r#"{"files":[],"outputPath":"out","model":"anime-specialist"}"#)
                .unwrap();
        assert!(!legacy.refine_hair);
        assert!(!legacy.recover_details);
        let enabled: AnimeCutoutOptions = serde_json::from_str(r#"{"files":[],"outputPath":"out","model":"anime-specialist","refineHair":true,"recoverDetails":true}"#).unwrap();
        assert!(enabled.refine_hair);
        assert!(enabled.recover_details);
        assert!(!Settings::default().anime_hair_refiner);
        assert!(!Settings::default().anime_detail_recovery);
    }

    fn sample_image(width: u32, height: u32) -> RgbaImage {
        RgbaImage::from_fn(width, height, |x, y| {
            Rgba([
                (x * 37 % 256) as u8,
                (y * 59 % 256) as u8,
                ((x + y) * 23 % 256) as u8,
                255,
            ])
        })
    }

    #[test]
    fn rgba8_dds_round_trip_preserves_pixels() {
        let image = sample_image(4, 4);
        let dds = Dds::read(&mut Cursor::new(encode_dds(&image, "8.8.8.8").unwrap())).unwrap();
        let decoded = image_from_dds(&dds, 0).unwrap();
        assert_eq!(decoded, image);
    }

    #[test]
    fn bc3_dds_round_trip_preserves_dimensions() {
        let image = sample_image(8, 8);
        let dds = Dds::read(&mut Cursor::new(encode_dds(&image, "DXT5").unwrap())).unwrap();
        let decoded = image_from_dds(&dds, 0).unwrap();
        assert_eq!(decoded.dimensions(), image.dimensions());
    }

    #[test]
    #[ignore = "manual intermediate-size experiment in test area"]
    fn intermediate_mipmap_experiment() {
        let input = PathBuf::from(std::env::var_os("AIAS_MIP_INPUT").expect("input"));
        let out = PathBuf::from(std::env::var_os("AIAS_MIP_OUTPUT").expect("output"));
        fs::create_dir_all(&out).unwrap();
        let source = image::open(input).unwrap().to_rgba8();
        let mut images = Vec::new();
        for width in [1024, 768, 512, 384, 256] {
            let height = (source.height() as u64 * width as u64 / source.width() as u64).max(1) as u32;
            let resized = image::imageops::resize(&source, width, height, image::imageops::FilterType::Lanczos3);
            resized.save(out.join(format!("level-{width}.png"))).unwrap();
            images.push(resized);
        }
        let mut report = String::new();
        for format in ["8.8.8.8", "DXT5"] {
            let file = out.join(format!("EXPERIMENT_INVALID_CHAIN-{format}.dds"));
            write_dds_with_mipmaps(&images, &file, format).unwrap();
            let dds = Dds::read(&mut BufReader::new(fs::File::open(file).unwrap())).unwrap();
            for level in 0..images.len() {
                match image_from_dds(&dds, level as u32) {
                    Ok(decoded) => {
                        report.push_str(&format!("{format} level {level}: supplied {:?}, decoded {:?}\n", images[level].dimensions(), decoded.dimensions()));
                        if level == 1 {
                            assert_ne!(decoded.dimensions(), images[level].dimensions());
                            decoded.save(out.join(format!("misread-level1-{format}.png"))).unwrap();
                        }
                    }
                    Err(error) => report.push_str(&format!("{format} level {level}: error {error}\n")),
                }
            }
        }
        fs::write(out.join("decoder-results.txt"), &report).unwrap();
        println!("{report}");
    }

    #[test]
    fn experimental_mips_keep_standard_dimensions_and_alpha() {
        let mut base = RgbaImage::from_pixel(17, 9, Rgba([255, 0, 0, 255]));
        for y in 0..9 { for x in 8..17 { base.put_pixel(x, y, Rgba([0, 0, 255, 0])); } }
        for intermediate in [false, true] {
            let levels = generate_experimental_mips(&base, intermediate);
            assert_eq!(levels.last().unwrap().dimensions(), (1, 1));
            for (index, level) in levels.iter().enumerate() {
                validate_mipmap_dimensions(level, Some(&base), index as u32).unwrap();
                if index > 0 { for p in level.pixels().filter(|p| p[3] > 0) { assert_eq!(p[2], 0, "invisible blue must not bleed"); } }
            }
            for format in ["8.8.8.8", "DXT5"] {
                let encoded = levels.iter().map(|im| MipmapLevel { width: im.width(), height: im.height(), payload: encode_image_payload(im, format) }).collect::<Vec<_>>();
                let dds = Dds::read(&mut Cursor::new(build_dds(&encoded, format).unwrap())).unwrap();
                for (index, level) in levels.iter().enumerate() { assert_eq!(image_from_dds(&dds, index as u32).unwrap().dimensions(), level.dimensions()); }
            }
        }
    }

    #[test]
    #[ignore = "manual game-ready pair in test area"]
    fn export_experimental_mip_pair() {
        let input = PathBuf::from(std::env::var_os("AIAS_MIP_INPUT").unwrap());
        let out = PathBuf::from(std::env::var_os("AIAS_MIP_OUTPUT").unwrap());
        fs::create_dir_all(&out).unwrap();
        let base = image::open(input).unwrap().to_rgba8();
        for intermediate in [false, true] {
            let levels = generate_experimental_mips(&base, intermediate);
            let file = out.join(if intermediate { "Mipmap_intermediate.dds" } else { "Mipmap_reference.dds" });
            write_dds_with_mipmaps(&levels, &file, "DXT5").unwrap();
            let dds = Dds::read(&mut BufReader::new(fs::File::open(file).unwrap())).unwrap();
            for (i, level) in levels.iter().enumerate() {
                let decoded = image_from_dds(&dds, i as u32).unwrap();
                assert_eq!(decoded.dimensions(), level.dimensions());
            }
            println!("intermediate={intermediate}, levels={}, base={:?}", levels.len(), base.dimensions());
        }
    }

    #[test]
    fn supplied_mipmaps_are_encoded_as_separate_levels() {
        let base = sample_image(4, 4);
        let mip = sample_image(2, 2);
        let dds = Dds::read(&mut Cursor::new(
            build_dds(
                &[
                    MipmapLevel {
                        width: 4,
                        height: 4,
                        payload: base.into_raw(),
                    },
                    MipmapLevel {
                        width: 2,
                        height: 2,
                        payload: mip.into_raw(),
                    },
                ],
                "8.8.8.8",
            )
            .unwrap(),
        ))
        .unwrap();
        assert_eq!(image_from_dds(&dds, 1).unwrap().dimensions(), (2, 2));
    }

    #[test]
    fn scale_option_downscales_longest_side_only() {
        let scaled = apply_scale(sample_image(8192, 4096), "4k");
        assert_eq!(scaled.dimensions(), (4096, 2048));
        let scaled_six = apply_scale(sample_image(8192, 8192), "6k");
        assert_eq!(scaled_six.dimensions(), (6144, 6144));
        let untouched = apply_scale(sample_image(2048, 1024), "4k");
        assert_eq!(untouched.dimensions(), (2048, 1024));
    }

    fn anime_base_dir_for_tests() -> Option<PathBuf> {
        dirs::data_dir().map(|dir| dir.join("studio.avroracl.aias"))
    }

    fn write_mean_alpha(png: &Path) -> f64 {
        let image = image::open(png).expect("output should be readable");
        let rgba = image.to_rgba8();
        let (width, height) = rgba.dimensions();
        let sum: u64 = rgba.pixels().map(|pixel| pixel[3] as u64).sum();
        sum as f64 / (width as u64 * height as u64) as f64
    }

    #[test]
    fn anime_cutout_runs_locally_without_comfyui() {
        let _gpu_guard = GPU_TEST_LOCK.lock().unwrap_or_else(|error| error.into_inner());
        let input = Path::new("F:\\WebUI\\ComfyUI\\input\\anime_test.png");
        let Some(base) = anime_base_dir_for_tests() else {
            eprintln!("skip: app data dir unavailable");
            return;
        };
        if !input.exists() || !anime::is_model_ready(&base, "simple") {
            eprintln!("skip: test image or simple model not found");
            return;
        }
        let output = std::env::temp_dir().join("aias_anime_cutout_test");
        let _ = std::fs::remove_dir_all(&output);
        let result = anime_cutout_inner(
            None,
            AnimeCutoutOptions {
                files: vec![path_to_string(input)],
                output_path: path_to_string(&output),
                model: Some("simple".into()),
                refine_hair: false,
                recover_details: false,
            },
        )
        .expect("anime cutout should succeed");
        assert_eq!(result.completed, 1);
        let saved = output.join("anime_test_simple.png");
        let image = image::open(&saved).expect("output should be readable");
        assert_eq!(image.width(), 1600);
        assert_eq!(image.height(), 1133);
        let mean_alpha = write_mean_alpha(&saved);
        assert!(
            mean_alpha < 200.0,
            "mean alpha {mean_alpha} should indicate a cut background"
        );
        assert!(
            image
                .as_rgba8()
                .map(|rgba| rgba.pixels().any(|pixel| pixel[3] < 10))
                .unwrap_or(false),
            "output should contain transparent pixels"
        );
    }

    #[test]
    fn superres_models_status_reports_catalog() {
        let dir = std::env::temp_dir().join("aias_superres_status_test");
        let _ = fs::remove_dir_all(&dir);
        let status = superres::models_status(&dir);
        assert_eq!(status.len(), 2);
        assert_eq!(status[0].id, "anime");
        assert_eq!(status[1].id, "general");
        assert!(!status[0].installed);
        assert!(status[0].total_size > 0);
        assert!(!superres::is_model_ready(&dir, "anime"));
        assert!(superres::superres_spec("anime").is_ok());
        assert!(superres::superres_spec("nope").is_err());
    }

    /// 重型 GPU 测试共享一把锁：并发加载多套 ONNX 会话会在显存紧张的机器上
    /// 触发 cuDNN/驱动级失败，串行执行才反映真实使用方式。
    static GPU_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    #[ignore]
    fn superres_full_image_bench() {
        // 手动基准：AIAS_AB_SR_INPUT=图片 AIAS_AB_SR_MODELS=anime,general
        let input = std::env::var("AIAS_AB_SR_INPUT")
            .unwrap_or_else(|_| "F:/AIAS/ab/inref-small/原图.png".into());
        let models = std::env::var("AIAS_AB_SR_MODELS").unwrap_or_else(|_| "anime,general".into());
        let Some(base) = anime_base_dir_for_tests() else {
            eprintln!("skip: app data dir unavailable");
            return;
        };
        let output = std::env::temp_dir().join("aias_superres_bench");
        let _ = fs::remove_dir_all(&output);
        for model in models.split(',') {
            if !superres::is_model_ready(&base, model) {
                eprintln!("skip: model {model} not installed");
                continue;
            }
            let started = std::time::Instant::now();
            let result = superres_run_inner(
                None,
                SuperResRunOptions {
                    files: vec![input.clone()],
                    output_path: path_to_string(&output),
                    model: model.to_string(),
                    scale: None,
                },
            )
            .unwrap_or_else(|error| panic!("{model}: {error}"));
            assert_eq!(result.completed, 1);
            println!("[sr-bench] model={model} elapsed={:?}", started.elapsed());
        }
    }

    #[test]
    fn superres_anime_upscale_runs_locally() {
        let _gpu_guard = GPU_TEST_LOCK.lock().unwrap_or_else(|error| error.into_inner());
        let Some(base) = anime_base_dir_for_tests() else {
            eprintln!("skip: app data dir unavailable");
            return;
        };
        if !superres::is_model_ready(&base, "anime") {
            eprintln!("skip: superres anime model not found");
            return;
        }
        // 300x96 红蓝棋盘测试图：横向跨 2 个推理块（验证拼接与颜色通道），
        // 右半带半透明 Alpha（验证 Alpha 通道超分）。
        let mut input_image = image::RgbaImage::new(300, 96);
        for (x, y, pixel) in input_image.enumerate_pixels_mut() {
            let check = (x / 8 + y / 8) % 2 == 0;
            *pixel = image::Rgba([if check { 220 } else { 40 }, 90, if check { 30 } else { 200 }, if x < 150 { 255 } else { 120 }]);
        }
        let input = std::env::temp_dir().join("aias_superres_input.png");
        input_image.save(&input).expect("save test input");
        let output_dir = std::env::temp_dir().join("aias_superres_run_test");
        let _ = fs::remove_dir_all(&output_dir);
        let result = superres_run_inner(
            None,
            SuperResRunOptions {
                files: vec![path_to_string(&input)],
                output_path: path_to_string(&output_dir),
                model: "anime".into(),
                scale: None,
            },
        )
        .expect("superres run should succeed");
        assert_eq!(result.completed, 1);
        let saved = output_dir.join("aias_superres_input_4x_anime.png");
        let image = image::open(&saved).expect("output should be readable");
        assert_eq!((image.width(), image.height()), (1200, 384), "4x output size");
        let rgba = image.to_rgba8();
        // Alpha 通道也被超分：右半 (x>=600) 的不透明度应明显低于左半。
        let left: u64 = rgba.pixels().filter(|p| p[3] >= 200).count() as u64;
        let right_soft: u64 = rgba
            .enumerate_pixels()
            .filter(|(x, _, p)| *x >= 600 && p[3] > 100 && p[3] < 220)
            .count() as u64;
        assert!(left > 1200 * 384 / 4, "left half should stay mostly opaque");
        assert!(right_soft > 10_000, "right half should be partially transparent after alpha superres");
        // 颜色通道必须保持（回归：曾把 NCHW 输出按 HWC 读取，输出变灰度乱块）。
        let reddish = rgba.pixels().filter(|p| p[0] as i32 > p[2] as i32 + 40).count();
        let bluish = rgba.pixels().filter(|p| p[2] as i32 > p[0] as i32 + 40).count();
        assert!(reddish > 50_000, "checkerboard red blocks should survive superres");
        assert!(bluish > 50_000, "checkerboard blue blocks should survive superres");

        // 无 Alpha 的图输出必须是全不透明（回归：曾输出 alpha=0 的全透明 PNG）。
        let opaque_input = std::env::temp_dir().join("aias_superres_input_opaque.png");
        image::DynamicImage::ImageRgba8(input_image).to_rgb8().save(&opaque_input).expect("save opaque input");
        let result = superres_run_inner(
            None,
            SuperResRunOptions {
                files: vec![path_to_string(&opaque_input)],
                output_path: path_to_string(&output_dir),
                model: "anime".into(),
                scale: None,
            },
        )
        .expect("opaque superres run should succeed");
        assert_eq!(result.completed, 1);
        let opaque = image::open(output_dir.join("aias_superres_input_opaque_4x_anime.png"))
            .expect("opaque output should be readable")
            .to_rgba8();
        assert_eq!(opaque.pixels().filter(|p| p[3] == 255).count(), (1200 * 384) as usize, "opaque input must stay fully opaque");

        // 非原生倍率：2x 在 4x 结果上缩小，8x 插值放大；输出名与尺寸都带倍率。
        for scale in [2_u32, 6, 8] {
            let result = superres_run_inner(
                None,
                SuperResRunOptions {
                    files: vec![path_to_string(&input)],
                    output_path: path_to_string(&output_dir),
                    model: "anime".into(),
                    scale: Some(scale),
                },
            )
            .unwrap_or_else(|error| panic!("scale {scale}: {error}"));
            assert_eq!(result.completed, 1);
            let saved = output_dir.join(superres_output_name("aias_superres_input", "anime", scale));
            let image = image::open(&saved).unwrap_or_else(|error| panic!("scale {scale}: {error}"));
            assert_eq!(
                (image.width(), image.height()),
                (300 * scale, 96 * scale),
                "{scale}x output size"
            );
        }
    }

    #[test]
    fn superres_output_name_uses_scale() {
        assert_eq!(superres_output_name("hero", "anime", 4), "hero_4x_anime.png");
        assert_eq!(superres_output_name("hero", "general", 6), "hero_6x_general.png");
    }

    #[test]
    fn anime_advanced_cutout_runs_locally() {
        let _gpu_guard = GPU_TEST_LOCK.lock().unwrap_or_else(|error| error.into_inner());
        let input = Path::new("F:\\WebUI\\ComfyUI\\input\\anime_test.png");
        let Some(base) = anime_base_dir_for_tests() else {
            eprintln!("skip: app data dir unavailable");
            return;
        };
        if !input.exists() || !anime::is_model_ready(&base, "advanced") {
            eprintln!("skip: test image or advanced models not found");
            return;
        }
        let output = std::env::temp_dir().join("aias_anime_cutout_advanced_test");
        let _ = std::fs::remove_dir_all(&output);
        let result = anime_cutout_inner(
            None,
            AnimeCutoutOptions {
                files: vec![path_to_string(input)],
                output_path: path_to_string(&output),
                model: Some("advanced".into()),
                refine_hair: false,
                recover_details: false,
            },
        )
        .expect("advanced cutout should succeed");
        assert_eq!(result.completed, 1);
        let saved = output.join("anime_test_advanced.png");
        let mean_alpha = write_mean_alpha(&saved);
        assert!(
            mean_alpha < 200.0,
            "mean alpha {mean_alpha} should indicate a cut background"
        );
    }

    #[test]
    #[ignore = "uses the real 1024 model and the maintained anime A/B fixture"]
    fn toonout_fallback_reports_and_saves_the_actual_result() {
        let input = Path::new(r"F:\战争雷霆涂装\贴图素材\F15E 塞雷娅\测试\原图.png");
        let output = PathBuf::from(
            r"F:\战争雷霆涂装\贴图素材\F15E 塞雷娅\AB测试结果\P53_toonout_ui_contract_specialist",
        );
        let Some(base) = anime_base_dir_for_tests() else {
            eprintln!("skip: app data dir unavailable");
            return;
        };
        assert!(input.is_file(), "A/B 原图必须存在");
        assert!(
            anime::is_model_ready(&base, "toonout"),
            "测试需要 ToonOut 模型"
        );
        assert!(
            anime::is_model_ready(&base, "anime-specialist")
                || anime::is_model_ready(&base, "birefnet-general"),
            "测试至少需要 AnimeSeg 或 General 1024 作为 ToonOut 回退模型"
        );
        std::fs::create_dir_all(&output).expect("创建 UI 合约测试输出目录");

        let result = anime_cutout_inner(
            None,
            AnimeCutoutOptions {
                files: vec![path_to_string(input)],
                output_path: path_to_string(&output),
                model: Some("toonout".into()),
                refine_hair: false,
                recover_details: false,
            },
        )
        .expect("ToonOut 回退流程必须成功");
        // 专精模型已安装时应优先接管；尚未安装的旧用户仍可由 General 保持可用。
        let expected_model = if anime::is_model_ready(&base, "anime-specialist") {
            "anime-specialist"
        } else {
            "birefnet-general"
        };
        let expected_name = format!("原图_{expected_model}.png");
        let expected = output.join(&expected_name);
        assert_eq!(result.completed, 1);
        assert!(expected.is_file(), "回退结果必须用实际模型名落盘");
        assert!(
            result
                .outputs
                .iter()
                .any(|path| path.ends_with(&expected_name)),
            "返回结果必须指向实际模型文件：{:?}",
            result.outputs
        );
        assert!(
            result
                .logs
                .iter()
                .any(|line| line.contains(&anime::model_label(expected_model))),
            "日志必须说明实际接管模型：{:?}",
            result.logs
        );
    }

    #[test]
    fn anime_models_status_reports_catalog() {
        let Some(base) = anime_base_dir_for_tests() else {
            eprintln!("skip: app data dir unavailable");
            return;
        };
        let status = anime::models_status(&base);
        assert_eq!(status.len(), 6);
        assert_eq!(status[0].id, "anime-specialist");
        assert_eq!(status[1].id, "toonout");
        assert_eq!(status[2].id, "birefnet-general");
        assert_eq!(status[3].id, "birefnet-lite");
        assert_eq!(status[4].id, "simple");
        assert_eq!(status[5].id, "advanced");
        for model in &status {
            assert!(!model.label.is_empty());
            assert!(model.total_size > 0);
        }
    }

    #[test]
    #[ignore = "touches the real appdata model files and the network; run with --ignored"]
    fn anime_uninstall_then_download_round_trip_for_simple() {
        let Some(base) = anime_base_dir_for_tests() else {
            eprintln!("skip: app data dir unavailable");
            return;
        };
        if !anime::is_model_ready(&base, "simple") {
            eprintln!("skip: simple model not seeded; download would take minutes");
            return;
        }
        anime::uninstall_model(&base, "simple").expect("uninstall should succeed");
        assert!(
            !anime::is_model_ready(&base, "simple"),
            "model should be missing after uninstall"
        );
        anime::download_model(None, &base, "simple").expect("download should succeed");
        assert!(
            anime::is_model_ready(&base, "simple"),
            "model should be ready after download"
        );
    }
}
