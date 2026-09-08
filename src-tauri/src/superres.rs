//! 立绘 / 素材 AI 超分：RealESRGAN x4（通用 + 动漫 6B 特化），分块 ONNX 推理。
//! 与抠图模型相互独立：模型目录、会话槽与命令都以 `superres` 命名，互不干扰。

use crate::anime::{
    build_session, curl_download, emit_progress, is_gpu_oom_error, lock_error, ModelFileSpec,
    ModelProgress,
};
use image::RgbaImage;
use ort::session::Session;
use ort::value::Tensor;
use serde::Serialize;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use tauri::AppHandle;

pub struct SuperResSpec {
    pub id: &'static str,
    pub label: &'static str,
    pub file: ModelFileSpec,
}

const ANIME_6B_SIZE: u64 = 18_352_469;

const GENERAL_X4_SIZE: u64 = 67_051_616;

pub const SUPERRES_MODELS: &[SuperResSpec] = &[
    SuperResSpec {
        id: "anime",
        label: "动漫超分（RealESRGAN 动漫 4x）",
        file: ModelFileSpec {
            name: "RealESRGAN_x4plus_anime_6B.onnx",
            size: ANIME_6B_SIZE,
            mirror_url: "https://hf-mirror.com/RekluzLabs/realesrgan_anime6b.onnx/resolve/main/realesrgan_anime6b.onnx",
            origin_url: "https://huggingface.co/RekluzLabs/realesrgan_anime6b.onnx/resolve/main/realesrgan_anime6b.onnx",
        },
    },
    SuperResSpec {
        id: "general",
        label: "通用超分（RealESRGAN 通用 4x）",
        file: ModelFileSpec {
            name: "RealESRGAN_x4plus.onnx",
            size: GENERAL_X4_SIZE,
            mirror_url: "https://hf-mirror.com/SceneWorks/real-esrgan-onnx/resolve/main/real_esrgan_x4.onnx",
            origin_url: "https://huggingface.co/SceneWorks/real-esrgan-onnx/resolve/main/real_esrgan_x4.onnx",
        },
    },
];

pub fn superres_spec(id: &str) -> Result<&'static SuperResSpec, String> {
    SUPERRES_MODELS
        .iter()
        .find(|model| model.id == id)
        .ok_or_else(|| format!("未知超分模型：{id}"))
}

pub fn superres_dir(base: &Path) -> PathBuf {
    base.join("models").join("superres")
}

fn model_path(base: &Path, id: &str) -> Result<PathBuf, String> {
    Ok(superres_dir(base).join(superres_spec(id)?.file.name))
}

pub fn is_model_ready(base: &Path, id: &str) -> bool {
    superres_spec(id)
        .map(|spec| {
            let path = superres_dir(base).join(spec.file.name);
            path.exists()
                && fs::metadata(&path).map(|meta| meta.len()).unwrap_or(0) == spec.file.size
        })
        .unwrap_or(false)
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SuperResModelStatus {
    pub id: String,
    pub label: String,
    pub installed: bool,
    pub total_size: u64,
}

pub fn models_status(base: &Path) -> Vec<SuperResModelStatus> {
    SUPERRES_MODELS
        .iter()
        .map(|spec| SuperResModelStatus {
            id: spec.id.to_string(),
            label: spec.label.to_string(),
            installed: is_model_ready(base, spec.id),
            total_size: spec.file.size,
        })
        .collect()
}

pub fn download_model(app: Option<&AppHandle>, base: &Path, id: &str) -> Result<(), String> {
    let spec = superres_spec(id)?;
    fs::create_dir_all(superres_dir(base)).map_err(crate::anime::to_string_error)?;
    let file = &spec.file;
    let dest = superres_dir(base).join(file.name);
    let already_ok =
        dest.exists() && fs::metadata(&dest).map(|meta| meta.len()).unwrap_or(0) == file.size;
    if already_ok {
        return Ok(());
    }
    let progress = |completed: u64, total: u64| {
        if let Some(handle) = app {
            emit_progress(
                Some(handle),
                ModelProgress {
                    model_id: format!("superres-{id}"),
                    file: file.name.to_string(),
                    completed,
                    total,
                },
            );
        }
    };
    let mut last_error = String::from("无可用下载源");
    for url in [file.mirror_url, file.origin_url] {
        match curl_download(url, &dest, Some(file.size), &progress) {
            Ok(()) => return Ok(()),
            Err(error) => {
                last_error = error;
                if dest.exists() {
                    let _ = fs::remove_file(&dest);
                }
            }
        }
    }
    Err(format!("下载 {} 失败：{last_error}", file.name))
}

pub fn uninstall_model(base: &Path, id: &str) -> Result<(), String> {
    superres_spec(id)?;
    release_session(id);
    let path = model_path(base, id)?;
    if path.exists() {
        fs::remove_file(&path).map_err(crate::anime::to_string_error)?;
    }
    Ok(())
}

fn sessions() -> &'static Mutex<Vec<(String, Session)>> {
    static SLOT: OnceLock<Mutex<Vec<(String, Session)>>> = OnceLock::new();
    SLOT.get_or_init(|| Mutex::new(Vec::new()))
}

fn release_session(id: &str) {
    // 缓存键为 "{id}:{use_gpu}"，按前缀同时释放 GPU 与 CPU 两个槽位。
    let prefix = format!("{id}:");
    if let Ok(mut sessions) = sessions().lock() {
        sessions.retain(|(model_id, _)| !model_id.starts_with(&prefix));
    }
}

/// 立绘/素材超分：模型固定 4x 推理（RGB 走分块推理；带 Alpha 的图把 Alpha
/// 当灰度图再过一遍同一网络，保持硬边缘不糊），再按目标倍率 2–8 做 Lanczos
/// 重采样——低于 4x 是高质量缩小，高于 4x 是插值放大（不新增细节）。
/// 输出固定为 `{原图名}_{倍率}x_{模型id}.png`。
/// `on_progress(done_units, total_units)`：图块级进度回调（带 Alpha 的图
/// RGB 与 Alpha 两遍推理，总量翻倍）。
pub fn upscale_with_progress(
    base: &Path,
    id: &str,
    input: &Path,
    output: &Path,
    scale: u32,
    on_progress: &dyn Fn(usize, usize, &str),
) -> Result<(), String> {
    let scale = scale.clamp(2, 8);
    on_progress(0, 1, "正在准备推理运行库");
    crate::anime::ensure_ort_runtime(base)?;
    superres_spec(id)?;
    on_progress(0, 1, "正在读取图片");
    let image = image::open(input)
        .map_err(crate::anime::to_string_error)?
        .to_rgba8();
    let (w, h) = image.dimensions();
    // 中间产物是 4x，最终尺寸由 scale 决定，两者都不得超出安全上限。
    let peak = w.max(h) as u64 * u64::from(scale.max(4));
    if peak > 16384 {
        return Err(format!(
            "图片过大（{scale}x 后约 {}x{}），请先缩小图片或降低倍率。",
            w as u64 * u64::from(scale),
            h as u64 * u64::from(scale)
        ));
    }

    retry_tiles(|use_gpu, tile_size| {
        let phase = if !use_gpu { "显存不足，切换 CPU 重试（64px 分块）".to_string() }
            else if tile_size < 256 { format!("显存不足，缩小为 {tile_size}px 分块重试") }
            else { "正在加载超分模型".to_string() };
        on_progress(0, 1, &phase);
        let result = try_upscale_with(base, id, &image, output, use_gpu, scale, tile_size, on_progress);
        if result.as_ref().err().is_some_and(|error| is_gpu_oom_error(error)) {
            release_session(id);
        }
        result
    })
}

fn retry_tiles(mut attempt: impl FnMut(bool, u32) -> Result<(), String>) -> Result<(), String> {
    for (use_gpu, tile_size) in [(true, 256), (true, 128), (true, 64), (false, 64)] {
        match attempt(use_gpu, tile_size) {
            Ok(()) => return Ok(()),
            Err(error) if is_gpu_oom_error(&error) => continue,
            Err(error) => return Err(error),
        }
    }
    Err("内存不足，无法完成超分：请关闭部分程序释放内存后重试，或改用更小的图片。".into())
}

fn try_upscale_with(
    base: &Path,
    id: &str,
    image: &RgbaImage,
    output: &Path,
    use_gpu: bool,
    scale: u32,
    tile_size: u32,
    on_progress: &dyn Fn(usize, usize, &str),
) -> Result<(), String> {
    let path = model_path(base, id)?;
    if !path.exists() {
        return Err("超分模型未安装，请先在右侧栏下载。".into());
    }
    let cache_key = format!("{id}:{use_gpu}");
    {
        let mut sessions = sessions().lock().map_err(lock_error)?;
        if !sessions.iter().any(|(model_id, _)| *model_id == cache_key) {
            sessions.push((cache_key.clone(), build_session(&path, use_gpu)?));
        }
    }
    let (w, h) = image.dimensions();
    let out_w = w as usize * 4;
    let out_h = h as usize * 4;
    let mut result = vec![0_u8; out_w * out_h * 4];
    // 不透明的图直接按不透明输出；带透明的图由后面的 Alpha 推理填充。
    let has_alpha = image.pixels().any(|pixel| pixel[3] != 255);
    if !has_alpha {
        for slot in result.chunks_exact_mut(4) {
            slot[3] = 255;
        }
    }

    // 图块总数与 run_pass 的分块规则一致（256px + 16px 重叠）；带 Alpha 的图
    // 两遍推理，进度单位翻倍。
    let tile_count = w.div_ceil(tile_size) as usize * h.div_ceil(tile_size) as usize;
    // Reserve one unit each for final resize and PNG save.
    let total_units = tile_count * (if has_alpha { 2 } else { 1 }) + 2;
    let report = |done_units: usize| {
        let phase = if done_units == total_units { "已保存".to_string() }
            else if done_units + 1 == total_units { "正在保存 PNG".to_string() }
            else if done_units + 2 == total_units { "正在调整输出尺寸".to_string() }
            else if done_units >= tile_count && has_alpha { format!("处理透明通道 · 分块 {}/{}", done_units - tile_count, tile_count) }
            else { format!("处理颜色细节 · 分块 {done_units}/{tile_count}") };
        on_progress(done_units.min(total_units), total_units, &phase);
    };

    report(0);
    // RGB 通道
    run_pass(&cache_key, image, tile_size, |pixel| {
        [pixel[0] as f32 / 255.0, pixel[1] as f32 / 255.0, pixel[2] as f32 / 255.0]
    }, |tile_out, tile_w, x0, y0| {
        for (row, pixels) in tile_out.chunks_exact(tile_w * 3).enumerate() {
            let dst_y = y0 + row;
            if dst_y >= out_h {
                break;
            }
            for (col, pixel) in pixels.chunks_exact(3).enumerate() {
                let dst_x = x0 + col;
                if dst_x >= out_w {
                    break;
                }
                let slot = (dst_y * out_w + dst_x) * 4;
                result[slot] = (pixel[0] * 255.0).round().clamp(0.0, 255.0) as u8;
                result[slot + 1] = (pixel[1] * 255.0).round().clamp(0.0, 255.0) as u8;
                result[slot + 2] = (pixel[2] * 255.0).round().clamp(0.0, 255.0) as u8;
            }
        }
    }, &report)?;

    // Alpha 通道（仅当存在透明像素）
    if has_alpha {
        run_pass(&cache_key, image, tile_size, |pixel| {
            let alpha = pixel[3] as f32 / 255.0;
            [alpha, alpha, alpha]
        }, |tile_out, tile_w, x0, y0| {
            for (row, pixels) in tile_out.chunks_exact(tile_w * 3).enumerate() {
                let dst_y = y0 + row;
                if dst_y >= out_h {
                    break;
                }
                for (col, pixel) in pixels.chunks_exact(3).enumerate() {
                    let dst_x = x0 + col;
                    if dst_x >= out_w {
                        break;
                    }
                    result[(dst_y * out_w + dst_x) * 4 + 3] =
                        (pixel[0] * 255.0).round().clamp(0.0, 255.0) as u8;
                }
            }
        }, &|done| report(tile_count + done))?;
    }

    let framed = image::RgbaImage::from_raw(w * 4, h * 4, result)
        .ok_or("超分输出缓冲尺寸无效")?;
    // 模型只会输出 4x；其余倍率在 4x 结果上做一次 Lanczos 重采样。
    let final_image = if scale == 4 {
        framed
    } else {
        image::imageops::resize(&framed, w * scale, h * scale, image::imageops::FilterType::Lanczos3)
    };
    report(total_units - 1);
    final_image
        .save_with_format(output, image::ImageFormat::Png)
        .map_err(crate::anime::to_string_error)?;
    report(total_units);
    Ok(())
}

/// 分块推理：每个 256px 输入块带 16px 上下文（避免接缝），输出裁掉边缘后
/// 按 4x 写回调用方给定的通道写回器。`sink(tile_out_flat_hwc_rgb, tile_out_w, x0, y0)`
/// 收到的是该块输出左上角对应的全图输出坐标。
fn run_pass(
    cache_key: &str,
    image: &RgbaImage,
    tile_size: u32,
    sample: impl Fn(&image::Rgba<u8>) -> [f32; 3],
    mut sink: impl FnMut(&[f32], usize, usize, usize),
    on_tile: &dyn Fn(usize),
) -> Result<(), String> {
    const OVERLAP: u32 = 16;
    let (w, h) = image.dimensions();
    let mut sessions = sessions().lock().map_err(lock_error)?;
    let session = sessions
        .iter_mut()
        .find(|(model_id, _)| *model_id == cache_key)
        .map(|(_, session)| session)
        .ok_or("超分模型会话未初始化")?;
    let input_name = session.inputs()[0].name().to_string();
    let output_name = session.outputs()[0].name().to_string();
    let mut tiles_done = 0_usize;

    let mut y = 0;
    while y < h {
        let y0 = y;
        let y1 = (y0 + tile_size).min(h);
        let mut x = 0;
        while x < w {
            let x0 = x;
            let x1 = (x0 + tile_size).min(w);
            let pad_x0 = x0.saturating_sub(OVERLAP);
            let pad_y0 = y0.saturating_sub(OVERLAP);
            let pad_x1 = (x1 + OVERLAP).min(w);
            let pad_y1 = (y1 + OVERLAP).min(h);
            let tw = (pad_x1 - pad_x0) as usize;
            let th = (pad_y1 - pad_y0) as usize;

            let tile = image::imageops::crop_imm(image, pad_x0, pad_y0, pad_x1 - pad_x0, pad_y1 - pad_y0)
                .to_image();
            let mut input = vec![0_f32; 3 * tw * th];
            for (index, pixel) in tile.pixels().enumerate() {
                let channels = sample(pixel);
                input[index] = channels[0];
                input[tw * th + index] = channels[1];
                input[2 * tw * th + index] = channels[2];
            }
            let tensor =
                Tensor::from_array((vec![1_usize, 3, th, tw], input)).map_err(crate::anime::to_string_error)?;
            let outputs = session
                .run(ort::inputs![input_name.as_str() => tensor])
                .map_err(crate::anime::to_string_error)?;
            let (shape, data) = outputs[output_name.as_str()]
                .try_extract_tensor::<f32>()
                .map_err(crate::anime::to_string_error)?;
            let out_w_tile = (*shape.get(3).ok_or("超分模型输出 shape 无效")?) as usize;
            let out_h_tile = (*shape.get(2).ok_or("超分模型输出 shape 无效")?) as usize;
            if out_w_tile != tw * 4 {
                return Err(format!("超分模型输出宽度异常：{out_w_tile} != {}", tw * 4));
            }

            // 裁掉上下文对应的输出边缘（4x = 64px）。图片边界处 padding 被
            // clamp，各侧实际可裁量按 pad 宽度计算，写回核心区。
            let crop_l = (x0 - pad_x0) as usize * 4;
            let crop_t = (y0 - pad_y0) as usize * 4;
            let core_w = (x1 - x0) as usize * 4;
            let core_h = (y1 - y0) as usize * 4;
            let mut core = vec![0_f32; core_w * core_h * 3];
            // 模型输出是 NCHW 平面布局，按通道平面取值后转成 HWC 交错。
            let plane = out_w_tile * out_h_tile;
            for row in 0..core_h {
                let src_row = (row + crop_t) * out_w_tile + crop_l;
                let dst_row = row * core_w * 3;
                for col in 0..core_w {
                    let src = src_row + col;
                    core[dst_row + col * 3] = data[src];
                    core[dst_row + col * 3 + 1] = data[plane + src];
                    core[dst_row + col * 3 + 2] = data[2 * plane + src];
                }
            }
            sink(&core, core_w, x0 as usize * 4, y0 as usize * 4);
            tiles_done += 1;
            on_tile(tiles_done);
            x = x1;
        }
        y = y1;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn superres_retries_smaller_gpu_tiles_before_cpu() {
        let mut seen = Vec::new();
        retry_tiles(|gpu, tile| {
            seen.push((gpu, tile));
            if gpu { Err("out of memory".into()) } else { Ok(()) }
        }).unwrap();
        assert_eq!(seen, [(true, 256), (true, 128), (true, 64), (false, 64)]);
    }

    #[test]
    fn superres_stops_after_success_or_unrelated_error() {
        let mut seen = Vec::new();
        retry_tiles(|gpu, tile| {
            seen.push((gpu, tile));
            if tile == 256 { Err("out of memory".into()) } else { Ok(()) }
        }).unwrap();
        assert_eq!(seen, [(true, 256), (true, 128)]);
        let mut calls = 0;
        let error = retry_tiles(|_, _| { calls += 1; Err("invalid tensor".into()) }).unwrap_err();
        assert_eq!(calls, 1);
        assert_eq!(error, "invalid tensor");
    }

    #[test]
    fn superres_reports_exhausted_memory_retries() {
        let mut calls = 0;
        let error = retry_tiles(|_, _| { calls += 1; Err("out of memory".into()) }).unwrap_err();
        assert_eq!(calls, 4);
        assert!(error.contains("内存不足"));
    }
}
