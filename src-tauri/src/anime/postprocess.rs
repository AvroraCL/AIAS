//! `anime::postprocess` — 拆分自 anime.rs，职责见模块内条目注释。

use super::*;
use image::{RgbImage, RgbaImage};
use rayon::prelude::*;
use std::collections::HashMap;

// P5 GT 诊断开关：只在测试构建中允许逐项绕过后处理，用于确定性 A/B；
// 正式应用始终保持完整管线，避免环境变量改变用户产物。
#[cfg(test)]
pub(crate) fn ab_postprocess_stage_enabled(stage: &str) -> bool {
    !std::env::var("AIAS_AB_DISABLE_STAGES")
        .unwrap_or_default()
        .split(',')
        .map(str::trim)
        .any(|disabled| disabled.eq_ignore_ascii_case(stage))
}

#[cfg(not(test))]
pub(crate) fn ab_postprocess_stage_enabled(_: &str) -> bool {
    true
}

pub(crate) fn remove_small_foreground_components(
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
pub(crate) fn remove_background_islands(matte: &mut [f32], width: u32, height: u32) {
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

pub(crate) fn fill_small_background_holes(
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

/// 半透明「幽灵残留」抑制：分割模型对复杂背景的低置信度响应会在发丝
/// 间隙、角色两侧留下大片半透明背景碎屑，浅色预览看不出来，换底后是
/// 一片幽灵色块，是抠图观感差的主因。策略：以实心主体（alpha ≥ SOLID）
/// 为源做城市块距离变换，非实心像素随距离渐进衰减——紧贴实心边缘的
/// 发丝/水花细节几乎不受影响，远离主体的背景碎屑平滑归零。孤立但实心
/// 的前景（如脱手的饰品）不受影响。
// matte 上采样后，半透明过渡带会出现锯齿和孤立噪点；只对过渡带（含 1px
// 膨胀）做 3x3 高斯平滑，实心和透明区域保持原样，避免啃掉细发丝。
pub(crate) fn suppress_background_ghosts(mask: &mut [f32], w: u32, h: u32) {
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

pub(crate) fn smooth_matte_edges(mask: &[f32], w: u32, h: u32) -> Vec<f32> {
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

/// 边缘去污染（defringe）：过渡带的颜色被旧背景混入，换背景后边缘发灰发粉。
/// 先用大半径加权估计局部背景色 B = Σ(C·(1-a)) / Σ(1-a)，
/// 再对 a < DECONTAM_CEIL 的像素解混 F = (C - (1-a)·B) / max(a, ε)。
// Matte 后处理：背景残留抑制、引导滤波、边缘去污染
pub(crate) fn decontaminate_colors(rgb: &RgbImage, matte: &[f32]) -> Vec<[u8; 3]> {
    const RADIUS: usize = 16;
    // CEIL 拉到 0.95：细发丝的「实心」像素只有 2-6px 宽，颜色同样被旧背景
    // 污染（换底后边缘发粉）。a=0.9 时解混修正量只有 ~10%，把近实心像素
    // 也纳入解混收益明显、风险很小；合法粉色主体（发饰）周围背景占比低，
    // 由 BG_PRESENCE_MIN 守卫。
    const CEIL: f32 = 242.0 / 255.0;
    const FLOOR_A: f32 = 0.15;
    const BG_PRESENCE_MIN: f64 = 0.05;

    let (w, h) = rgb.dimensions();
    let (w, h) = (w as usize, h as usize);
    let total = w * h;
    let mut out: Vec<[u8; 3]> = Vec::new();
    if total == 0 || matte.len() != total {
        for pixel in rgb.pixels() {
            out.push([pixel[0], pixel[1], pixel[2]]);
        }
        return out;
    }
    // Only confident background can estimate the old background color. Soft
    // foreground pixels otherwise contaminate their own estimate and lose color.
    let weight: Vec<f64> = matte.iter().map(|a| if *a <= 0.05 { 1.0 } else { 0.0 }).collect();
    let rgb_raw = rgb.as_raw();
    let build_weighted = |ch: usize| -> Vec<f64> {
        weight
            .par_iter()
            .enumerate()
            .map(|(index, wgt)| rgb_raw[index * 3 + ch] as f64 * wgt)
            .collect()
    };
    let weighted = [build_weighted(0), build_weighted(1), build_weighted(2)];
    // 4 路均值共用同一积分图缓冲，省去重复的数百 MB 分配与清零。
    let mut sat = Vec::new();
    let mut buf: Vec<f64> = Vec::new();
    box_mean_f64_into(&weight, w, h, RADIUS, &mut sat, &mut buf);
    let mean_w = std::mem::take(&mut buf);
    let mut mean_c: [Vec<f64>; 3] = Default::default();
    for ch in 0..3 {
        box_mean_f64_into(&weighted[ch], w, h, RADIUS, &mut sat, &mut buf);
        mean_c[ch] = std::mem::take(&mut buf);
    }
    out.resize(total, [0u8; 3]);
    out.par_iter_mut().enumerate().for_each(|(index, slot)| {
        let pixel = rgb.get_pixel((index % w) as u32, (index / w) as u32);
        let a = matte[index];
        let wsum = mean_w[index];
        if a >= CEIL || a <= 0.0 || wsum < BG_PRESENCE_MIN {
            *slot = [pixel[0], pixel[1], pixel[2]];
            return;
        }
        let aa = (a as f64).max(FLOOR_A as f64);
        let mut color = [0u8; 3];
        for ch in 0..3 {
            let bg = mean_c[ch][index] / wsum;
            // Use the same regularized alpha on both sides of the equation;
            // mixing a with max(a, floor) darkens very fine, low-alpha edges.
            let foreground = (pixel[ch] as f64 - (1.0 - aa) * bg) / aa;
            // Uncertain alpha must not amplify a small background-estimation
            // error into a black/white fringe. Keep correction local in color.
            let original = pixel[ch] as f64;
            color[ch] = foreground.clamp(original - 24.0, original + 24.0)
                .round().clamp(0.0, 255.0) as u8;
        }
        *slot = color;
    });
    out
}

/// O(n) 积分图盒均值。
/// O(n) 积分图盒均值。`sat`/`out` 由调用方提供以便同一调用点连续求多路均值
/// （去污染为 4 路）时复用缓冲，省去每次数百 MB 的临时分配与清零；
/// 逐元素结果与独立分配调用完全一致。
pub(crate) fn box_mean_f64_into(
    values: &[f64],
    w: usize,
    h: usize,
    radius: usize,
    sat: &mut Vec<f64>,
    out: &mut Vec<f64>,
) {
    let stride = w + 1;
    sat.clear();
    sat.resize(stride * (h + 1), 0.0);
    for y in 0..h {
        let mut row_sum = 0f64;
        for x in 0..w {
            row_sum += values[y * w + x];
            sat[(y + 1) * stride + (x + 1)] = sat[y * stride + (x + 1)] + row_sum;
        }
    }
    out.clear();
    out.resize(w * h, 0.0);
    out.par_chunks_mut(w).enumerate().for_each(|(y, row)| {
        let y0 = y.saturating_sub(radius);
        let y1 = (y + radius + 1).min(h);
        for x in 0..w {
            let x0 = x.saturating_sub(radius);
            let x1 = (x + radius + 1).min(w);
            let area = ((y1 - y0) * (x1 - x0)) as f64;
            let sum = sat[y1 * stride + x1] - sat[y0 * stride + x1] - sat[y1 * stride + x0]
                + sat[y0 * stride + x0];
            row[x] = sum / area;
        }
    });
}

/// O(n) 积分图盒均值（f32 版）：引导滤波要用约 17 路均值，f64 版会带来
/// 数百 MB 瞬时内存；累加仍走 f64 积分图保证精度，输入输出用 f32。
pub(crate) fn box_mean_f32(values: &[f32], w: usize, h: usize, radius: usize) -> Vec<f32> {
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
    out.par_chunks_mut(w).enumerate().for_each(|(y, row)| {
        let y0 = y.saturating_sub(radius);
        let y1 = (y + radius + 1).min(h);
        for x in 0..w {
            let x0 = x.saturating_sub(radius);
            let x1 = (x + radius + 1).min(w);
            let area = ((y1 - y0) * (x1 - x0)) as f64;
            let sum = sat[y1 * stride + x1] - sat[y0 * stride + x1] - sat[y1 * stride + x0]
                + sat[y0 * stride + x0];
            row[x] = (sum / area) as f32;
        }
    });
    out
}

/// RGB 引导的快速引导滤波（He et al.）：低分辨率推理的掩码上采样后边缘
/// 软糊（512² 模型放大 4 倍时过渡带约 8px、发丝尖端糊成圆头），用原图
/// 做引导把 alpha 贴回真实结构——原图里发丝轮廓是清晰的，滤波后过渡带
/// 收窄到 1-2px，糊住的尖端重新分开。eps 越小越贴合强边缘；平坦区域
/// a→0 退化为均值，掩码不会被过度改动。
pub(crate) fn guided_filter_matte(rgb: &RgbImage, p: &[f32], radius: usize, eps: f64) -> Vec<f32> {
    let (w, h) = rgb.dimensions();
    let (w, h) = (w as usize, h as usize);
    let n = w * h;
    if n == 0 || p.len() != n || radius == 0 {
        return p.to_vec();
    }
    let radius = radius
        .min(w.saturating_sub(1))
        .min(h.saturating_sub(1))
        .max(1);

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
    let mut pair_mean = |a: &[f32], bch: &[f32]| -> Vec<f32> {
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
pub(crate) fn solidify_subject(rgb: &RgbImage, mask: &mut [f32], w: u32, h: u32) {
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

    // 判定逐像素独立，可并行；写回按索引顺序串行执行，保证与旧串行实现
    // 逐字节一致。
    let verdicts: Vec<u8> = (0..n)
        .into_par_iter()
        .map(|i| {
            let a = mask[i];
            if a <= MIN_KEEP || a >= MAX_KEEP {
                return 0u8;
            }
            let (sf, sb) = (wsum_fg[i], wsum_bg[i]);
            if sf < 1e-4 || sb < 1e-4 {
                return 0u8;
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
                1
            } else if a < 0.85 && db * 0.8 < df {
                // 颜色站在背景一边：中间置信度的背景残迹归零。
                2
            } else {
                0
            }
        })
        .collect();
    for (i, verdict) in verdicts.iter().enumerate() {
        match verdict {
            1 => mask[i] = 1.0,
            2 => mask[i] = 0.0,
            _ => {}
        }
    }
}

/// 1024 General 已在原始图尺寸保留足够的边界细节；再做引导滤波会沿画面中的
/// 线稿偏移 alpha，实测增加边界误差。其它低分辨率模型仍依赖引导滤波贴回边缘。
pub(crate) fn model_uses_native_edge_alpha(model_id: &str) -> bool {
    matches!(model_id, "birefnet-general" | "anime-specialist")
}

/// 开发期可通过 `AIAS_AB_ALPHA_FLOOR` 量化不同的极淡 alpha 清理阈值；
/// 正式流程固定使用经回归验证的默认值，避免用户环境变量意外改变产品输出。
pub(crate) fn low_alpha_floor() -> f32 {
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

/// 以 AnimeSeg 的全局语义遮罩生成自动 trimap，并在原图分辨率的窄轮廓带内
/// 解闭式 alpha。它只负责恢复亚像素轮廓，不重新判断主体，因此不会像局部分块
/// 模型那样把复杂动漫背景重新纳入前景。
///
/// 此函数刻意不依赖 Python/OpenCV：每个 512px 核心块只求解环绕主实例的未知带，
/// 使 4K 图保持可控的内存占用。调用方应先在 A/B 中验证后再接入正式模型路径。
pub(crate) fn refine_closed_form_boundary_alpha(rgb: &RgbImage, mask: Vec<f32>) -> Vec<f32> {
    refine_closed_form_boundary_alpha_with_diagnostics(rgb, mask).0
}

/// 仅供开发期 A/B 测试读取分块求解的退出原因；正式路径只使用
/// [`refine_closed_form_boundary_alpha`] 的 alpha 输出。
#[cfg(test)]
pub(crate) fn refine_closed_form_boundary_alpha_for_ab(
    rgb: &RgbImage,
    mask: Vec<f32>,
) -> (Vec<f32>, ClosedFormRefineDiagnostics) {
    refine_closed_form_boundary_alpha_with_diagnostics(rgb, mask)
}

#[derive(Debug, Default)]
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) struct ClosedFormRefineDiagnostics {
    candidate_tiles: usize,
    solved_tiles: usize,
    anchored_unknown_rows: usize,
    skipped_one_sided_tiles: usize,
    invalid_matrix_tiles: usize,
    unstable_solver_tiles: usize,
}

fn refine_closed_form_boundary_alpha_with_diagnostics(
    rgb: &RgbImage,
    mask: Vec<f32>,
) -> (Vec<f32>, ClosedFormRefineDiagnostics) {
    const MIN_LONG_SIDE: u32 = 1600;
    const TRIMAP_RADIUS: usize = 4;
    const TILE_CORE: usize = 512;
    const TILE_PAD: usize = 16;

    let (width, height) = rgb.dimensions();
    let (w, h) = (width as usize, height as usize);
    if width.max(height) < MIN_LONG_SIDE || mask.len() != w.saturating_mul(h) {
        return (mask, ClosedFormRefineDiagnostics::default());
    }
    let Some(main) = cf_largest_component(&mask, w, h) else {
        return (mask, ClosedFormRefineDiagnostics::default());
    };
    let main_pixels = main.iter().filter(|value| **value).count();
    // 近乎整图或极小的“主件”不具备可靠的前/背景锚点，直接保持模型 alpha。
    if main_pixels < 512 || main_pixels * 100 > mask.len() * 95 {
        return (mask, ClosedFormRefineDiagnostics::default());
    }
    let foreground_known = cf_erode(&main, w, h, TRIMAP_RADIUS);
    let dilated = cf_dilate(&main, w, h, TRIMAP_RADIUS);
    let background_known: Vec<bool> = dilated.into_iter().map(|value| !value).collect();
    let unknown: Vec<bool> = foreground_known
        .iter()
        .zip(background_known.iter())
        .map(|(foreground, background)| !foreground && !background)
        .collect();
    if unknown.iter().filter(|value| **value).count() < 64 {
        return (mask, ClosedFormRefineDiagnostics::default());
    }

    let mut refined = mask;
    let mut diagnostics = ClosedFormRefineDiagnostics::default();
    // 各块的迭代初值与锚点统一读自求解前的快照：块核心区互不相交，写回
    // 结果与调度顺序无关，多线程与单线程产物逐字节一致；同时避免并行时
    // 对共享掩码的读写竞争。
    let snapshot = refined.clone();
    let mut jobs = Vec::new();
    for top in (0..h).step_by(TILE_CORE) {
        let bottom = (top + TILE_CORE).min(h);
        for left in (0..w).step_by(TILE_CORE) {
            let right = (left + TILE_CORE).min(w);
            if !(top..bottom).any(|y| {
                unknown[y * w + left..y * w + right]
                    .iter()
                    .any(|value| *value)
            }) {
                continue;
            }
            jobs.push((left, top, right, bottom));
        }
    }
    let solutions: Vec<((usize, usize, usize, usize), CfTileOutcome, Vec<(usize, f32)>)> = jobs
        .par_iter()
        .map(|&(left, top, right, bottom)| {
            let padded_top = top.saturating_sub(TILE_PAD);
            let padded_bottom = (bottom + TILE_PAD).min(h);
            let padded_left = left.saturating_sub(TILE_PAD);
            let padded_right = (right + TILE_PAD).min(w);
            let (outcome, patch) = cf_solve_tile(
                rgb,
                &foreground_known,
                &background_known,
                &unknown,
                &snapshot,
                w,
                h,
                left,
                top,
                right,
                bottom,
                padded_left,
                padded_top,
                padded_right,
                padded_bottom,
            );
            ((left, top, right, bottom), outcome, patch)
        })
        .collect();
    for (_, outcome, patch) in solutions {
        diagnostics.candidate_tiles += 1;
        for (index, value) in patch {
            refined[index] = value;
        }
        match outcome {
            CfTileOutcome::Solved { anchored_rows } => {
                diagnostics.solved_tiles += 1;
                diagnostics.anchored_unknown_rows += anchored_rows;
            }
            CfTileOutcome::OneSidedTrimap => diagnostics.skipped_one_sided_tiles += 1,
            CfTileOutcome::InvalidMatrix => diagnostics.invalid_matrix_tiles += 1,
            CfTileOutcome::UnstableSolver => diagnostics.unstable_solver_tiles += 1,
        }
    }
    (refined, diagnostics)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CfTileOutcome {
    Solved { anchored_rows: usize },
    OneSidedTrimap,
    InvalidMatrix,
    UnstableSolver,
}

fn cf_largest_component(mask: &[f32], w: usize, h: usize) -> Option<Vec<bool>> {
    if mask.len() != w.saturating_mul(h) || w == 0 || h == 0 {
        return None;
    }
    let mut visited = vec![false; mask.len()];
    let mut largest = Vec::<usize>::new();
    for start in 0..mask.len() {
        if visited[start] || mask[start] < 0.5 {
            continue;
        }
        let mut component = vec![start];
        visited[start] = true;
        let mut cursor = 0;
        while cursor < component.len() {
            let index = component[cursor];
            cursor += 1;
            let x = index % w;
            let y = index / w;
            for neighbor in [
                x.checked_sub(1).map(|nx| y * w + nx),
                (x + 1 < w).then_some(y * w + x + 1),
                y.checked_sub(1).map(|ny| ny * w + x),
                (y + 1 < h).then_some((y + 1) * w + x),
            ]
            .into_iter()
            .flatten()
            {
                if !visited[neighbor] && mask[neighbor] >= 0.5 {
                    visited[neighbor] = true;
                    component.push(neighbor);
                }
            }
        }
        if component.len() > largest.len() {
            largest = component;
        }
    }
    if largest.is_empty() {
        return None;
    }
    let mut result = vec![false; mask.len()];
    for index in largest {
        result[index] = true;
    }
    Some(result)
}

fn cf_dilate(input: &[bool], w: usize, h: usize, iterations: usize) -> Vec<bool> {
    let mut current = input.to_vec();
    for _ in 0..iterations {
        let mut next = vec![false; current.len()];
        for y in 0..h {
            for x in 0..w {
                let index = y * w + x;
                next[index] = current[index]
                    || (x > 0 && current[index - 1])
                    || (x + 1 < w && current[index + 1])
                    || (y > 0 && current[index - w])
                    || (y + 1 < h && current[index + w]);
            }
        }
        current = next;
    }
    current
}

fn cf_erode(input: &[bool], w: usize, h: usize, iterations: usize) -> Vec<bool> {
    let mut current = input.to_vec();
    for _ in 0..iterations {
        let mut next = vec![false; current.len()];
        for y in 0..h {
            for x in 0..w {
                let index = y * w + x;
                if x == 0 || y == 0 || x + 1 == w || y + 1 == h {
                    next[index] = false;
                    continue;
                }
                next[index] = current[index]
                    && current[index - 1]
                    && current[index + 1]
                    && current[index - w]
                    && current[index + w];
            }
        }
        current = next;
    }
    current
}

#[allow(clippy::too_many_arguments)]
/// 纯求解：初值/锚点读自 `snapshot`（求解前的掩码），不读也不写共享的
/// 输出缓冲；解出的未知像素以 (全局索引, alpha) 补丁返回，由调用方写回。
fn cf_solve_tile(
    rgb: &RgbImage,
    foreground_known: &[bool],
    background_known: &[bool],
    unknown: &[bool],
    snapshot: &[f32],
    image_w: usize,
    image_h: usize,
    core_left: usize,
    core_top: usize,
    core_right: usize,
    core_bottom: usize,
    left: usize,
    top: usize,
    right: usize,
    bottom: usize,
) -> (CfTileOutcome, Vec<(usize, f32)>) {
    const UNMAPPED: usize = usize::MAX;
    const EPSILON: f64 = 1e-7;
    const MAX_ITERATIONS: usize = 3_000;
    const RELATIVE_TOLERANCE: f64 = 1e-6;

    let tile_w = right - left;
    let tile_h = bottom - top;
    if tile_w < 3 || tile_h < 3 || right > image_w || bottom > image_h {
        return (CfTileOutcome::InvalidMatrix, Vec::new());
    }
    let tile_len = tile_w * tile_h;
    let mut ids = vec![UNMAPPED; tile_len];
    let mut unknown_local_positions = Vec::new();
    let mut unknown_count = 0;
    let mut has_foreground = false;
    let mut has_background = false;
    for y in 0..tile_h {
        for x in 0..tile_w {
            let global = (top + y) * image_w + left + x;
            let local = y * tile_w + x;
            if unknown[global] {
                ids[local] = unknown_count;
                unknown_local_positions.push(local);
                unknown_count += 1;
            } else {
                has_foreground |= foreground_known[global];
                has_background |= background_known[global];
            }
        }
    }
    if unknown_count == 0 || !has_foreground || !has_background {
        return (CfTileOutcome::OneSidedTrimap, Vec::new());
    }
    let mut colors = vec![[0.0f64; 3]; tile_len];
    for y in 0..tile_h {
        for x in 0..tile_w {
            let pixel = rgb.get_pixel((left + x) as u32, (top + y) as u32);
            colors[y * tile_w + x] = [
                pixel[0] as f64 / 255.0,
                pixel[1] as f64 / 255.0,
                pixel[2] as f64 / 255.0,
            ];
        }
    }
    let mut rows: Vec<HashMap<usize, f64>> = (0..unknown_count)
        .map(|_| HashMap::with_capacity(32))
        .collect();
    let mut b = vec![0.0f64; unknown_count];

    // Levin et al. 的 3×3 closed-form matting Laplacian。只保留 unknown 行，
    // 因而复杂度随窄 trimap 带增长，而不是随整张 4K 图片增长。
    for cy in 1..tile_h - 1 {
        for cx in 1..tile_w - 1 {
            let mut window_has_unknown = false;
            for wy in cy - 1..=cy + 1 {
                for wx in cx - 1..=cx + 1 {
                    if ids[wy * tile_w + wx] != UNMAPPED {
                        window_has_unknown = true;
                    }
                }
            }
            if !window_has_unknown {
                continue;
            }
            let mut mean = [0.0f64; 3];
            for wy in cy - 1..=cy + 1 {
                for wx in cx - 1..=cx + 1 {
                    let color = colors[wy * tile_w + wx];
                    for channel in 0..3 {
                        mean[channel] += color[channel] / 9.0;
                    }
                }
            }
            let mut covariance = [[0.0f64; 3]; 3];
            for wy in cy - 1..=cy + 1 {
                for wx in cx - 1..=cx + 1 {
                    let color = colors[wy * tile_w + wx];
                    let d = [color[0] - mean[0], color[1] - mean[1], color[2] - mean[2]];
                    for row in 0..3 {
                        for column in 0..3 {
                            covariance[row][column] += d[row] * d[column];
                        }
                    }
                }
            }
            for axis in 0..3 {
                covariance[axis][axis] += EPSILON;
                for other in 0..3 {
                    covariance[axis][other] /= 9.0;
                }
            }
            let Some(inverse) = cf_inverse_3x3(covariance) else {
                continue;
            };
            for iy in cy - 1..=cy + 1 {
                for ix in cx - 1..=cx + 1 {
                    let local_i = iy * tile_w + ix;
                    let row_id = ids[local_i];
                    if row_id == UNMAPPED {
                        continue;
                    }
                    let color_i = colors[local_i];
                    let di = [
                        color_i[0] - mean[0],
                        color_i[1] - mean[1],
                        color_i[2] - mean[2],
                    ];
                    for jy in cy - 1..=cy + 1 {
                        for jx in cx - 1..=cx + 1 {
                            let local_j = jy * tile_w + jx;
                            let color_j = colors[local_j];
                            let dj = [
                                color_j[0] - mean[0],
                                color_j[1] - mean[1],
                                color_j[2] - mean[2],
                            ];
                            let mut dot = 0.0;
                            for row in 0..3 {
                                for column in 0..3 {
                                    dot += di[row] * inverse[row][column] * dj[column];
                                }
                            }
                            let value =
                                if local_i == local_j { 1.0 } else { 0.0 } - (1.0 + dot) / 9.0;
                            let column_id = ids[local_j];
                            if column_id == UNMAPPED {
                                let global_j = (top + jy) * image_w + left + jx;
                                let alpha = if foreground_known[global_j] { 1.0 } else { 0.0 };
                                b[row_id] -= value * alpha;
                            } else {
                                *rows[row_id].entry(column_id).or_insert(0.0) += value;
                            }
                        }
                    }
                }
            }
        }
    }

    let mut sparse_rows: Vec<Vec<(usize, f64)>> = rows
        .into_iter()
        .map(|row| {
            let mut entries: Vec<_> = row
                .into_iter()
                .filter(|(_, value)| value.is_finite() && value.abs() > 1e-14)
                .collect();
            // HashMap 仅用于累计窗口贡献；求解时固定列顺序，避免不同进程的哈希
            // 随机种子令同一张图出现不必要的浮点累加差异。
            entries.sort_unstable_by_key(|(column, _)| *column);
            entries
        })
        .collect();
    let mut diagonal = vec![0.0f64; unknown_count];
    let mut anchored_rows = 0;
    for (row_id, row) in sparse_rows.iter_mut().enumerate() {
        diagonal[row_id] = row
            .iter()
            .find_map(|(column, value)| (*column == row_id).then_some(*value))
            .unwrap_or(0.0);
        if !diagonal[row_id].is_finite() || diagonal[row_id].abs() < 1e-12 {
            // 分块边缘偶尔会落在没有完整 3×3 窗口的未知像素上。Python 参考
            // 实现借由 IChol 的移位继续处理同一块；这里显式把这种“没有方程”的
            // 像素锁为原模型 alpha，避免整块被丢弃，同时不凭空扩张前景。
            let local = unknown_local_positions[row_id];
            let local_x = local % tile_w;
            let local_y = local / tile_w;
            let original_alpha =
                snapshot[(top + local_y) * image_w + left + local_x].clamp(0.0, 1.0) as f64;
            row.clear();
            row.push((row_id, 1.0));
            diagonal[row_id] = 1.0;
            b[row_id] = original_alpha;
            anchored_rows += 1;
        }
    }
    let mut x = vec![0.0f64; unknown_count];
    for y in 0..tile_h {
        for x_local in 0..tile_w {
            let id = ids[y * tile_w + x_local];
            if id != UNMAPPED {
                x[id] = snapshot[(top + y) * image_w + left + x_local].clamp(0.0, 1.0) as f64;
            }
        }
    }
    let multiply = |vector: &[f64]| -> Vec<f64> {
        sparse_rows
            .iter()
            .map(|row| {
                row.iter()
                    .map(|(column, value)| value * vector[*column])
                    .sum()
            })
            .collect()
    };
    let mut residual: Vec<f64> = b
        .iter()
        .zip(multiply(&x))
        .map(|(right, left)| right - left)
        .collect();
    let norm_b = b
        .iter()
        .map(|value| value * value)
        .sum::<f64>()
        .sqrt()
        .max(1e-12);
    let mut z: Vec<f64> = residual
        .iter()
        .zip(diagonal.iter())
        .map(|(value, diagonal)| value / diagonal)
        .collect();
    let mut direction = z.clone();
    let mut rz = residual
        .iter()
        .zip(z.iter())
        .map(|(a, b)| a * b)
        .sum::<f64>();
    if !rz.is_finite() {
        return (CfTileOutcome::UnstableSolver, Vec::new());
    }
    for _ in 0..MAX_ITERATIONS {
        let applied = multiply(&direction);
        let denominator = direction
            .iter()
            .zip(applied.iter())
            .map(|(a, b)| a * b)
            .sum::<f64>();
        if !denominator.is_finite() || denominator.abs() < 1e-18 {
            return (CfTileOutcome::UnstableSolver, Vec::new());
        }
        let step = rz / denominator;
        if !step.is_finite() {
            return (CfTileOutcome::UnstableSolver, Vec::new());
        }
        for index in 0..unknown_count {
            x[index] += step * direction[index];
            residual[index] -= step * applied[index];
        }
        let residual_norm = residual
            .iter()
            .map(|value| value * value)
            .sum::<f64>()
            .sqrt();
        if residual_norm <= norm_b * RELATIVE_TOLERANCE {
            break;
        }
        for index in 0..unknown_count {
            z[index] = residual[index] / diagonal[index];
        }
        let next_rz = residual
            .iter()
            .zip(z.iter())
            .map(|(a, b)| a * b)
            .sum::<f64>();
        if !next_rz.is_finite() || rz.abs() < 1e-18 {
            return (CfTileOutcome::UnstableSolver, Vec::new());
        }
        let beta = next_rz / rz;
        for index in 0..unknown_count {
            direction[index] = z[index] + beta * direction[index];
        }
        rz = next_rz;
    }
    let mut patch = Vec::new();
    for y in core_top..core_bottom {
        for x_local in core_left..core_right {
            let local = (y - top) * tile_w + x_local - left;
            let id = ids[local];
            if id != UNMAPPED && x[id].is_finite() {
                patch.push((y * image_w + x_local, x[id].clamp(0.0, 1.0) as f32));
            }
        }
    }
    (CfTileOutcome::Solved { anchored_rows }, patch)
}

fn cf_inverse_3x3(matrix: [[f64; 3]; 3]) -> Option<[[f64; 3]; 3]> {
    let a = matrix[0][0];
    let b = matrix[0][1];
    let c = matrix[0][2];
    let d = matrix[1][0];
    let e = matrix[1][1];
    let f = matrix[1][2];
    let g = matrix[2][0];
    let h = matrix[2][1];
    let i = matrix[2][2];
    let determinant = a * (e * i - f * h) - b * (d * i - f * g) + c * (d * h - e * g);
    if !determinant.is_finite() || determinant.abs() < 1e-20 {
        return None;
    }
    let scale = 1.0 / determinant;
    Some([
        [
            (e * i - f * h) * scale,
            (c * h - b * i) * scale,
            (b * f - c * e) * scale,
        ],
        [
            (f * g - d * i) * scale,
            (a * i - c * g) * scale,
            (c * d - a * f) * scale,
        ],
        [
            (d * h - e * g) * scale,
            (b * g - a * h) * scale,
            (a * e - b * d) * scale,
        ],
    ])
}

/// 将任意模型的全尺寸 alpha 套入统一的正式后处理与去污染步骤。
/// 单独抽出是为了让开发期的本地模型验证也能与正式输出逐像素同口径比较；
/// 它不改变任何正式模型选择、下载或 UI 行为。
pub(crate) fn finalize_cutout_image(
    rgb: &RgbImage,
    mask: Vec<f32>,
    preserve_native_edges: bool,
) -> RgbaImage {
    finalize_cutout_image_with_alpha_gamma(rgb, mask, preserve_native_edges, 1.0)
}

/// `alpha_gamma` 是开发期边缘校准实验的显式入口。正式流程固定传入 1.0；
/// 只有人工真值证明某个曲线能在保住主体的同时减少边缘外溢，才会讨论产品化。
pub(crate) fn finalize_cutout_image_with_alpha_gamma(
    rgb: &RgbImage,
    mask: Vec<f32>,
    preserve_native_edges: bool,
    alpha_gamma: f32,
) -> RgbaImage {
    let (w, h) = rgb.dimensions();
    // 引导滤波：低分辨率推理的软边掩码贴回原图结构，过渡带收窄、糊住的
    // 发丝尖端分开；先于残留清理执行，滤波沿背景线条的微溢出由后续清理兜底。
    let mut mask = if !preserve_native_edges && ab_postprocess_stage_enabled("guided") {
        timed("f1 guided", || guided_filter_matte(rgb, &mask, 8, 5e-4))
    } else {
        mask
    };
    // 颜色证据整定：把模型低置信度的细结构按全分辨率颜色归类到实心/透明。
    if ab_postprocess_stage_enabled("solidify") {
        timed("f2 solidify", || solidify_subject(rgb, &mut mask, w, h));
    }

    // 幽灵残留抑制先于去污染：碎屑清除后，过渡带背景色估计更准。
    if ab_postprocess_stage_enabled("ghost") {
        timed("f3 ghost", || suppress_background_ghosts(&mut mask, w, h));
    }
    // 极淡残雾归零：上采样振铃和滤波残余的极低 alpha 在换底上呈灰雾。
    if ab_postprocess_stage_enabled("threshold") {
        timed("f4 threshold", || {
            let floor = low_alpha_floor();
            for value in mask.iter_mut() {
                if *value < floor {
                    *value = 0.0;
                }
            }
        });
    }
    // 再清一轮孤岛：残留中与主体不连通的小碎块（线稿笔触、噪点）整块移除，
    // 与 advanced 管线共用同一面积尺度。
    if ab_postprocess_stage_enabled("components") {
        timed("f5 components", || {
            remove_small_foreground_components(&mut mask, w, h, advanced_min_component_area(w, h))
        });
    }
    // 背景次级孤岛：面积小且远离主体的独立前景块（背景人物等）整块移除。
    if ab_postprocess_stage_enabled("islands") {
        timed("f6 islands", || remove_background_islands(&mut mask, w, h));
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
    let colors = timed("f7 decontaminate", || decontaminate_colors(rgb, &mask));
    let mut raw = vec![0u8; (w as usize) * (h as usize) * 4];
    timed("f8 composite", || {
        raw.par_chunks_exact_mut(4).enumerate().for_each(|(index, pixel)| {
            let alpha = (mask[index] * 255.0).round().clamp(0.0, 255.0) as u8;
            let color = colors[index];
            pixel[0] = color[0];
            pixel[1] = color[1];
            pixel[2] = color[2];
            pixel[3] = alpha;
        });
    });
    RgbaImage::from_raw(w, h, raw).expect("composite buffer size matches")
}

#[cfg(test)]
mod color_regression_tests {
    use super::*;
    #[test]
    fn uncertain_foreground_does_not_desaturate_itself() {
        let rgb = RgbImage::from_pixel(40, 40, image::Rgb([160, 100, 220]));
        for alpha in [0.02, 0.1, 0.5, 0.9, 1.0] {
            let colors = decontaminate_colors(&rgb, &vec![alpha; 1600]);
            assert!(colors.iter().all(|c| *c == [160, 100, 220]));
        }
    }
    #[test]
    fn low_alpha_with_matching_background_keeps_original_color() {
        let rgb = RgbImage::from_pixel(40, 40, image::Rgb([160, 100, 220]));
        let mut alpha = vec![0.0; 1600];
        alpha[820] = 0.08;
        assert_eq!(decontaminate_colors(&rgb, &alpha)[820], [160, 100, 220]);
    }
    #[test]
    fn edge_correction_is_bounded_and_solid_pixels_are_unchanged() {
        let mut rgb = RgbImage::from_pixel(40, 40, image::Rgb([255, 255, 255]));
        rgb.put_pixel(20, 20, image::Rgb([80, 120, 160]));
        let mut alpha = vec![0.0; 1600];
        alpha[820] = 0.1;
        let colors = decontaminate_colors(&rgb, &alpha);
        for channel in 0..3 {
            assert!((colors[820][channel] as i16 - rgb.get_pixel(20, 20)[channel] as i16).abs() <= 24);
        }
        alpha[820] = 242.0 / 255.0;
        assert_eq!(decontaminate_colors(&rgb, &alpha)[820], [80, 120, 160]);
    }

    #[test]
    #[ignore = "requires test-area source and existing matte"]
    fn color_ab_existing_matte() {
        let input = std::path::PathBuf::from(std::env::var_os("AIAS_COLOR_INPUT").unwrap());
        let baseline = std::path::PathBuf::from(std::env::var_os("AIAS_COLOR_BASELINE").unwrap());
        let output = std::path::PathBuf::from(std::env::var_os("AIAS_COLOR_OUTPUT").unwrap());
        let rgb = image::open(input).unwrap().to_rgb8();
        let mut result = image::open(baseline).unwrap().to_rgba8();
        assert_eq!(rgb.dimensions(), result.dimensions());
        let mask: Vec<f32> = result.pixels().map(|p| p[3] as f32 / 255.0).collect();
        let colors = decontaminate_colors(&rgb, &mask);
        for (pixel, color) in result.pixels_mut().zip(colors) {
            pixel[0] = color[0]; pixel[1] = color[1]; pixel[2] = color[2];
        }
        result.save(output).unwrap();
    }
}
