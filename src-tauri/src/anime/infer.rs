//! `anime::infer` — 拆分自 anime.rs，职责见模块内条目注释。

use super::*;

use image::imageops::FilterType;
use image::{ImageBuffer, Luma, RgbImage};
use ort::session::Session;
use ort::value::{Tensor, ValueType};
use std::path::Path;

pub(crate) fn input_size(session: &Session) -> Result<(usize, usize), String> {
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

pub(crate) fn outlet_tensor_shape(outlet: &ort::value::Outlet) -> Result<Vec<i64>, String> {
    match outlet.dtype() {
        ValueType::Tensor { shape, .. } => Ok(shape.to_vec()),
        other => Err(format!("模型输入类型不支持：{other:?}")),
    }
}

/// PIL-style thumbnail: aspect-preserving downscale, never enlarges.
// Shared image helpers
pub(crate) fn thumbnail_fit(w: u32, h: u32, max_w: u32, max_h: u32) -> (u32, u32) {
    let ratio = (max_w as f64 / w as f64)
        .min(max_h as f64 / h as f64)
        .min(1.0);
    let new_w = (w as f64 * ratio).round() as u32;
    let new_h = (h as f64 * ratio).round() as u32;
    (new_w.max(1), new_h.max(1))
}

pub(crate) fn bilinear_resize_luma(data: &[u8], width: u32, height: u32, new_w: u32, new_h: u32) -> Vec<u8> {
    let source = ImageBuffer::<Luma<u8>, Vec<u8>>::from_raw(width, height, data.to_vec())
        .expect("buffer size mismatch");
    image::imageops::resize(&source, new_w, new_h, FilterType::Triangle).into_raw()
}

pub(crate) fn to_f32(data: Vec<u8>) -> Vec<f32> {
    data.into_iter().map(|value| value as f32 / 255.0).collect()
}

pub(crate) fn probability_luma(probabilities: &[f32], threshold: f32) -> Vec<u8> {
    probabilities
        .iter()
        .map(|probability| if *probability > threshold { 255 } else { 0 })
        .collect()
}

pub(crate) fn refine_threshold() -> f32 {
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

pub(crate) fn advanced_min_component_area(width: u32, height: u32) -> usize {
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

// Simple model (ISNet / isnetis.onnx) — port of simple_anime_seg.py
pub(crate) fn run_simple(base: &Path, rgb: &RgbImage) -> Result<Vec<f32>, String> {
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

pub(crate) const STRIDES: [u32; 3] = [8, 16, 32];

pub(crate) const DETECTION_THRESHOLD: f32 = 0.3;

pub(crate) const REFINE_THRESHOLD: f32 = 0.3;

pub(crate) const MEAN: [f32; 3] = [123.675, 116.28, 103.53];

pub(crate) const STD: [f32; 3] = [58.395, 57.12, 57.375];

/// RTMDet 的一个角色实例候选。默认仍选最高检测置信度；开发期可用面积与
/// 中心位置做重排序，验证「最大且最居中主体」能否减少背景角色误选。
#[derive(Debug, Clone, Copy)]
pub(crate) struct CharacterCandidate {
    pub(crate) confidence: f32,
    pub(crate) stride: u32,
    pub(crate) row: usize,
    pub(crate) col: usize,
    pub(crate) x1: f32,
    pub(crate) y1: f32,
    pub(crate) x2: f32,
    pub(crate) y2: f32,
}

pub(crate) fn main_subject_score(candidate: CharacterCandidate, width: u32, height: u32) -> f32 {
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
pub(crate) fn ab_main_subject_selector_enabled() -> bool {
    std::env::var("AIAS_AB_INSTANCE_SELECTOR")
        .ok()
        .is_some_and(|value| value.eq_ignore_ascii_case("main-subject"))
}

#[cfg(not(test))]
pub(crate) fn ab_main_subject_selector_enabled() -> bool {
    false
}

pub(crate) fn choose_character_candidate(
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

pub(crate) fn resize_pad_rgb(img: &RgbImage, size: u32) -> (RgbImage, (u32, u32, u32, u32)) {
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

// Advanced model (RTMDet + ISNetDis refiner) — port of advanced_anime_seg.py
pub(crate) fn run_advanced(base: &Path, rgb: &RgbImage) -> Result<Vec<f32>, String> {
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

// BiRefNet family (ToonOut + official general/portrait/HR/lite)
pub(crate) const BIREFNET_MEAN: [f32; 3] = [0.485, 0.456, 0.406];

pub(crate) const BIREFNET_STD: [f32; 3] = [0.229, 0.224, 0.225];

/// BiRefNet 发布推理的图像预处理：整图直接缩放到模型固定输入。
/// 不进行等比留边，否则竖图会浪费掉大部分有效分割面积。
pub(crate) fn resize_birefnet_input(rgb: &RgbImage, target_w: u32, target_h: u32) -> RgbImage {
    if rgb.dimensions() == (target_w, target_h) {
        rgb.clone()
    } else {
        image::imageops::resize(rgb, target_w, target_h, birefnet_resize_filter())
    }
}

/// 上游 `preprocessor_config.json` 的 `resample: 2` 是双线性插值；A/B 仍可
/// 显式切回其它插值验证，以避免后续调整悄悄偏离该工作流。
pub(crate) fn birefnet_resize_filter() -> FilterType {
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
pub(crate) fn birefnet_range_normalization_enabled() -> bool {
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
pub(crate) fn run_birefnet(base: &Path, id: &str, matting: bool, rgb: &RgbImage) -> Result<Vec<f32>, String> {
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
pub(crate) fn run_birefnet_on_provider(
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

pub(crate) fn mean_with_horizontal_flip(
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
pub(crate) fn run_birefnet_single_for_test(
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

pub(crate) fn try_run_birefnet(
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
pub(crate) fn try_run_birefnet_path(
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
pub(crate) fn run_birefnet_local_file(
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

pub(crate) fn matte_is_substantially_cleaner(candidate: &[f32], baseline: &[f32], w: u32, h: u32) -> bool {
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
pub(crate) fn border_band_mean(matte: &[f32], w: u32, h: u32, band_px: usize) -> f32 {
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
pub(crate) fn foreground_ratio(matte: &[f32], w: u32, h: u32) -> f32 {
    if w == 0 || h == 0 {
        return 0.0;
    }
    let count = matte.iter().filter(|value| **value > 0.5).count();
    count as f32 / (w * h) as f32
}
