// 风格实验室参数状态：10 套图片转风格化样式的参数白名单、默认值与恢复逻辑。
// 纯函数模块（无 DOM 依赖），node --test 可直接覆盖。

export const STYLE_IDS = ['glitch', 'camo', 'wear', 'oil', 'halftone', 'sketch', 'thermal', 'neon', 'cross', 'marble', 'duotone', 'watercolor', 'lowpoly', 'pixel', 'woodcut', 'film'];

export const STYLE_LABELS = {
  glitch: '故障艺术',
  camo: '迷彩生成',
  wear: '磨损掉漆',
  oil: '油画厚涂',
  halftone: '半调印刷',
  sketch: '素描炭笔',
  thermal: '热感假彩',
  neon: '赛博霓虹',
  cross: '十字绣',
  marble: '大理石纹',
  duotone: '双色调',
  watercolor: '水彩晕染',
  lowpoly: '低多边形',
  pixel: '像素画',
  woodcut: '版画木刻',
  film: '胶片颗粒',
};

// 每套样式的参数默认值；inspector 按此渲染，处理函数按此消费。
export const STYLE_DEFAULTS = {
  glitch: { split: 18, scanline: 35, blocks: 45, wave: 20, noise: 25 },
  camo: { pattern: 'blotch', colors: 4, scale: 42, sharp: 55, rotation: 15, contrast: 60 },
  wear: { strength: 55, edge: 65, scratches: 38, grain: 30, baseColor: 1 },
  oil: { radius: 4, levels: 7, smooth: 30 },
  halftone: { shape: 'circle', cell: 8, angle: 15, mode: 'gray', sharpen: 35 },
  sketch: { pencil: 65, edge: 55, grain: 40, invert: false },
  thermal: { lut: 'iron', mix: 85, contrast: 40 },
  neon: { edge: 70, glow: 55, hue: 185, dark: 70 },
  cross: { levels: 10, stitch: 10, fabric: 35, grid: true },
  marble: { octaves: 5, turbulence: 45, vein: 42, palette: 'auto', scale: 60 },
  duotone: { shadow: '#1a1a2e', highlight: '#e8c547', midpoint: 50, softness: 30 },
  watercolor: { bleed: 55, edge: 65, paper: 40, washes: 3, saturation: 70 },
  lowpoly: { cell: 24, jitter: 40, flat: 35, palette: 'auto', colors: 6 },
  pixel: { size: 6, levels: 6, dither: 'ordered', palette: 'auto', paletteN: 8 },
  woodcut: { lineWidth: 3, angle: 0, contrast: 65, roughness: 35 },
  film: { grain: 45, halation: 35, fade: 25, warmth: 40 },
};

export const STYLE_ENUMS = {
  camo: [['pattern', [['blotch', '斑块'], ['digital', '数码'], ['leopard', '豹纹'], ['stripe', '条纹'], ['crack', '裂纹']]]],
  halftone: [
    ['shape', [['circle', '圆点'], ['square', '方块'], ['diamond', '菱形'], ['line', '平行线'], ['cross', '十字']]],
    ['mode', [['gray', '灰度半调'], ['cmyk', '四色套印']]],
  ],
  sketch: [['invert', null]],
  thermal: [['lut', [['iron', '铁红'], ['rainbow', '彩虹'], ['nightvision', '夜视'], ['gold', '鎏金'], ['ice', '冰蓝']]]],
  cross: [['levels', null]],
  marble: [['palette', [['auto', '取色于原图'], ['blackwhite', '黑白'], ['jade', '青玉'], ['amber', '琥珀']]]],
  lowpoly: [['palette', [['auto', '取色于原图'], ['warm', '暖调'], ['cool', '冷调']]]],
  pixel: [['dither', [['none', '关闭'], ['ordered', '有序'], ['diffusion', '误差扩散']]]],
};

// 参数恢复白名单：键 → [类型, 最小, 最大]（min/max 仅数值用）。
const PARAM_RANGES = {
  glitch: { split: [10, 80], scanline: [0, 100], blocks: [0, 100], wave: [0, 100], noise: [0, 100] },
  camo: { colors: [2, 8], scale: [8, 100], sharp: [0, 100], rotation: [0, 90], contrast: [10, 100] },
  wear: { strength: [0, 100], edge: [0, 100], scratches: [0, 100], grain: [0, 100], baseColor: [0, 1] },
  oil: { radius: [1, 8], levels: [2, 16], smooth: [0, 100] },
  halftone: { cell: [3, 24], angle: [0, 90], sharpen: [0, 100] },
  sketch: { pencil: [10, 100], edge: [0, 100], grain: [0, 100] },
  thermal: { mix: [0, 100], contrast: [0, 100] },
  neon: { edge: [10, 100], glow: [0, 100], hue: [0, 360], dark: [0, 100] },
  cross: { levels: [3, 24], stitch: [4, 24], fabric: [0, 100] },
  marble: { octaves: [2, 7], turbulence: [5, 100], vein: [10, 90], scale: [10, 100] },
  duotone: { midpoint: [10, 90], softness: [0, 100] },
  watercolor: { bleed: [10, 100], edge: [10, 100], paper: [0, 100], washes: [1, 6], saturation: [0, 100] },
  lowpoly: { cell: [8, 64], jitter: [0, 100], flat: [0, 100], colors: [3, 12] },
  pixel: { size: [2, 16], levels: [2, 16] },
  woodcut: { lineWidth: [1, 8], angle: [0, 180], contrast: [10, 100], roughness: [0, 100] },
  film: { grain: [0, 100], halation: [0, 100], fade: [0, 100], warmth: [0, 100] },
};

const clamp = (value, min, max) => Math.max(min, Math.min(max, value));

export function restoreStyleLabSettings(value = {}) {
  const source = value && typeof value === 'object' ? value : {};
  const style = STYLE_IDS.includes(source.style) ? source.style : 'glitch';
  const defaults = { style, ...STYLE_DEFAULTS[style] };
  const out = { ...defaults };
  const ranges = PARAM_RANGES[style] || {};
  for (const [key, [min, max]] of Object.entries(ranges)) {
    if (Number.isFinite(source[key]) && source[key] >= min && source[key] <= max) out[key] = source[key];
  }
  for (const [key, allowed] of Object.entries(STYLE_ENUMS)) {
    const option = (allowed.find(([k]) => k === key) || [null, null])[1];
    if (!option) continue;
    const match = option.find(([, v]) => v === source[key]);
    if (match) out[key] = match[1];
  }
  if (STYLE_ENUMS.sketch) {
    // 布尔参数单独处理（sketch.invert）
    if (typeof source.invert === 'boolean') out.invert = source.invert;
  }
  if (typeof source.grid === 'boolean') out.grid = source.grid;
  if (typeof source.baseColor === 'boolean') out.baseColor = source.baseColor;
  if (Array.isArray(source.palette) && source.palette.length >= 2) {
    const colors = source.palette
      .filter(c => Array.isArray(c) && c.length === 3 && c.every(v => Number.isFinite(v) && v >= 0 && v <= 255))
      .slice(0, 8);
    if (colors.length >= 2) out.palette = colors;
  }
  if (typeof source.exportScale === 'number' && [1, 2].includes(source.exportScale)) out.exportScale = source.exportScale;
  else out.exportScale = 1;
  return out;
}

// mulberry32：小巧可复现的 PRNG，噪声类样式按 seed 重建噪声场。
export function mulberry32(seed) {
  let a = seed >>> 0;
  return function () {
    a |= 0; a = (a + 0x6d2b79f5) | 0;
    let t = Math.imul(a ^ (a >>> 15), 1 | a);
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

// 哈希驱动的 value noise（双线性插值），fBm 叠加多倍频。
export function makeNoise2D(seed) {
  const rand = mulberry32(seed);
  const perm = new Uint8Array(512);
  for (let i = 0; i < 256; i++) perm[i] = i;
  for (let i = 255; i > 0; i--) {
    const j = Math.floor(rand() * (i + 1));
    [perm[i], perm[j]] = [perm[j], perm[i]];
  }
  for (let i = 0; i < 256; i++) perm[i + 256] = perm[i];
  const fade = t => t * t * (3 - 2 * t);
  const lattice = (x, y) => perm[(perm[x & 255] + y) & 255] / 255;
  return function noise(x, y) {
    const x0 = Math.floor(x), y0 = Math.floor(y);
    const tx = fade(x - x0), ty = fade(y - y0);
    const a = lattice(x0, y0), b = lattice(x0 + 1, y0);
    const c = lattice(x0, y0 + 1), d = lattice(x0 + 1, y0 + 1);
    const top = a + (b - a) * tx;
    const bottom = c + (d - c) * tx;
    return top + (bottom - top) * ty;
  };
}

export function makeFbm(seed, octaves) {
  const noise = makeNoise2D(seed);
  return function fbm(x, y) {
    let sum = 0, amp = 0.5, freq = 1, norm = 0;
    for (let o = 0; o < octaves; o++) {
      sum += noise(x * freq, y * freq) * amp;
      norm += amp;
      amp *= 0.5;
      freq *= 2;
    }
    return sum / norm;
  };
}

// 直方图桶化取色：把图片像素按 5bit/通道 聚桶，取占比最高的 n 桶并以桶内
// 均值作为代表色——从图片自动提取迷彩/大理石调色板。
export function extractPalette(data, n) {
  const buckets = new Map();
  for (let i = 0; i < data.length; i += 4) {
    if (data[i + 3] < 16) continue;
    const r = data[i], g = data[i + 1], b = data[i + 2];
    const key = ((r >> 3) << 10) | ((g >> 3) << 5) | (b >> 3);
    const entry = buckets.get(key) || [0, 0, 0, 0, 0];
    entry[0] += r; entry[1] += g; entry[2] += b; entry[3] += 1;
    buckets.set(key, entry);
  }
  const sorted = [...buckets.values()]
    .sort((a, b) => b[3] - a[3])
    .slice(0, Math.max(2, n))
    .map(([rs, gs, bs, count]) => [Math.round(rs / count), Math.round(gs / count), Math.round(bs / count)]);
  while (sorted.length < n && sorted.length) sorted.push(sorted[sorted.length - 1]);
  return sorted.slice(0, n);
}
