//! Anime matting models: catalog, download/uninstall into the app data folder,
//! and built-in ONNX inference (no ComfyUI required at runtime).

use image::{ImageBuffer, Luma};
use serde::Serialize;
use std::fs;
use std::path::{Path, PathBuf};
mod infer;
mod postprocess;
#[cfg(test)]
mod recovery_tests;
mod runtime;
#[cfg(test)]
mod tests;

pub(crate) use infer::*;
pub(crate) use postprocess::*;
pub(crate) use runtime::*;

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

pub(crate) const ISNETIS_SIZE: u64 = 176_069_933;

pub(crate) const RTMDET_SIZE: u64 = 238_686_077;

pub(crate) const REFINER_SIZE: u64 = 176_197_192;

pub(crate) const TOONOUT_SIZE: u64 = 492_381_880;

pub(crate) const BIREFNET_LITE_SIZE: u64 = 114_538_787;

pub(crate) const BIREFNET_GENERAL_1024_FP16_SIZE: u64 = 489_666_272;

pub(crate) const ANIME_SPECIALIST_SIZE: u64 = 117_239_813;

/// ViTMatte small 的 ONNX 导出，仅作为 AnimeSeg 成图的可选 8px 边界 alpha
/// 精修器；它不参与主模型下拉选择，也不会在未启用时被加载或下载。
pub(crate) const HAIR_REFINER_ID: &str = "vitmatte-hair-refiner";
pub(crate) const HAIR_REFINER_LABEL: &str = "精细发丝边缘（ViTMatte）";
pub(crate) const HAIR_REFINER_FILE: &str = "vitmatte-small-distinctions-646.onnx";
pub(crate) const HAIR_REFINER_SIZE: u64 = 103_885_865;
pub(crate) const HAIR_REFINER_FILES: &[ModelFileSpec] = &[ModelFileSpec {
    name: HAIR_REFINER_FILE,
    size: HAIR_REFINER_SIZE,
    mirror_url: "https://hf-mirror.com/Xenova/vitmatte-small-distinctions-646/resolve/da379332422700028fcade44e2cb915b6eed3548/onnx/model.onnx",
    origin_url: "https://huggingface.co/Xenova/vitmatte-small-distinctions-646/resolve/da379332422700028fcade44e2cb915b6eed3548/onnx/model.onnx",
}];

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

pub(crate) fn model_spec(id: &str) -> Result<&'static ModelSpec, String> {
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

// Status
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

/// 发丝精修器是附属能力而非一个可单独抠图的模型，因此单独暴露其状态，避免
/// 在主模型选择器中出现一个无法独立工作的选项。
pub fn hair_refiner_status(base: &Path) -> ModelStatus {
    let dir = models_dir(base);
    let files: Vec<ModelFileStatus> = HAIR_REFINER_FILES
        .iter()
        .map(|file| {
            let path = dir.join(file.name);
            let size = fs::metadata(&path).map(|meta| meta.len()).unwrap_or(0);
            ModelFileStatus {
                name: file.name.to_string(),
                present: path.exists(),
                size,
                expected_size: file.size,
            }
        })
        .collect();
    let installed = files
        .iter()
        .all(|file| file.present && file.size == file.expected_size);
    ModelStatus {
        id: HAIR_REFINER_ID.to_string(),
        label: HAIR_REFINER_LABEL.to_string(),
        installed,
        total_size: HAIR_REFINER_FILES.iter().map(|file| file.size).sum(),
        files,
    }
}

pub fn is_hair_refiner_ready(base: &Path) -> bool {
    hair_refiner_status(base).installed
}

pub fn is_model_ready(base: &Path, id: &str) -> bool {
    models_status(base)
        .into_iter()
        .find(|model| model.id == id)
        .map(|model| model.installed)
        .unwrap_or(false)
}

pub(crate) fn stable_sigmoid(value: f32) -> f32 {
    if value >= 0.0 {
        1.0 / (1.0 + (-value).exp())
    } else {
        let exp = value.exp();
        exp / (1.0 + exp)
    }
}

/// 一次抠图的结果：实际生效的模型、以及是否发生了兜底回退。
pub struct CutoutOutcome {
    pub model_used: String,
    pub fallback: bool, // 是否从 toonout 回退到其他模型
}

// Public entry: cut a single image
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
    cutout_with_options(base, model_id, input, output, false, false)
}

/// 与默认抠图相同，但可显式启用实验性细节恢复与 ViTMatte 的窄边界发丝精修。
/// 两个开关仅对用户直接选择的 AnimeSeg 生效，避免改变 ToonOut 回退链及其它
/// 已验证模型的行为。
pub fn cutout_with_options(
    base: &Path,
    model_id: &str,
    input: &Path,
    output: &Path,
    refine_hair_edges: bool,
    recover_details: bool,
) -> Result<CutoutOutcome, String> {
    if refine_hair_edges && model_id != "anime-specialist" {
        return Err("精细发丝边缘目前仅支持动漫专精（AnimeSeg）。".into());
    }
    if refine_hair_edges && !is_hair_refiner_ready(base) {
        return Err("精细发丝边缘模型未安装，请先在右侧栏下载。".into());
    }
    if recover_details && model_id != "anime-specialist" {
        return Err("高分辨率细节补全目前仅支持动漫专精（AnimeSeg）。".into());
    }
    let mut image = image::open(input).map_err(to_string_error)?;
    if let Some(orientation) = exif_orientation(input)? {
        image.apply_orientation(orientation);
    }
    let rgb = image.to_rgb8();
    let (w, h) = rgb.dimensions();

    let (mask, fallback_model) = match model_spec(model_id)?.kind {
        ModelKind::Simple => (run_simple(base, &rgb)?, model_id),
        ModelKind::Advanced => (run_advanced(base, &rgb)?, model_id),
        ModelKind::BiRefNet { matting } => (run_birefnet(base, model_id, matting, &rgb)?, model_id),
        ModelKind::Toonout => {
            let mask = run_birefnet(base, "toonout", false, &rgb)?;
            if toonout_likely_failed(&mask, w, h) {
                // 先释放 ToonOut 会话，避免两套大模型在显存中重叠。AnimeSeg 专精
                // 模型优先处理「主体与背景同为动漫线稿」的误保留，通用 BiRefNet
                // 仍保留为专精模型未安装或未通过客观清理门槛时的兜底。
                release_birefnet_session("toonout");
                let specialist_candidate = if is_model_ready(base, "anime-specialist") {
                    match run_birefnet(base, "anime-specialist", false, &rgb) {
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

    // AnimeSeg 固定以 1024 输入换取稳定的动漫主体语义。对于长边达到 1600px 的
    // 原图，再只在其自动 4px 边界带上使用原始 RGB 求解 alpha，恢复高分辨率
    // 轮廓；其它模型和较小图片保留既有输出，避免改变已验证的行为。
    let mask = if fallback_model == "anime-specialist" {
        refine_closed_form_boundary_alpha(&rgb, mask)
    } else {
        mask
    };
    let result = finalize_cutout_image(&rgb, mask, model_uses_native_edge_alpha(fallback_model));
    let result = if recover_details
        && model_id == "anime-specialist"
        && fallback_model == "anime-specialist"
    {
        recover_anime_specialist_details_rgba(base, &rgb, result)?
    } else {
        result
    };
    let result = if refine_hair_edges
        && model_id == "anime-specialist"
        && fallback_model == "anime-specialist"
    {
        refine_vitmatte_boundary_rgba(base, &rgb, result)?
    } else {
        result
    };
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

/// EXIF orientation via the format decoder; only JPEG actually carries it here.
pub(crate) fn exif_orientation(
    input: &Path,
) -> Result<Option<image::metadata::Orientation>, String> {
    let file = fs::File::open(input).map_err(to_string_error)?;
    let mut decoder = match image::codecs::jpeg::JpegDecoder::new(std::io::BufReader::new(file)) {
        Ok(decoder) => decoder,
        Err(_) => return Ok(None),
    };
    use image::ImageDecoder as _;
    Ok(decoder.orientation().ok())
}

pub(crate) fn to_string_error(error: impl std::fmt::Display) -> String {
    error.to_string()
}
