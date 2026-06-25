use image::{DynamicImage, ImageBuffer, Luma, Rgba, RgbaImage};
use serde::{Deserialize, Serialize};
use std::{
  collections::BTreeMap,
  fs,
  path::{Path, PathBuf},
  process::{Command, Stdio},
};
use tauri::{Manager, State};
use tempfile::TempDir;

const DDS_EXT: &str = ".dds";

#[derive(Clone)]
struct AppState {
  settings_path: PathBuf,
  texconv_path: PathBuf,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
struct Settings {
  auto_update: bool,
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
  image_to_dds_output_path: String,
  image_to_dds_alpha: String,
  image_to_dds_format: String,
  skin_manager_path: String,
}

impl Default for Settings {
  fn default() -> Self {
    Self {
      auto_update: false,
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
      image_to_dds_output_path: String::new(),
      image_to_dds_alpha: "keep".into(),
      image_to_dds_format: "DXT5".into(),
      skin_manager_path: String::new(),
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
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MergePbrOptions {
  input_path: String,
  output_path: String,
  alpha: Option<String>,
  format: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SplitPbrOptions {
  files: Vec<String>,
  output_path: String,
  export_format: Option<String>,
  export_alpha: Option<bool>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MipmapOptions {
  input_path: String,
  output_path: String,
  alpha: Option<String>,
  format: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ConvertImagesOptions {
  files: Vec<String>,
  output_path: String,
  alpha: Option<String>,
  format: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SkinFile {
  name: String,
  path: String,
  disabled: bool,
  size: u64,
  modified_at: u128,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ImportSkinOptions {
  files: Vec<String>,
  target_directory: String,
}

#[derive(Debug, Serialize)]
struct ImportSkinResult {
  imported: usize,
}

#[derive(Debug, Serialize)]
struct PathResult {
  path: String,
}

#[derive(Debug, Serialize)]
struct DeleteResult {
  deleted: bool,
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
    .setup(|app| {
      let app_data = app
        .path()
        .app_data_dir()
        .map_err(|error| format!("Cannot resolve app data directory: {error}"))?;
      let settings_path = app_data.join("settings.json");
      let texconv_path = resolve_texconv_path(app)?;
      app.manage(AppState {
        settings_path,
        texconv_path,
      });
      Ok(())
    })
    .invoke_handler(tauri::generate_handler![
      settings_get,
      settings_set,
      texture_find_groups,
      texture_merge_pbr,
      texture_split_pbr,
      texture_create_mipmap,
      texture_convert_images_to_dds,
      skin_auto_detect,
      skin_list,
      skin_import,
      skin_toggle,
      skin_delete
    ])
    .run(tauri::generate_context!())
    .expect("error while running AIAS");
}

fn resolve_texconv_path(app: &tauri::App) -> Result<PathBuf, String> {
  let resource_path = app.path().resource_dir().ok();
  let current_dir = std::env::current_dir().ok();
  let candidates = [
    resource_path.map(|path| path.join("tools").join("texconv.exe")),
    current_dir.as_ref().map(|path| path.join("tools").join("texconv.exe")),
    current_dir.as_ref().map(|path| path.join("texconv.exe")),
  ];
  candidates
    .into_iter()
    .flatten()
    .find(|path| path.exists())
    .ok_or_else(|| "未找到 texconv.exe。请确认 tools/texconv.exe 存在。".to_string())
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
fn texture_merge_pbr(state: State<AppState>, options: MergePbrOptions) -> Result<TaskResult, String> {
  require_directory(&options.input_path, "输入目录")?;
  fs::create_dir_all(&options.output_path).map_err(to_string_error)?;
  let groups = find_texture_groups(Path::new(&options.input_path))?;
  let mut logs = vec![format!("找到 {} 组完整 PBR 贴图。", groups.len())];
  let format = options.format.as_deref().unwrap_or("DXT5");
  let alpha = options.alpha.as_deref().unwrap_or("black");
  let mut completed = 0;

  for group in &groups {
    let c_path = Path::new(&options.output_path).join(format!("{}_c.dds", group.prefix));
    let n_path = Path::new(&options.output_path).join(format!("{}_n.dds", group.prefix));
    process_base_color(&state.texconv_path, Path::new(&group.files.basecolor), &c_path, alpha, format)?;
    process_roughness_metallic_normal(
      &state.texconv_path,
      Path::new(&group.files.roughness),
      Path::new(&group.files.metallic),
      Path::new(&group.files.normal),
      &n_path,
      format,
    )?;
    completed += 1;
    logs.push(format!("完成 {}", group.prefix));
  }

  Ok(TaskResult {
    completed,
    total: groups.len(),
    logs,
  })
}

#[tauri::command]
fn texture_split_pbr(state: State<AppState>, options: SplitPbrOptions) -> Result<TaskResult, String> {
  fs::create_dir_all(&options.output_path).map_err(to_string_error)?;
  let export_format = options.export_format.as_deref().unwrap_or("png");
  let export_alpha = options.export_alpha.unwrap_or(true);
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
    let image = dds_to_image(&state.texconv_path, file_path)?;
    let rgba = image.to_rgba8();
    let (width, height) = rgba.dimensions();
    let lower = stem.to_lowercase();

    if lower.ends_with("_c") {
      let mut rgb = Vec::with_capacity((width * height * 3) as usize);
      let mut alpha = Vec::with_capacity((width * height) as usize);
      for pixel in rgba.pixels() {
        rgb.extend_from_slice(&[pixel[0], pixel[1], pixel[2]]);
        alpha.push(pixel[3]);
      }
      save_rgb_image(&rgb, width, height, output_dir.join(format!("{prefix}_BaseColor.{export_format}")), export_format)?;
      if export_alpha {
        save_luma_image(&alpha, width, height, output_dir.join(format!("{prefix}_Alpha.{export_format}")), export_format)?;
      }
      logs.push(format!("拆分 {stem}: BaseColor{}", if export_alpha { " / Alpha" } else { "" }));
    } else if lower.ends_with("_n") {
      let mut roughness = Vec::with_capacity((width * height) as usize);
      let mut metallic = Vec::with_capacity((width * height) as usize);
      let mut normal = RgbaImage::new(width, height);
      for (x, y, pixel) in rgba.enumerate_pixels() {
        roughness.push(255 - pixel[0]);
        metallic.push(pixel[2]);
        normal.put_pixel(x, y, Rgba([pixel[3], pixel[1], 255, 255]));
      }
      save_luma_image(&roughness, width, height, output_dir.join(format!("{prefix}_Roughness.{export_format}")), export_format)?;
      save_luma_image(&metallic, width, height, output_dir.join(format!("{prefix}_Metallic.{export_format}")), export_format)?;
      save_dynamic_image(&DynamicImage::ImageRgba8(normal), output_dir.join(format!("{prefix}_Normal.{export_format}")), export_format)?;
      logs.push(format!("拆分 {stem}: Roughness / Metallic / Normal"));
    }
    completed += 1;
  }

  Ok(TaskResult {
    completed,
    total: options.files.len(),
    logs,
  })
}

#[tauri::command]
fn texture_create_mipmap(state: State<AppState>, options: MipmapOptions) -> Result<TaskResult, String> {
  require_directory(&options.input_path, "输入目录")?;
  fs::create_dir_all(&options.output_path).map_err(to_string_error)?;
  let alpha = options.alpha.as_deref().unwrap_or("keep");
  let format = options.format.as_deref().unwrap_or("DXT5");
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

  let temp_dir = TempDir::new().map_err(to_string_error)?;
  let mut levels = Vec::new();
  for (index, file) in files.iter().enumerate() {
    let temp_png = temp_dir.path().join(format!("p{index}.png"));
    prepare_image(file, &temp_png, alpha)?;
    let dds = png_to_dds_buffer(&state.texconv_path, &temp_png, format)?;
    let image = image::open(&temp_png).map_err(to_string_error)?;
    levels.push(MipmapLevel {
      width: image.width(),
      height: image.height(),
      payload: extract_dds_payload(&dds)?,
    });
  }

  let output_file = Path::new(&options.output_path).join("Mipmap.dds");
  fs::write(&output_file, build_dds_with_mipmaps(&levels, format)?).map_err(to_string_error)?;
  Ok(TaskResult {
    completed: files.len(),
    total: files.len(),
    logs: vec![format!("生成 {}", output_file.display())],
  })
}

#[tauri::command]
fn texture_convert_images_to_dds(state: State<AppState>, options: ConvertImagesOptions) -> Result<TaskResult, String> {
  fs::create_dir_all(&options.output_path).map_err(to_string_error)?;
  let alpha = options.alpha.as_deref().unwrap_or("keep");
  let format = options.format.as_deref().unwrap_or("DXT5");
  let mut logs = Vec::new();

  for file in &options.files {
    let input = Path::new(file);
    let output_file = Path::new(&options.output_path)
      .join(input.file_stem().and_then(|value| value.to_str()).unwrap_or("output"))
      .with_extension("dds");
    image_to_dds(&state.texconv_path, input, &output_file, alpha, format)?;
    logs.push(format!(
      "转换 {} -> {}",
      input.file_name().and_then(|value| value.to_str()).unwrap_or(file),
      output_file.file_name().and_then(|value| value.to_str()).unwrap_or("output.dds")
    ));
  }

  Ok(TaskResult {
    completed: options.files.len(),
    total: options.files.len(),
    logs,
  })
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

#[tauri::command]
fn skin_list(directory: String) -> Result<Vec<SkinFile>, String> {
  require_directory(&directory, "涂装目录")?;
  let mut items = Vec::new();
  for entry in fs::read_dir(directory).map_err(to_string_error)? {
    let entry = entry.map_err(to_string_error)?;
    let metadata = entry.metadata().map_err(to_string_error)?;
    if !metadata.is_file() {
      continue;
    }
    let name = entry.file_name().to_string_lossy().to_string();
    let modified_at = metadata
      .modified()
      .ok()
      .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
      .map(|duration| duration.as_millis())
      .unwrap_or_default();
    items.push(SkinFile {
      disabled: name.ends_with(".disabled"),
      name,
      path: path_to_string(entry.path()),
      size: metadata.len(),
      modified_at,
    });
  }
  items.sort_by(|a, b| a.name.cmp(&b.name));
  Ok(items)
}

#[tauri::command]
fn skin_import(options: ImportSkinOptions) -> Result<ImportSkinResult, String> {
  require_directory(&options.target_directory, "涂装目录")?;
  let mut imported = 0;
  for file in options.files {
    let source = Path::new(&file);
    if !source.is_file() {
      continue;
    }
    let Some(name) = source.file_name() else {
      continue;
    };
    fs::copy(source, Path::new(&options.target_directory).join(name)).map_err(to_string_error)?;
    imported += 1;
  }
  Ok(ImportSkinResult { imported })
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
  if !source.exists() {
    return Ok(DeleteResult { deleted: false });
  }
  fs::remove_file(source).map_err(to_string_error)?;
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
    let Some(ext) = path.extension().and_then(|value| value.to_str()).map(|value| format!(".{}", value.to_lowercase())) else {
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
        groups.entry(prefix).or_default().insert(kind.into(), path_to_string(&path));
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

fn process_base_color(texconv: &Path, base_color: &Path, output: &Path, alpha: &str, format: &str) -> Result<(), String> {
  let mut image = image::open(base_color).map_err(to_string_error)?.to_rgba8();
  let alpha_value = if alpha == "white" { 255 } else { 0 };
  for pixel in image.pixels_mut() {
    pixel[3] = alpha_value;
  }
  raw_to_dds(texconv, &image, output, format)
}

fn process_roughness_metallic_normal(
  texconv: &Path,
  roughness_path: &Path,
  metallic_path: &Path,
  normal_path: &Path,
  output: &Path,
  format: &str,
) -> Result<(), String> {
  let normal = image::open(normal_path).map_err(to_string_error)?.to_rgba8();
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
  raw_to_dds(texconv, &combined, output, format)
}

fn raw_to_dds(texconv: &Path, image: &RgbaImage, output: &Path, format: &str) -> Result<(), String> {
  let temp_dir = TempDir::new().map_err(to_string_error)?;
  let temp_png = temp_dir.path().join("source.png");
  image.save(&temp_png).map_err(to_string_error)?;
  convert_png_to_dds(texconv, &temp_png, output, format)
}

fn image_to_dds(texconv: &Path, input: &Path, output: &Path, alpha: &str, format: &str) -> Result<(), String> {
  let temp_dir = TempDir::new().map_err(to_string_error)?;
  let temp_png = temp_dir.path().join("source.png");
  prepare_image(input, &temp_png, alpha)?;
  convert_png_to_dds(texconv, &temp_png, output, format)
}

fn prepare_image(input: &Path, output: &Path, alpha: &str) -> Result<(), String> {
  let mut image = image::open(input).map_err(to_string_error)?.to_rgba8();
  if alpha == "black" || alpha == "white" {
    let alpha_value = if alpha == "white" { 255 } else { 0 };
    for pixel in image.pixels_mut() {
      pixel[3] = alpha_value;
    }
  }
  image.save(output).map_err(to_string_error)
}

fn convert_png_to_dds(texconv: &Path, input_png: &Path, output: &Path, format: &str) -> Result<(), String> {
  let buffer = png_to_dds_buffer(texconv, input_png, format)?;
  fs::write(output, buffer).map_err(to_string_error)
}

fn png_to_dds_buffer(texconv: &Path, input_png: &Path, format: &str) -> Result<Vec<u8>, String> {
  let temp_dir = TempDir::new().map_err(to_string_error)?;
  run_command(
    texconv,
    &[
      "-y",
      "-f",
      to_texconv_format(format),
      "-m",
      "1",
      "-o",
      temp_dir.path().to_str().ok_or_else(|| "临时目录路径无效。".to_string())?,
      input_png.to_str().ok_or_else(|| "输入文件路径无效。".to_string())?,
    ],
  )?;
  let generated = temp_dir.path().join(input_png.file_stem().unwrap()).with_extension("dds");
  fs::read(generated).map_err(to_string_error)
}

fn dds_to_image(texconv: &Path, dds_path: &Path) -> Result<DynamicImage, String> {
  let temp_dir = TempDir::new().map_err(to_string_error)?;
  run_command(
    texconv,
    &[
      "-y",
      "-ft",
      "png",
      "-o",
      temp_dir.path().to_str().ok_or_else(|| "临时目录路径无效。".to_string())?,
      dds_path.to_str().ok_or_else(|| "DDS 文件路径无效。".to_string())?,
    ],
  )?;
  let png_path = temp_dir.path().join(dds_path.file_stem().unwrap()).with_extension("png");
  image::open(png_path).map_err(to_string_error)
}

fn save_rgb_image(bytes: &[u8], width: u32, height: u32, output: PathBuf, format: &str) -> Result<(), String> {
  let image = image::RgbImage::from_raw(width, height, bytes.to_vec()).ok_or_else(|| "RGB 数据无效。".to_string())?;
  save_dynamic_image(&DynamicImage::ImageRgb8(image), output, format)
}

fn save_luma_image(bytes: &[u8], width: u32, height: u32, output: PathBuf, format: &str) -> Result<(), String> {
  let image: ImageBuffer<Luma<u8>, Vec<u8>> =
    ImageBuffer::from_raw(width, height, bytes.to_vec()).ok_or_else(|| "灰度数据无效。".to_string())?;
  save_dynamic_image(&DynamicImage::ImageLuma8(image), output, format)
}

fn save_dynamic_image(image: &DynamicImage, output: PathBuf, format: &str) -> Result<(), String> {
  if format.eq_ignore_ascii_case("tga") {
    image.save_with_format(output, image::ImageFormat::Tga).map_err(to_string_error)
  } else {
    image.save_with_format(output, image::ImageFormat::Png).map_err(to_string_error)
  }
}

fn extract_dds_payload(dds: &[u8]) -> Result<Vec<u8>, String> {
  if dds.len() < 128 || &dds[0..4] != b"DDS " {
    return Err("DDS 数据无效。".into());
  }
  let header_size = u32::from_le_bytes(dds[4..8].try_into().unwrap()) as usize;
  let pixel_flags = u32::from_le_bytes(dds[80..84].try_into().unwrap());
  let four_cc = &dds[84..88];
  let mut header_end = 4 + header_size;
  if pixel_flags & 0x4 != 0 && four_cc == b"DX10" {
    header_end += 20;
  }
  Ok(dds[header_end..].to_vec())
}

fn build_dds_with_mipmaps(levels: &[MipmapLevel], format: &str) -> Result<Vec<u8>, String> {
  let first = levels.first().ok_or_else(|| "没有可写入的 mipmap 层级。".to_string())?;
  let compressed = !matches!(format, "8.8.8.8" | "R8G8B8A8_UNORM");
  let mut header = vec![0_u8; 128];
  header[0..4].copy_from_slice(b"DDS ");
  write_u32(&mut header, 4, 124);
  write_u32(&mut header, 8, 0x1 | 0x2 | 0x4 | 0x1000 | 0x20000 | if compressed { 0x80000 } else { 0x8 });
  write_u32(&mut header, 12, first.height);
  write_u32(&mut header, 16, first.width);
  write_u32(&mut header, 20, if compressed { calculate_bc3_size(first.width, first.height) } else { first.width * 4 });
  write_u32(&mut header, 28, levels.len() as u32);
  write_u32(&mut header, 76, 32);
  if compressed {
    write_u32(&mut header, 80, 0x4);
    header[84..88].copy_from_slice(b"DXT5");
  } else {
    write_u32(&mut header, 80, 0x40 | 0x1);
    write_u32(&mut header, 88, 32);
    write_u32(&mut header, 92, 0x00ff0000);
    write_u32(&mut header, 96, 0x0000ff00);
    write_u32(&mut header, 100, 0x000000ff);
    write_u32(&mut header, 104, 0xff000000);
  }
  write_u32(&mut header, 108, 0x1000 | 0x400000 | 0x8);
  let mut output = header;
  for level in levels {
    output.extend_from_slice(&level.payload);
  }
  Ok(output)
}

fn run_command(command: &Path, args: &[&str]) -> Result<(), String> {
  let output = Command::new(command)
    .args(args)
    .stdin(Stdio::null())
    .output()
    .map_err(to_string_error)?;
  if output.status.success() {
    Ok(())
  } else {
    Err(String::from_utf8_lossy(if output.stderr.is_empty() { &output.stdout } else { &output.stderr }).to_string())
  }
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

fn to_texconv_format(format: &str) -> &'static str {
  if format == "8.8.8.8" || format == "R8G8B8A8_UNORM" {
    "R8G8B8A8_UNORM"
  } else {
    "BC3_UNORM"
  }
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
