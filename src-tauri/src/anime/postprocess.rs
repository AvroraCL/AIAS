//! `anime::postprocess` — 拆分自 anime.rs，职责见模块内条目注释。

use super::*;
use image::{RgbImage, Rgba, RgbaImage};


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
pub(crate) fn box_mean_f64(values: &[f64], w: usize, h: usize, radius: usize) -> Vec<f64> {
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
pub(crate) fn guided_filter_matte(rgb: &RgbImage, p: &[f32], radius: usize, eps: f64) -> Vec<f32> {
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
