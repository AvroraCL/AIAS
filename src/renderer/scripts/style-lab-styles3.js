// 风格实验室第三批引擎：水波纹/玻璃折射/马赛克拼贴/热浪扭曲/蜡笔粉彩/幻彩全息。
// 全部纯函数，从 style-lab.js 的 renderRows 分发调用。
import { mulberry32, makeFbm, makeNoise2D } from './style-lab-state.mjs';

export function prepareRipple(src, w, h, p) {
  return { gray: toGrayArr(src, w, h) };
}

export function prepareGlass(src, w, h, p) {
  const gray = toGrayArr(src, w, h);
  return { gray, blur: boxBlurG(gray, w, h, Math.max(1, Math.round(p.blur / 5))) };
}

export function prepareHeatwave(src, w, h, p) {
  return {};
}

export function preparePastel(src, w, h, p) {
  const gray = toGrayArr(src, w, h);
  const blur = boxBlurG(gray, w, h, Math.max(2, Math.round(Math.min(w, h) / 80)));
  return { gray, blur };
}

export function prepareHolo(src, w, h, p) {
  const gray = toGrayArr(src, w, h);
  const blur = boxBlurG(gray, w, h, Math.max(2, Math.round(Math.min(w, h) / 60)));
  return { gray, blur };
}

function toGrayArr(src, w, h) {
  const out = new Float32Array(w * h);
  for (let i = 0; i < w * h; i++) {
    out[i] = 0.2126 * src[i * 4] + 0.7152 * src[i * 4 + 1] + 0.0722 * src[i * 4 + 2];
  }
  return out;
}

function boxBlurG(src, w, h, radius) {
  const tmp = new Float32Array(w * h);
  const out = new Float32Array(w * h);
  for (let y = 0; y < h; y++) {
    let acc = 0;
    for (let x = -radius; x <= radius; x++) acc += src[y * w + Math.min(w - 1, Math.max(0, x))];
    for (let x = 0; x < w; x++) {
      tmp[y * w + x] = acc / (2 * radius + 1);
      acc += src[y * w + Math.min(w - 1, x + radius + 1)] - src[y * w + Math.max(0, x - radius)];
    }
  }
  for (let x = 0; x < w; x++) {
    let acc = 0;
    for (let y = -radius; y <= radius; y++) acc += tmp[Math.min(h - 1, Math.max(0, y)) * w + x];
    for (let y = 0; y < h; y++) {
      out[y * w + x] = acc / (2 * radius + 1);
      acc += tmp[Math.min(h - 1, y + radius + 1) * w + x] - tmp[Math.max(0, y - radius) * w + x];
    }
  }
  return out;
}

// 水波纹：正弦波纹偏移采样模拟水面折射。
export function renderRipple(dst, src, w, h, y0, y1, p, pre) {
  const amp = p.amplitude || 18;
  const wl = p.wavelength || 32;
  const speed = p.speed / 100;
  const mix = p.mix / 100;
  const time = Date.now() * 0.001 * speed;
  for (let y = y0; y < y1; y++) {
    for (let x = 0; x < w; x++) {
      const i = (y * w + x) * 4;
      const dx = Math.sin((x + time * 60) * (6.28 / wl)) * amp;
      const dy = Math.cos((y + time * 60) * (6.28 / (wl * 1.3))) * amp;
      const sx = Math.min(w - 1, Math.max(0, x + Math.round(dx)));
      const sy = Math.min(h - 1, Math.max(0, y + Math.round(dy)));
      const si = (sy * w + sx) * 4;
      for (let c = 0; c < 3; c++) {
        dst[i + c] = src[si + c] * mix + src[i + c] * (1 - mix);
      }
      dst[i + 3] = src[i + 3];
    }
  }
}

// 玻璃折射：用模糊灰度梯度偏移采样，模拟厚玻璃的折射变形。
export function renderGlassR(dst, src, w, h, y0, y1, p, pre) {
  const ref = p.refraction / 100;
  const tint = p.tint / 100;
  const blur = pre.blur;
  for (let y = y0; y < y1; y++) {
    for (let x = 0; x < w; x++) {
      const i = (y * w + x) * 4;
      const xL = Math.max(0, x - 1), xR = Math.min(w - 1, x + 1);
      const yT = Math.max(0, y - 1), yB = Math.min(h - 1, y + 1);
      const gx = (blur[y * w + xR] - blur[y * w + xL]) * ref * 3;
      const gy = (blur[yB * w + x] - blur[yT * w + x]) * ref * 3;
      const sx = Math.min(w - 1, Math.max(0, x + Math.round(gx)));
      const sy = Math.min(h - 1, Math.max(0, y + Math.round(gy)));
      const si = (sy * w + sx) * 4;
      dst[i] = src[si] * (1 - tint * 0.1) + 12 * tint;
      dst[i + 1] = src[si + 1] * (1 - tint * 0.05) + 8 * tint;
      dst[i + 2] = src[si + 2] * (1 - tint * 0.02) + 20 * tint;
      dst[i + 3] = src[i + 3];
    }
  }
}

// 马赛克拼贴：网格砖块 + 缝隙 + 抖动旋转偏移。
export function renderMosaic(dst, src, w, h, y0, y1, p) {
  const tile = Math.max(3, Math.round(p.tile));
  const gap = Math.round(p.gap);
  const jit = p.jitter / 100;
  const grout = p.grout / 100;
  const rand = mulberry32(5678);
  for (let y = y0; y < y1; y++) {
    for (let x = 0; x < w; x++) {
      const i = (y * w + x) * 4;
      const cellX = Math.floor(x / tile), cellY = Math.floor(y / tile);
      const lx = x % tile, ly = y % tile;
      if (gap > 0 && (lx < gap || ly < gap)) {
        const groutV = Math.round(60 + rand() * 20 * grout);
        dst[i] = dst[i + 1] = dst[i + 2] = Math.max(0, groutV - grout * 30);
        dst[i + 3] = src[i + 3];
        continue;
      }
      const jx = Math.round((rand() - 0.5) * jit * tile * 0.15);
      const jy = Math.round((rand() - 0.5) * jit * tile * 0.15);
      const sx = Math.min(w - 1, Math.max(0, cellX * tile + (tile >> 1) + jx));
      const sy = Math.min(h - 1, Math.max(0, cellY * tile + (tile >> 1) + jy));
      const si = (sy * w + sx) * 4;
      dst[i] = src[si]; dst[i + 1] = src[si + 1]; dst[i + 2] = src[si + 2];
      dst[i + 3] = src[i + 3];
    }
  }
}

// 热浪扭曲：正弦列偏移 + 亮度微调，模拟热空气上升导致的视觉扭曲。
export function renderHeatwave(dst, src, w, h, y0, y1, p) {
  const strength = p.strength / 100;
  const speed = p.speed / 100;
  const freq = p.freq / 100;
  const time = Date.now() * 0.001 * (0.5 + speed);
  for (let y = y0; y < y1; y++) {
    const offset = Math.sin(y * freq * 0.08 + time * 3) * strength * 12;
    for (let x = 0; x < w; x++) {
      const i = (y * w + x) * 4;
      const sx = Math.min(w - 1, Math.max(0, x + Math.round(offset)));
      const si = (y * w + sx) * 4;
      dst[i] = src[si]; dst[i + 1] = src[si + 1]; dst[i + 2] = src[si + 2];
      dst[i + 3] = src[i + 3];
    }
  }
}

// 蜡笔粉彩：柔化 + 蜡笔颗粒 + 亮部晕开 + 纸纹。
export function renderPastel(dst, src, w, h, y0, y1, p, pre) {
  const soft = p.softness / 100;
  const grain = p.grain / 100;
  const bloom = p.bloom / 100;
  const paper = p.paper / 100;
  const rand = mulberry32(4141);
  for (let y = y0; y < y1; y++) {
    for (let x = 0; x < w; x++) {
      const i = (y * w + x) * 4;
      // blur 是每像素单通道（步长 1），i 是 RGBA 步长 4
      const softV = pre.blur[i / 4] / 255;
      const grainN = (rand() - 0.5) * grain * 30;
      const paperN = (rand() - 0.5) * paper * 16;
      for (let c = 0; c < 3; c++) {
        const orig = src[i + c] / 255;
        const blurred = softV;
        let v = orig * (1 - soft * 0.4) + blurred * soft * 0.4;
        v = v + (255 - v) * bloom * 0.15;
        v += grainN * 0.5 + paperN;
        dst[i + c] = Math.max(0, Math.min(255, v));
      }
      dst[i + 3] = src[i + 3];
    }
  }
}

// 幻彩全息：按位置渐变色相偏移 + 扫描线 + 闪光噪声，模拟全息薄膜。
export function renderHolo(dst, src, w, h, y0, y1, p, pre) {
  const intensity = p.intensity / 100;
  const spectrum = p.spectrum / 100;
  const scan = p.scanline / 100;
  const shimmer = p.shimmer / 100;
  const time = Date.now() * 0.001;
  const rand = mulberry32(8080);
  for (let y = y0; y < y1; y++) {
    for (let x = 0; x < w; x++) {
      const i = (y * w + x) * 4;
      const lum = pre.gray[y * w + x] / 255;
      const hueShift = Math.sin(x * 0.02 + y * 0.03 + time * 2) * spectrum * 180 + x * 0.15;
      const h = ((hueShift % 360) + 360) % 360;
      const s = 0.4 + intensity * 0.5;
      const v = lum * (0.7 + intensity * 0.5);
      // HSV→RGB 简化
      const c = v * s;
      const hp = h / 60;
      const xx = c * (1 - Math.abs(hp % 2 - 1));
      let r = 0, g = 0, b = 0;
      if (hp < 1) { r = c; g = xx; } else if (hp < 2) { r = xx; g = c; }
      else if (hp < 3) { g = c; b = xx; } else if (hp < 4) { g = xx; b = c; }
      else if (hp < 5) { r = xx; b = c; } else { r = c; b = xx; }
      const m = v - c;
      let or = (r + m) * 255, og = (g + m) * 255, ob = (b + m) * 255;
      if (scan > 0 && y % 3 === 0) { or *= (1 - scan); og *= (1 - scan); ob *= (1 - scan); }
      if (shimmer > 0) {
        const sn = (rand() - 0.5) * shimmer * 40;
        or += sn; og += sn; ob += sn;
      }
      dst[i] = Math.max(0, Math.min(255, or * intensity + src[i] * (1 - intensity)));
      dst[i + 1] = Math.max(0, Math.min(255, og * intensity + src[i + 1] * (1 - intensity)));
      dst[i + 2] = Math.max(0, Math.min(255, ob * intensity + src[i + 2] * (1 - intensity)));
      dst[i + 3] = src[i + 3];
    }
  }
}
