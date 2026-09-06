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

/// 立绘/素材 4x 超分：RGB 走模型分块推理；带 Alpha 的图（如抠图结果、
/// 带透明的立绘）把 Alpha 当灰度图再过一遍同一网络，保持硬边缘不糊。
/// 输出固定为 `{原图名}_4x_{模型id}.png`。
pub fn upscale(base: &Path, id: &str, input: &Path, output: &Path) -> Result<(), String> {
    crate::anime::ensure_ort_runtime(base)?;
    superres_spec(id)?;
    let image = image::open(input)
        .map_err(crate::anime::to_string_error)?
        .to_rgba8();
    let (w, h) = image.dimensions();
    let out_w = w as u64 * 4;
    let out_h = h as u64 * 4;
    if out_w.max(out_h) > 16384 {
        return Err(format!(
            "图片过大（4x 后约 {out_w}x{out_h}），请先缩小后再超分。"
        ));
    }

    match try_upscale_with(base, id, &image, output, true) {
        Ok(()) => Ok(()),
        Err(error) if is_gpu_oom_error(&error) => {
            release_session(id);
            try_upscale_with(base, id, &image, output, false).map_err(|cpu_error| {
                if is_gpu_oom_error(&cpu_error) {
                    // CPU 回退仍分配失败：机器可提交内存耗尽（GPU 会话与图块缓冲叠加）。
                    "内存不足，无法完成超分：请关闭部分程序释放内存后重试，或改用更小的图片。".to_string()
                } else {
                    cpu_error
                }
            })
        }
        Err(error) => Err(error),
    }
}

fn try_upscale_with(
    base: &Path,
    id: &str,
    image: &RgbaImage,
    output: &Path,
    use_gpu: bool,
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

    // RGB 通道
    run_pass(&cache_key, image, |pixel| {
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
    })?;

    // Alpha 通道（仅当存在透明像素）
    if has_alpha {
        run_pass(&cache_key, image, |pixel| {
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
        })?;
    }

    let framed = image::RgbaImage::from_raw(w * 4, h * 4, result)
        .ok_or("超分输出缓冲尺寸无效")?;
    framed
        .save_with_format(output, image::ImageFormat::Png)
        .map_err(crate::anime::to_string_error)?;
    Ok(())
}

/// 分块推理：每个 256px 输入块带 16px 上下文（避免接缝），输出裁掉边缘后
/// 按 4x 写回调用方给定的通道写回器。`sink(tile_out_flat_hwc_rgb, tile_out_w, x0, y0)`
/// 收到的是该块输出左上角对应的全图输出坐标。
fn run_pass(
    cache_key: &str,
    image: &RgbaImage,
    sample: impl Fn(&image::Rgba<u8>) -> [f32; 3],
    mut sink: impl FnMut(&[f32], usize, usize, usize),
) -> Result<(), String> {
    const TILE: u32 = 256;
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

    let mut y = 0;
    while y < h {
        let y0 = y;
        let y1 = (y0 + TILE).min(h);
        let mut x = 0;
        while x < w {
            let x0 = x;
            let x1 = (x0 + TILE).min(w);
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
            x = x1;
        }
        y = y1;
    }
    Ok(())
}
