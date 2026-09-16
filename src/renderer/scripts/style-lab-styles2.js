// 风格实验室第二批引擎：双色调/水彩/低多边形/像素画/版画木刻/胶片颗粒。
// 均为纯函数，从 style-lab.js 的 renderRows 分发调用。

export function hexRGB(hex) {
  const m = hex.match(/^#([0-9a-f]{2})([0-9a-f]{2})([0-9a-f]{2})$/i);
  return m ? [parseInt(m[1], 16), parseInt(m[2], 16), parseInt(m[3], 16)] : [26, 26, 46];
}

// 双色调：暗部映射 shadow 色、亮部映射 highlight 色。
export function renderDuotone(dst, w, h, y0, y1, p, gray) {
  const shadow = hexRGB(p.shadow || '#1a1a2e');
  const highlight = hexRGB(p.highlight || '#e8c547');
  const mid = (p.midpoint || 50) / 100;
  const soft = 1 + (p.softness || 30) / 25;
  for (let y = y0; y < y1; y++) {
    for (let x = 0; x < w; x++) {
      const i = (y * w + x) * 4;
      const g = Math.max(0, Math.min(1, (gray[i] / 255 - mid) * soft + 0.5));
      dst[i] = shadow[0] + (highlight[0] - shadow[0]) * g;
      dst[i + 1] = shadow[1] + (highlight[1] - shadow[1]) * g;
      dst[i + 2] = shadow[2] + (highlight[2] - shadow[2]) * g;
    }
  }
}

// 水彩晕染：多遍 fBm 驱动颜色偏移模拟颜料浸润。
export function renderWatercolor(dst, src, w, h, y0, y1, p, pre) {
  const bleed = p.bleed / 100;
  const edgeK = p.edge / 100;
  const paper = p.paper / 100;
  const sat = p.saturation / 100;
  const rand = mulberry32(919);
  const washes = p.washes || 3;
  const fbm = makeFbm(7777, 4);
  for (let y = y0; y < y1; y++) {
    for (let x = 0; x < w; x++) {
      const i = (y * w + x) * 4;
      let offset = 0;
      for (let w2 = 0; w2 < washes; w2++) {
        offset += fbm(x * 0.008 * (w2 + 1), y * 0.008 * (w2 + 1)) * bleed * 14;
      }
      const n1 = fbm(x * 0.015, y * 0.015);
      const n2 = fbm(x * 0.035 + 100, y * 0.035 - 50);
      const sx = Math.min(w - 1, Math.max(0, x + Math.round((n1 - 0.5) * offset)));
      const sy = Math.min(h - 1, Math.max(0, y + Math.round((n2 - 0.5) * offset)));
      const si = (sy * w + sx) * 4;
      const edgeMag = 0; // 简化：不含 sobel（水彩重点在偏移晕染而非边缘）
      const dark = 1 - edgeMag * edgeK * 0.65;
      const paperN = paper > 0 ? (rand() - 0.5) * paper * 0.3 : 0;
      for (let c = 0; c < 3; c++) {
        let v = src[si + c] * dark;
        const mean = (v - 128) * sat + 128;
        v = v + (mean - v) * sat * 0.5 + paperN * 6;
        dst[i + c] = Math.max(0, Math.min(255, v));
      }
      dst[i + 3] = src[i + 3];
    }
  }
}

// 低多边形：网格三角化 + 顶点抖动 + 均值填色 + 可选调色板映射。
export function renderLowpoly(dst, src, w, h, y0, y1, p, pre) {
  const cell = Math.max(4, Math.round(p.cell));
  const jit = (p.jitter / 100) * cell * 0.5;
  const flat = p.flat / 100;
  const rand = mulberry32(2024);
  const palette = p.palette === 'warm'
    ? [[180, 60, 30], [220, 130, 50], [240, 190, 90], [200, 80, 60], [120, 40, 20]]
    : p.palette === 'cool'
      ? [[20, 40, 80], [40, 90, 140], [80, 160, 180], [140, 200, 200], [200, 230, 230]]
      : extractPalette(src, p.colors || 6);
  const tri = (ax, ay, bx, by, cx, cy) => {
    let sr = 0, sg = 0, sb = 0, count = 0;
    const minX = Math.max(0, Math.floor(Math.min(ax, bx, cx)));
    const maxX = Math.min(w - 1, Math.ceil(Math.max(ax, bx, cx)));
    const minY = Math.max(0, Math.floor(Math.min(ay, by, cy)));
    const maxY = Math.min(h - 1, Math.ceil(Math.max(ay, by, cy)));
    for (let py = minY; py <= maxY; py++) {
      for (let px = minX; px <= maxX; px++) {
        const i4 = (py * w + px) * 4;
        sr += src[i4]; sg += src[i4 + 1]; sb += src[i4 + 2]; count++;
      }
    }
    if (count === 0) return;
    let r = sr / count, g = sg / count, b = sb / count;
    if (flat < 50 && palette) {
      const idx = nearestPalette(r, g, b, palette);
      r = palette[idx][0]; g = palette[idx][1]; b = palette[idx][2];
    }
    r *= 1 - flat * 0.3; g *= 1 - flat * 0.3; b *= 1 - flat * 0.3;
    drawTri(dst, w, h, ax, ay, bx, by, cx, cy, r, g, b);
  };
  for (let cy = 0; cy < Math.ceil(h / cell); cy++) {
    if (cy * cell > y1) break;
    if ((cy + 1) * cell < y0) continue;
    for (let cx = 0; cx < Math.ceil(w / cell); cx++) {
      const x0 = cx * cell, y0c = cy * cell;
      const x1 = Math.min(w, x0 + cell), y1c = Math.min(h, y0c + cell);
      const jx = () => (rand() - 0.5) * jit;
      const jy = () => (rand() - 0.5) * jit;
      tri(x0 + jx(), y0c + jy(), x1 + jx(), y0c + jy(), x0 + jx(), y1c + jy());
      tri(x1 + jx(), y0c + jy(), x1 + jx(), y1c + jy(), x0 + jx(), y1c + jy());
    }
  }
}

function nearestPalette(r, g, b, palette) {
  let best = 0, bestD = Infinity;
  for (let i = 0; i < palette.length; i++) {
    const dr = r - palette[i][0], dg = g - palette[i][1], db = b - palette[i][2];
    const d = dr * dr + dg * dg + db * db;
    if (d < bestD) { bestD = d; best = i; }
  }
  return best;
}

function drawTri(dst, w, h, ax, ay, bx, by, cx, cy, r, g, b) {
  const minX = Math.max(0, Math.floor(Math.min(ax, bx, cx)));
  const maxX = Math.min(w - 1, Math.ceil(Math.max(ax, bx, cx)));
  const minY = Math.max(0, Math.floor(Math.min(ay, by, cy)));
  const maxY = Math.min(h - 1, Math.ceil(Math.max(ay, by, cy)));
  const det = (bx - ax) * (cy - ay) - (cx - ax) * (by - ay);
  if (Math.abs(det) < 1e-8) return;
  for (let py = minY; py <= maxY; py++) {
    for (let px = minX; px <= maxX; px++) {
      const w0 = ((bx - ax) * (py + 0.5 - ay) - (by - ay) * (px + 0.5 - ax)) / det;
      const w1 = ((ax - cx) * (py + 0.5 - ay) - (ay - cy) * (px + 0.5 - ax)) / det;
      const w2 = 1 - w0 - w1;
      if (w0 >= 0 && w1 >= 0 && w2 >= 0) {
        const i = (py * w + px) * 4;
        dst[i] = r; dst[i + 1] = g; dst[i + 2] = b;
      }
    }
  }
}

// 像素画：降采样 + 调色板量化。
export function renderPixelArt(dst, src, w, h, y0, y1, p) {
  const size = Math.max(2, Math.round(p.size));
  const levels = Math.max(2, Math.round(p.levels));
  for (let y = y0; y < y1; y++) {
    for (let x = 0; x < w; x++) {
      const i = (y * w + x) * 4;
      const sx = Math.floor(x / size) * size;
      const sy = Math.floor(y / size) * size;
      const si = (Math.min(h - 1, sy) * w + Math.min(w - 1, sx)) * 4;
      let r = src[si], g = src[si + 1], b = src[si + 2];
      if (levels < 16) {
        const step = 255 / (levels - 1);
        r = Math.round(r / step) * step;
        g = Math.round(g / step) * step;
        b = Math.round(b / step) * step;
      }
      dst[i] = Math.max(0, Math.min(255, r));
      dst[i + 1] = Math.max(0, Math.min(255, g));
      dst[i + 2] = Math.max(0, Math.min(255, b));
      dst[i + 3] = src[i + 3];
    }
  }
}

// 版画木刻：定向排线按亮度疏密 + 粗糙度扰动 + 高对比。
export function renderWoodcut(dst, src, w, h, y0, y1, p, gray) {
  const lw = Math.max(1, Math.round(p.lineWidth));
  const angle = (p.angle || 0) * Math.PI / 180;
  const contrast = 1 + p.contrast / 50;
  const rough = p.roughness / 100;
  const rand = mulberry32(33);
  const cosA = Math.cos(angle), sinA = Math.sin(angle);
  for (let y = y0; y < y1; y++) {
    for (let x = 0; x < w; x++) {
      const i = y * w + x;
      let g = gray[i];
      const proj = x * cosA + y * sinA;
      const lineIdx = Math.floor(proj / (lw * 2));
      const inLine = (proj % (lw * 2)) < lw;
      const jitter = rough > 0 ? (rand() - 0.5) * rough * 60 : 0;
      g = Math.max(0, Math.min(255, (g - 128) * contrast + 128 + jitter));
      let v = inLine ? (g > 128 ? g : Math.max(0, g - 60)) : (g > 128 ? Math.min(255, g + 30) : g);
      dst[i * 4] = dst[i * 4 + 1] = dst[i * 4 + 2] = Math.max(0, Math.min(255, v));
      dst[i * 4 + 3] = 255;
    }
  }
}

// 胶片颗粒：复合噪点 + 光晕 + 褪色 + 暖调偏移。
export function renderFilm(dst, src, w, h, y0, y1, p, lum, blur) {
  const rand = mulberry32(9);
  const grain = p.grain / 100;
  const halation = p.halation / 100;
  const fade = p.fade / 100;
  const warmth = p.warmth / 100;
  for (let y = y0; y < y1; y++) {
    for (let x = 0; x < w; x++) {
      const i = (y * w + x) * 4;
      const n = (rand() - 0.5) * grain * 70;
      const gl = blur[y * w + x] / 255 * halation;
      let r = src[i * 4], g = src[i * 4 + 1], b = src[i * 4 + 2];
      r = Math.min(255, r + gl * 60);
      r = Math.min(255, r * (1 + warmth * 0.08));
      b *= 1 - warmth * 0.06;
      r = r * (1 - fade * 0.15) + fade * 42;
      g = g * (1 - fade * 0.12) + fade * 40;
      b = b * (1 - fade * 0.10) + fade * 38;
      const gn = (rand() - 0.5) * grain * 36;
      dst[i * 4] = Math.max(0, Math.min(255, r + n + gn * 0.7));
      dst[i * 4 + 1] = Math.max(0, Math.min(255, g + n * 0.7 + gn * 0.7));
      dst[i * 4 + 2] = Math.max(0, Math.min(255, b + n * 0.4 + gn * 0.4));
      dst[i * 4 + 3] = src[i * 4 + 3];
    }
  }
}
