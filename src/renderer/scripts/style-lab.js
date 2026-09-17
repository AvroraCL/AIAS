import { STYLE_IDS, STYLE_LABELS, STYLE_DEFAULTS, restoreStyleLabSettings, makeFbm, makeNoise2D, mulberry32, extractPalette } from './style-lab-state.mjs';
import { renderDuotone, renderWatercolor, renderLowpoly, renderPixelArt, renderWoodcut, renderFilm } from './style-lab-styles2.js';
import { renderRipple, renderGlassR, renderMosaic, renderHeatwave, renderPastel, renderHolo } from './style-lab-styles3.js';
import './style-lab.css';

const EXPORT_MAX = 4096;

// 每套样式的检查器参数表（type: range/select/check/color）。
const STYLE_SCHEMA = {
  glitch: [
    { key: 'split', type: 'range', label: '通道色散', min: 10, max: 80, step: 1 },
    { key: 'blocks', type: 'range', label: '块位移', min: 0, max: 100, step: 1 },
    { key: 'wave', type: 'range', label: '波形抖动', min: 0, max: 100, step: 1 },
    { key: 'scanline', type: 'range', label: '扫描线', min: 0, max: 100, step: 1 },
    { key: 'noise', type: 'range', label: '噪点', min: 0, max: 100, step: 1 },
  ],
  pixelsort: [
    { key: 'threshold', type: 'range', label: '触发亮度', min: 0, max: 100, step: 1 },
    { key: 'span', type: 'range', label: '排序段上限', min: 10, max: 100, step: 1 },
    { key: 'mode', type: 'select', label: '排序对象', options: [['light', '亮部排序'], ['dark', '暗部排序']] },
    { key: 'shuffle', type: 'range', label: '打散程度', min: 0, max: 100, step: 1 },
  ],
  datamosh: [
    { key: 'density', type: 'range', label: '坏块密度', min: 0, max: 100, step: 1 },
    { key: 'size', type: 'range', label: '块尺寸', min: 8, max: 64, step: 1 },
    { key: 'smear', type: 'range', label: '拖影长度', min: 0, max: 100, step: 1 },
    { key: 'tint', type: 'range', label: '色偏', min: 0, max: 100, step: 1 },
  ],
  tear: [
    { key: 'bands', type: 'range', label: '撕裂带数', min: 2, max: 24, step: 1 },
    { key: 'shift', type: 'range', label: '错位幅度', min: 0, max: 100, step: 1 },
    { key: 'split', type: 'range', label: '通道色散', min: 0, max: 100, step: 1 },
    { key: 'noise', type: 'range', label: '撕裂噪点', min: 0, max: 100, step: 1 },
  ],
  decay: [
    { key: 'strength', type: 'range', label: '腐蚀强度', min: 0, max: 100, step: 1 },
    { key: 'size', type: 'range', label: '损坏块尺寸', min: 4, max: 48, step: 1 },
    { key: 'levels', type: 'range', label: '色阶数', min: 2, max: 16, step: 1 },
    { key: 'scan', type: 'range', label: '错位扫描', min: 0, max: 100, step: 1 },
  ],
  crt: [
    { key: 'curve', type: 'range', label: '弧面畸变', min: 0, max: 100, step: 1 },
    { key: 'mask', type: 'range', label: '荫罩栅格', min: 0, max: 100, step: 1 },
    { key: 'glow', type: 'range', label: '辉光', min: 0, max: 100, step: 1 },
    { key: 'vignette', type: 'range', label: '暗角', min: 0, max: 100, step: 1 },
    { key: 'roll', type: 'range', label: '滚动亮带', min: 0, max: 100, step: 1 },
  ],
  echo: [
    { key: 'ghosts', type: 'range', label: '重影数量', min: 1, max: 8, step: 1 },
    { key: 'offset', type: 'range', label: '错位距离', min: 0, max: 100, step: 1 },
    { key: 'fade', type: 'range', label: '残影衰减', min: 10, max: 90, step: 1 },
    { key: 'aberration', type: 'range', label: '色散偏移', min: 0, max: 100, step: 1 },
  ],
  camo: [
    { key: 'pattern', type: 'select', label: '图案', options: [['blotch', '斑块'], ['digital', '数码'], ['leopard', '豹纹'], ['stripe', '条纹'], ['crack', '裂纹']] },
    { key: 'colors', type: 'range', label: '色彩数', min: 2, max: 8, step: 1 },
    { key: 'scale', type: 'range', label: '斑块尺度', min: 8, max: 100, step: 1 },
    { key: 'sharp', type: 'range', label: '边缘锐度', min: 0, max: 100, step: 1 },
    { key: 'rotation', type: 'range', label: '旋转', min: 0, max: 90, step: 1 },
    { key: 'contrast', type: 'range', label: '明暗对比', min: 10, max: 100, step: 1 },
  ],
  oil: [
    { key: 'radius', type: 'range', label: '笔触半径', min: 1, max: 6, step: 1 },
    { key: 'levels', type: 'range', label: '色阶数', min: 2, max: 16, step: 1 },
    { key: 'smooth', type: 'range', label: '原图混合', min: 0, max: 100, step: 1 },
  ],
  halftone: [
    { key: 'mode', type: 'select', label: '模式', options: [['gray', '灰度半调'], ['cmyk', '四色套印']] },
    { key: 'shape', type: 'select', label: '网点形状', options: [['circle', '圆点'], ['square', '方块'], ['diamond', '菱形'], ['line', '平行线'], ['cross', '十字']] },
    { key: 'cell', type: 'range', label: '网格尺寸', min: 3, max: 24, step: 1 },
    { key: 'angle', type: 'range', label: '屏幕角度', min: 0, max: 90, step: 5 },
    { key: 'sharpen', type: 'range', label: '对比', min: 0, max: 100, step: 1 },
  ],
  sketch: [
    { key: 'pencil', type: 'range', label: '炭笔浓度', min: 10, max: 100, step: 1 },
    { key: 'edge', type: 'range', label: '轮廓加深', min: 0, max: 100, step: 1 },
    { key: 'grain', type: 'range', label: '纸面颗粒', min: 0, max: 100, step: 1 },
    { key: 'invert', type: 'check', label: '白炭笔（深底）' },
  ],
  thermal: [
    { key: 'lut', type: 'select', label: '调色板', options: [['iron', '铁红'], ['rainbow', '彩虹'], ['nightvision', '夜视'], ['gold', '鎏金'], ['ice', '冰蓝']] },
    { key: 'mix', type: 'range', label: '假彩强度', min: 0, max: 100, step: 1 },
    { key: 'contrast', type: 'range', label: '对比', min: 0, max: 100, step: 1 },
  ],
  cross: [
    { key: 'levels', type: 'range', label: '色彩级数', min: 3, max: 24, step: 1 },
    { key: 'stitch', type: 'range', label: '绣格尺寸', min: 4, max: 24, step: 1 },
    { key: 'fabric', type: 'range', label: '布纹', min: 0, max: 100, step: 1 },
    { key: 'grid', type: 'check', label: '显示格线' },
  ],
  duotone: [
    { key: 'shadow', type: 'color', label: '暗部色', def: '#1a1a2e' },
    { key: 'highlight', type: 'color', label: '亮部色', def: '#e8c547' },
    { key: 'midpoint', type: 'range', label: '明暗分界', min: 10, max: 90, step: 1 },
    { key: 'softness', type: 'range', label: '过渡柔和', min: 0, max: 100, step: 1 },
  ],
  watercolor: [
    { key: 'bleed', type: 'range', label: '晕染范围', min: 10, max: 100, step: 1 },
    { key: 'edge', type: 'range', label: '轮廓加深', min: 10, max: 100, step: 1 },
    { key: 'paper', type: 'range', label: '纸纹', min: 0, max: 100, step: 1 },
    { key: 'washes', type: 'range', label: '罩染层数', min: 1, max: 6, step: 1 },
    { key: 'saturation', type: 'range', label: '饱和度', min: 0, max: 100, step: 1 },
  ],
  lowpoly: [
    { key: 'cell', type: 'range', label: '多边形尺寸', min: 8, max: 64, step: 1 },
    { key: 'jitter', type: 'range', label: '顶点抖动', min: 0, max: 100, step: 1 },
    { key: 'flat', type: 'range', label: '平面化', min: 0, max: 100, step: 1 },
    { key: 'palette', type: 'select', label: '调色板', options: [['auto', '取色于原图'], ['warm', '暖调'], ['cool', '冷调']] },
    { key: 'colors', type: 'range', label: '色彩数', min: 3, max: 12, step: 1 },
  ],
  pixel: [
    { key: 'size', type: 'range', label: '像素大小', min: 2, max: 16, step: 1 },
    { key: 'levels', type: 'range', label: '色彩级数', min: 2, max: 16, step: 1 },
  ],
  woodcut: [
    { key: 'lineWidth', type: 'range', label: '线宽', min: 1, max: 8, step: 1 },
    { key: 'angle', type: 'range', label: '排线角度', min: 0, max: 180, step: 15 },
    { key: 'contrast', type: 'range', label: '对比', min: 10, max: 100, step: 1 },
    { key: 'roughness', type: 'range', label: '粗糙度', min: 0, max: 100, step: 1 },
  ],
  film: [
    { key: 'grain', type: 'range', label: '颗粒', min: 0, max: 100, step: 1 },
    { key: 'halation', type: 'range', label: '光晕', min: 0, max: 100, step: 1 },
    { key: 'fade', type: 'range', label: '褪色', min: 0, max: 100, step: 1 },
    { key: 'warmth', type: 'range', label: '暖调', min: 0, max: 100, step: 1 },
  ],
  ripple: [
    { key: 'amplitude', type: 'range', label: '波幅', min: 2, max: 60, step: 1 },
    { key: 'wavelength', type: 'range', label: '波长', min: 8, max: 100, step: 1 },
    { key: 'speed', type: 'range', label: '速度', min: 0, max: 100, step: 1 },
    { key: 'mix', type: 'range', label: '混合', min: 0, max: 100, step: 1 },
  ],
  glass: [
    { key: 'refraction', type: 'range', label: '折射强度', min: 5, max: 100, step: 1 },
    { key: 'blur', type: 'range', label: '模糊', min: 0, max: 40, step: 1 },
    { key: 'tint', type: 'range', label: '色调', min: 0, max: 60, step: 1 },
  ],
  mosaic: [
    { key: 'tile', type: 'range', label: '砖块尺寸', min: 4, max: 40, step: 1 },
    { key: 'gap', type: 'range', label: '缝隙', min: 0, max: 8, step: 1 },
    { key: 'jitter', type: 'range', label: '抖动', min: 0, max: 100, step: 1 },
    { key: 'grout', type: 'range', label: '填缝暗度', min: 0, max: 60, step: 1 },
  ],
  heatwave: [
    { key: 'strength', type: 'range', label: '扭曲强度', min: 5, max: 100, step: 1 },
    { key: 'speed', type: 'range', label: '速度', min: 0, max: 100, step: 1 },
    { key: 'freq', type: 'range', label: '频率', min: 5, max: 50, step: 1 },
  ],
  pastel: [
    { key: 'softness', type: 'range', label: '柔化', min: 20, max: 100, step: 1 },
    { key: 'grain', type: 'range', label: '蜡笔颗粒', min: 0, max: 100, step: 1 },
    { key: 'bloom', type: 'range', label: '晕开', min: 0, max: 100, step: 1 },
    { key: 'paper', type: 'range', label: '纸纹', min: 0, max: 100, step: 1 },
  ],
  holo: [
    { key: 'intensity', type: 'range', label: '强度', min: 10, max: 100, step: 1 },
    { key: 'spectrum', type: 'range', label: '光谱宽度', min: 10, max: 100, step: 1 },
    { key: 'scanline', type: 'range', label: '扫描线', min: 0, max: 80, step: 1 },
    { key: 'shimmer', type: 'range', label: '闪光', min: 0, max: 100, step: 1 },
  ],
};

const BUILTIN_PRESETS = {
  glitch: [
    { name: 'VHS 录像带', settings: { split: 26, blocks: 30, wave: 55, scanline: 60, noise: 40 } },
    { name: '数据崩坏', settings: { split: 60, blocks: 85, wave: 15, scanline: 20, noise: 70 } },
    { name: '轻微色差', settings: { split: 12, blocks: 5, wave: 10, scanline: 10, noise: 8 } },
  ],
  pixelsort: [
    { name: '数据流', settings: { threshold: 22, span: 100, mode: 'light', shuffle: 5 } },
    { name: '冰晶碎片', settings: { threshold: 55, span: 35, mode: 'light', shuffle: 30 } },
    { name: '暗部侵蚀', settings: { threshold: 30, span: 80, mode: 'dark', shuffle: 10 } },
  ],
  datamosh: [
    { name: '编码损坏', settings: { density: 70, size: 16, smear: 85, tint: 30 } },
    { name: '轻微融化', settings: { density: 20, size: 40, smear: 30, tint: 10 } },
    { name: '果冻流动', settings: { density: 55, size: 28, smear: 70, tint: 45 } },
  ],
  tear: [
    { name: 'VHS 跳带', settings: { bands: 14, shift: 60, split: 45, noise: 55 } },
    { name: '轻微失锁', settings: { bands: 5, shift: 20, split: 15, noise: 15 } },
    { name: '信号风暴', settings: { bands: 24, shift: 90, split: 60, noise: 80 } },
  ],
  decay: [
    { name: '低色深转码', settings: { strength: 45, size: 10, levels: 5, scan: 20 } },
    { name: '文件损坏', settings: { strength: 85, size: 20, levels: 3, scan: 70 } },
    { name: '轻微量化', settings: { strength: 20, size: 8, levels: 10, scan: 10 } },
  ],
  crt: [
    { name: '老式电视机', settings: { curve: 55, mask: 65, glow: 45, vignette: 55, roll: 35 } },
    { name: '街机屏', settings: { curve: 20, mask: 80, glow: 30, vignette: 30, roll: 15 } },
    { name: '柔和监视器', settings: { curve: 30, mask: 35, glow: 55, vignette: 40, roll: 25 } },
  ],
  echo: [
    { name: '天线不良', settings: { ghosts: 4, offset: 55, fade: 60, aberration: 40 } },
    { name: '幽微残像', settings: { ghosts: 2, offset: 20, fade: 35, aberration: 15 } },
    { name: '迷幻轨迹', settings: { ghosts: 7, offset: 75, fade: 75, aberration: 60 } },
  ],
  camo: [
    { name: '林地迷彩', settings: { pattern: 'blotch', colors: 4, scale: 55, sharp: 35, contrast: 65 } },
    { name: '数码迷彩', settings: { pattern: 'digital', colors: 4, scale: 30, sharp: 90, contrast: 70 } },
    { name: '豹纹点', settings: { pattern: 'leopard', colors: 3, scale: 22, sharp: 70, contrast: 80 } },
    { name: '沙漠裂纹', settings: { pattern: 'crack', colors: 5, scale: 60, sharp: 45, contrast: 55 } },
  ],
  oil: [
    { name: '印象派厚涂', settings: { radius: 5, levels: 8, smooth: 15 } },
    { name: '细腻笔触', settings: { radius: 3, levels: 12, smooth: 35 } },
  ],
  halftone: [
    { name: '报纸灰调', settings: { mode: 'gray', shape: 'circle', cell: 7, angle: 15, sharpen: 55 } },
    { name: '四色套印', settings: { mode: 'cmyk', cell: 9, angle: 15, sharpen: 30 } },
  ],
  sketch: [
    { name: '炭笔素描', settings: { pencil: 80, edge: 45, grain: 45, invert: false } },
    { name: '粉笔黑板', settings: { pencil: 55, edge: 30, grain: 60, invert: true } },
  ],
  thermal: [
    { name: '铁红热感', settings: { lut: 'iron', mix: 90, contrast: 45 } },
    { name: '夜视仪', settings: { lut: 'nightvision', mix: 100, contrast: 55 } },
  ],
  cross: [
    { name: '复古十字绣', settings: { levels: 8, stitch: 12, fabric: 45, grid: true } },
    { name: '精细绣格', settings: { levels: 16, stitch: 6, fabric: 25, grid: false } },
  ],
  duotone: [
    { name: '午夜金', settings: { shadow: '#1a1a2e', highlight: '#e8c547', midpoint: 50, softness: 30 } },
    { name: '墨绿红', settings: { shadow: '#0d2818', highlight: '#ff6b6b', midpoint: 45, softness: 40 } },
  ],
  watercolor: [
    { name: '淡彩速写', settings: { bleed: 40, edge: 35, paper: 55, washes: 2, saturation: 60 } },
    { name: '浓彩水墨', settings: { bleed: 80, edge: 75, paper: 30, washes: 5, saturation: 85 } },
  ],
  lowpoly: [
    { name: '暖调低多边形', settings: { cell: 20, jitter: 45, flat: 30, palette: 'warm', colors: 6 } },
    { name: '冷调低多边形', settings: { cell: 28, jitter: 30, flat: 45, palette: 'cool', colors: 5 } },
  ],
  pixel: [
    { name: 'GameBoy', settings: { size: 8, levels: 4, dither: 'ordered' } },
    { name: 'CGA 16色', settings: { size: 5, levels: 10, dither: 'diffusion' } },
  ],
  woodcut: [
    { name: '木刻版画', settings: { lineWidth: 3, angle: 0, contrast: 80, roughness: 30 } },
    { name: '细纹铜版', settings: { lineWidth: 2, angle: 90, contrast: 60, roughness: 50 } },
  ],
  film: [
    { name: 'Portra 400', settings: { grain: 35, halation: 40, fade: 30, warmth: 55 } },
    { name: 'Cinestill 800T', settings: { grain: 50, halation: 70, fade: 15, warmth: 10 } },
  ],
};

export function createStyleLab({ onImage = null, root, inspector, runArea, desktop, open, saveDialog, convertFileSrc, invoke, settings, save, setBusy, busy, changed, notify }) {
  let config = restoreStyleLabSettings(settings), source = null, sourceName = "", result = null, disposed = false;
  let active = false, exporting = false, importing = false, computing = false, revision = 0, importRevision = 0;
  let view = 'result', zoom = 1, saveTimer, controlsBuiltFor = null;
  let styleLabUI = null; void styleLabUI;
  const ZOOM_MIN = 0.25, ZOOM_MAX = 3, WHEEL_STEP = 0.0015;

  root.innerHTML = `<div class="style-lab-toolbar"><button id="stylize-import" class="secondary-action" type="button">选择图片</button><button id="stylize-clear" class="secondary-action" type="button" disabled>清空</button><span id="stylize-name">PNG · JPG · WebP</span><input id="stylize-file" type="file" accept="image/png,image/jpeg,image/webp" hidden></div>
    <div class="style-lab-toolbar"><button id="stylize-fit" class="secondary-action" type="button">适应窗口</button><label>缩放 <input id="stylize-zoom" aria-label="预览缩放" type="range" min="0.25" max="3" step="0.05" value="1"></label><span id="stylize-style-name" class="style-lab-status"></span></div>
    <div class="style-lab-stage" id="stylize-stage"><div id="stylize-empty"><strong>把图片变成新的风格</strong><p>选择或拖入一张图片，右侧调参数，实时出效果。</p><button id="stylize-empty-import" class="secondary-action" type="button">选择图片</button></div><canvas id="stylize-canvas" hidden aria-label="风格化预览"></canvas></div>
    <div class="style-lab-bottom"><span id="stylize-size"></span><span id="stylize-progress" hidden>处理中…</span></div><p id="stylize-status" role="status"></p>`;

  const controls = document.createElement('div');
  controls.className = 'style-lab-controls';
  controls.hidden = true;
  inspector.append(controls);

  const run = document.createElement('button');
  run.id = 'stylize-run'; run.type = 'button'; run.className = 'run-button hidden'; run.innerHTML = '<span>导出 PNG</span>';
  runArea.append(run);

  const elements = new Map([...root.querySelectorAll('[id]'), ...controls.querySelectorAll('[id]'), run].map(el => [el.id, el]));
  const $ = key => elements.get(`stylize-${key}`);
  const status = text => { $('status').textContent = text; };

  function currentSchema() { return STYLE_SCHEMA[config.style] || []; }
  function styleLabel() { return STYLE_LABELS[config.style] || config.style; }

  // ---- 参数恢复与持久化 -------------------------------------------------
  function restore() {
    config = restoreStyleLabSettings({ style: config.style, ...settings, ...STYLE_DEFAULTS[config.style], style: config.style });
  }
  function persist() {
    clearTimeout(saveTimer);
    saveTimer = setTimeout(() => {
      save({ style: config.style, ...config }).catch(e => { if (!disposed) status(`设置保存失败：${e.message || e}`); });
    }, 250);
  }

  // ---- 检查器渲染（按当前样式 schema 生成） -----------------------------
  function renderControls() {
    controls.replaceChildren();
    const schema = currentSchema();
    const presets = BUILTIN_PRESETS[config.style] || [];
    const presetSection = document.createElement('section');
    presetSection.className = 'inspector-group';
    presetSection.innerHTML = `<button class="group-toggle" type="button" aria-expanded="true"><span>预设 · ${styleLabel()}</span><i data-lucide="chevron-down"></i></button><div class="group-content"><div class="style-lab-presets"></div><div class="style-lab-preset-save"><input maxlength="20" placeholder="预设名称" aria-label="预设名称" data-stylize-preset-name><button class="secondary-action" type="button" data-stylize-preset-save>保存当前</button></div></div>`;
    const presetWrap = presetSection.querySelector('.style-lab-presets');
    for (const preset of presets) {
      const chip = document.createElement('button');
      chip.type = 'button'; chip.className = 'style-lab-preset-chip'; chip.textContent = preset.name;
      chip.onclick = () => { config = restoreStyleLabSettings({ ...preset.settings, style: config.style }); syncControls(); schedule(); };
      presetWrap.append(chip);
    }
    // 用户自定义预设
    const saved = config.customPresets || [];
    for (const preset of saved) {
      const chip = document.createElement('button');
      chip.type = 'button'; chip.className = 'style-lab-preset-chip'; chip.textContent = preset.name;
      chip.onclick = () => { config = restoreStyleLabSettings({ ...preset.settings, style: config.style }); syncControls(); schedule(); };
      const cross = document.createElement('span'); cross.className = 'style-lab-preset-remove'; cross.textContent = '×';
      cross.onclick = event => {
        event.stopPropagation();
        config.customPresets = (config.customPresets || []).filter(p => p.name !== preset.name);
        persist(); renderControls();
      };
      chip.append(cross);
      presetWrap.append(chip);
    }
    const presetNameInput = presetSection.querySelector('[data-stylize-preset-name]');
    const presetSaveBtn = presetSection.querySelector('[data-stylize-preset-save]');
    if (presetNameInput && presetSaveBtn) {
      presetSaveBtn.onclick = () => {
        const name = (presetNameInput.value || '').trim();
        if (!name) { presetNameInput.focus(); return; }
        config.customPresets = [{ name: name.slice(0, 20), settings: { ...config } }, ...(config.customPresets || [])].slice(0, 10);
        presetNameInput.value = '';
        persist(); renderControls();
        status(`预设「${name}」已保存。`);
      };
    }
    controls.append(presetSection);

    const paramSection = document.createElement('section');
    paramSection.className = 'inspector-group';
    paramSection.innerHTML = `<button class="group-toggle" type="button" aria-expanded="true"><span>样式参数</span><i data-lucide="chevron-down"></i></button><div class="group-content style-lab-grid"></div>`;
    const grid = paramSection.querySelector('.style-lab-grid');
    for (const p of schema) {
      if (p.type === 'range') {
        // Figma/Blender 式数值行：右侧数值可直接键入，左侧标签可拖动微调，
        // 双击行恢复该参数默认，滚轮在滑杆上步进（Shift 粗调 / Alt 细调）。
        const row = document.createElement('div');
        row.className = 'style-lab-param';
        row.dataset.paramRow = p.key;
        const head = document.createElement('div');
        head.className = 'style-lab-param-head';
        const name = document.createElement('span');
        name.className = 'style-lab-param-name';
        name.textContent = p.label;
        name.title = '左右拖动微调（Shift 更精细）· 双击恢复默认';
        const val = document.createElement('input');
        val.type = 'text'; val.inputMode = 'decimal'; val.spellcheck = false;
        val.className = 'style-lab-param-value';
        val.dataset.paramValue = p.key;
        val.setAttribute('aria-label', `${p.label} 数值`);
        head.append(name, val);
        const slider = document.createElement('input');
        slider.type = 'range';
        slider.min = String(p.min); slider.max = String(p.max); slider.step = String(p.step);
        slider.dataset.param = p.key;
        slider.setAttribute('aria-label', p.label);
        row.append(head, slider);
        grid.append(row);
      } else if (p.type === 'select') {
        const label = document.createElement('label');
        label.textContent = p.label;
        const select = document.createElement('select');
        select.dataset.param = p.key;
        for (const [v, text] of p.options) select.append(new Option(text, v));
        label.append(select);
        grid.append(label);
      } else if (p.type === 'color') {
        const label = document.createElement('label');
        label.textContent = p.label;
        const input = document.createElement('input');
        input.type = 'color'; input.dataset.param = p.key; input.value = p.def || '#000000';
        label.append(input);
        grid.append(label);
      } else if (p.type === 'check') {
        const label = document.createElement('label');
        label.className = 'ascii-check';
        const box = document.createElement('input');
        box.type = 'checkbox'; box.dataset.param = p.key;
        label.append(box, document.createTextNode(p.label));
        grid.append(label);
      }
    }
    controls.append(paramSection);
    const actionsRow = document.createElement('div');
    actionsRow.className = 'style-lab-param-actions';
    const randomButton = document.createElement('button');
    randomButton.type = 'button';
    randomButton.className = 'secondary-action';
    randomButton.textContent = '随机探索';
    randomButton.title = '在全部参数范围内随机取值（Photomosh 式快速探索），效果可复现于当前种子';
    randomButton.onclick = () => {
      const rand = mulberry32((Math.random() * 0x7fffffff) | 0);
      for (const prm of currentSchema()) {
        if (prm.type === 'range') {
          const raw = prm.min + rand() * (prm.max - prm.min);
          config[prm.key] = clampParam(prm, String(raw));
        } else if (prm.type === 'select') {
          config[prm.key] = prm.options[Math.floor(rand() * prm.options.length)][0];
        }
      }
      syncControls();
      schedule();
    };
    actionsRow.append(randomButton);
    if (resetButton) actionsRow.append(resetButton);
    controls.append(actionsRow);

    // 导出设置
    const exportSection = document.createElement('section');
    exportSection.className = 'inspector-group';
    exportSection.innerHTML = `<button class="group-toggle" type="button" aria-expanded="true"><span>导出设置</span><i data-lucide="chevron-down"></i></button><div class="group-content"><label>导出倍率<select data-stylize-exportScale><option value="1">1×</option><option value="2">2×</option><option value="4">4×</option></select></label><small>倍率越高导出越清晰（不影响预览）。</small></div>`;
    controls.append(exportSection);
    const exportScaleEl = exportSection.querySelector('[data-stylize-exportScale]');
    if (exportScaleEl) exportScaleEl.value = String(config.exportScale || 1);
    if (exportScaleEl) exportScaleEl.onchange = () => { config.exportScale = Number(exportScaleEl.value); persist(); };
    syncControls();
    controlsBuiltFor = config.style;
  }

  function syncControls() {
    for (const el of controls.querySelectorAll('[data-param]')) {
      const key = el.dataset.param;
      if (el.type === 'checkbox') el.checked = Boolean(config[key]);
      else el.value = String(config[key]);
    }
    for (const el of controls.querySelectorAll('[data-param-value]')) {
      const prm = paramMeta(el.dataset.paramValue);
      el.value = prm ? formatParamValue(config[el.dataset.paramValue], prm) : '';
    }
    controls.querySelectorAll('select').forEach(select => { if (select.dataset.selectSkip !== 'true') select.dispatchEvent(new Event('styled')); });
  }

  function paramMeta(key) {
    return currentSchema().find(p => p.key === key) || null;
  }

  function formatParamValue(value, prm) {
    if (typeof value !== 'number' || !Number.isFinite(value)) return String(value ?? '');
    if ((prm.step || 1) < 1) return String(Math.round(value * 100) / 100);
    return String(Math.round(value));
  }

  function clampParam(prm, raw) {
    let v = Number.parseFloat(raw);
    if (!Number.isFinite(v)) return null;
    const step = prm.step || 1;
    v = Math.round(v / step) * step;
    return Math.min(prm.max, Math.max(prm.min, v));
  }

  function commitParamValue(key, text) {
    const prm = paramMeta(key);
    if (!prm || prm.type !== 'range') return false;
    const v = clampParam(prm, text);
    if (v === null) { syncControls(); return false; }
    config[key] = v;
    syncControls();
    schedule();
    return true;
  }

  // ---- 预览调度 ---------------------------------------------------------
  function schedule() {
    revision++;
    const current = revision;
    clearTimeout(saveTimer);
    saveTimer = setTimeout(() => {
      persist();
      if (!source || !active || disposed) return;
      renderAsync(current);
    }, 180);
  }

  function renderAsync(current) {
    computing = true;
    const progressEl = $('progress');
    progressEl.hidden = false;
    status('正在生成效果…');
    // 两帧后启动，让「处理中」状态先上屏
    requestAnimationFrame(() => requestAnimationFrame(() => {
      // 被新参数抢占时同样要复位状态：否则「处理中…」常驻，blocker()
      // 一直返回“正在生成效果”锁住运行按钮。
      if (current !== revision || disposed) { computing = false; progressEl.hidden = true; return; }
      try {
        const w = source.width, h = source.height;
        const src = source.getContext('2d').getImageData(0, 0, w, h);
        const pre = prepareStyle(config.style, src.data, w, h, config);
        const out = new ImageData(new Uint8ClampedArray(src.data), w, h);
        const band = Math.max(1, Math.ceil(h / 14));
        let y0 = 0;
        const step = () => {
          if (current !== revision || disposed) { computing = false; progressEl.hidden = true; return; }
          const y1 = Math.min(h, y0 + band);
          renderRows(config.style, out.data, src.data, w, h, y0, y1, config, pre);
          y0 = y1;
          progressEl.textContent = `处理中 ${Math.round(y1 / h * 100)}%`;
          if (y0 < h) requestAnimationFrame(step);
          else {
            computing = false;
            progressEl.hidden = true;
            result = new ImageData(out.data, w, h);
            draw();
            status('预览已更新');
          }
        };
        step();
      } catch (e) {
        computing = false;
        progressEl.hidden = true;
        refresh();
        status(`生成失败：${e.message || e}`);
      }
    }));
  }

  // ---- 预览与导出 -------------------------------------------------------
  function fit() {
    const canvas = $('canvas'); if (canvas.hidden) return;
    const stage = $('stage');
    const scale = Math.min((stage.clientWidth - 32) / canvas.width, (stage.clientHeight - 32) / canvas.height, 1) * zoom;
    canvas.style.width = `${Math.max(1, canvas.width * scale)}px`;
    canvas.style.height = `${Math.max(1, canvas.height * scale)}px`;
  }
  function draw() {
    root.querySelectorAll('[data-stylize-view]').forEach(el => {
      const selected = el.dataset.stylizeView === view;
      el.classList.toggle('selected', selected);
      el.setAttribute('aria-pressed', String(selected));
    });
    const styleName = $('style-name');
    if (styleName) styleName.textContent = styleLabel();
    const canvas = $('canvas');
    canvas.hidden = view === 'source' ? !source : !result;
    $('empty').hidden = Boolean(source);
    if (canvas.hidden) return;
    if (view === 'source') { canvas.width = source.width; canvas.height = source.height; canvas.getContext('2d').drawImage(source, 0, 0); }
    else { canvas.width = result.width; canvas.height = result.height; canvas.getContext('2d').putImageData(result, 0, 0); }
    fit();
  }

  run.onclick = async () => {
    if (!result || exporting) return;
    const name = `${sourceName.replace(/\.[^.]+$/, '')}_${config.style}.png`;
    exporting = true; setBusy(run, true);
    try {
      const scale = config.exportScale || 1;
      const canvas = document.createElement('canvas');
      canvas.width = result.width * scale; canvas.height = result.height * scale;
      const ctx = canvas.getContext('2d');
      const tempCanvas = document.createElement('canvas');
      tempCanvas.width = result.width; tempCanvas.height = result.height;
      tempCanvas.getContext('2d').putImageData(result, 0, 0);
      ctx.drawImage(tempCanvas, 0, 0, canvas.width, canvas.height);
      const blob = await new Promise((resolve, reject) => canvas.toBlob(v => v ? resolve(v) : reject(Error('PNG 编码失败。')), 'image/png'));
      let path = name;
      if (desktop) {
        const picked = await saveDialog({ defaultPath: name, filters: [{ name: 'PNG', extensions: ['png'] }] });
        if (!picked) { status('已取消导出。'); return; }
        path = picked;
        const base64 = await new Promise((resolve, reject) => {
          const reader = new FileReader();
          reader.onload = () => resolve(reader.result.split(',')[1]);
          reader.onerror = () => reject(Error('PNG 读取失败。'));
          reader.readAsDataURL(blob);
        });
        await invoke('ascii_export', { path, format: 'png', content: base64 });
      } else {
        const url = URL.createObjectURL(blob), a = document.createElement('a');
        a.href = url; a.download = name; a.hidden = true;
        document.body.append(a); a.click(); a.remove();
        setTimeout(() => URL.revokeObjectURL(url), 1000);
      }
      status(`已导出 PNG`); notify(`已导出 ${path.split(/[\\/]/).pop()}`, 'success');
    } catch (e) {
      status(`导出失败：${e.message || e}`); notify(`导出失败：${e.message || e}`, 'error');
    } finally { exporting = false; setBusy(run, false); }
  };

  // ---- 图片导入 ---------------------------------------------------------
  async function loadFile(file) {
    if (exporting || importing) return;
    if (!/\.(png|jpe?g|webp)$/i.test(typeof file === 'string' ? file : file.name)) { status('请选择 PNG、JPG 或 WebP 图片。'); return; }
    const ticket = ++importRevision; importing = true; result = null; refresh(); status('正在读取图片…');
    try {
      const blob = typeof file === 'string' ? await (await fetch(convertFileSrc(file))).blob() : file;
      if (blob.size > 50 * 1024 * 1024) throw Error('图片文件超过 50 MB，请先缩小图片。');
      const bitmap = await createImageBitmap(blob);
      if (ticket !== importRevision || disposed || !active) { bitmap.close(); return; }
      const scale = Math.min(1, EXPORT_MAX / Math.max(bitmap.width, bitmap.height));
      const decoded = document.createElement('canvas');
      decoded.width = Math.max(1, Math.round(bitmap.width * scale));
      decoded.height = Math.max(1, Math.round(bitmap.height * scale));
      decoded.getContext('2d').drawImage(bitmap, 0, 0, decoded.width, decoded.height);
      bitmap.close();
      source = decoded;
      sourceName = typeof file === 'string' ? file.split(/[\\/]/).pop() : file.name;
      $('name').textContent = sourceName; $('import').textContent = '替换图片';
      zoom = 1; $('zoom').value = 1;
      view = 'result';
      // 发布到 ASCII 系：风格实验室与 ASCII 系共用同一张源图
      onImage?.({ name: sourceName, canvas: source });
      schedule();
    } catch (e) { status(`读取失败：${e.message || e}`); }
    finally { importing = false; refresh(); draw(); }
  }
  async function pick() {
    if (importing || exporting) return;
    if (!desktop) { $('file').click(); return; }
    try { const file = await open({ multiple: false, filters: [{ name: '图片', extensions: ['png', 'jpg', 'jpeg', 'webp'] }] }); if (typeof file === 'string') await loadFile(file); }
    catch (e) { status(`选择失败：${e.message || e}`); }
  }
  $('import').onclick = pick; $('empty-import').onclick = pick;
  $('file').onchange = () => { const file = $('file').files[0]; $('file').value = ''; if (file) loadFile(file); };
  root.ondragover = e => e.preventDefault();
  root.ondrop = e => { e.preventDefault(); if (!desktop && e.dataTransfer.files.length === 1) loadFile(e.dataTransfer.files[0]); };
  $('clear').onclick = () => { if (exporting || importing) return; source = result = null; sourceName = ''; $('name').textContent = 'PNG · JPG · WebP'; status(''); draw(); refresh(); };
  // 控件由 renderControls 按样式动态重建：初始化时容器为空，逐元素绑定
  // 会永远落空。用事件委托，滑杆/颜色走 input（拖动实时预览），下拉/复选
  // 框走 change（提交即应用）。
  const applyParam = el => {
    const key = el.dataset.param;
    config[key] = el.type === 'checkbox' ? el.checked
      : el.type === 'range' ? Number(el.value)
      : el.value;
    schedule();
  };
  controls.addEventListener('input', event => {
    const el = event.target.closest('[data-param]');
    if (el && el.type !== 'checkbox' && el.tagName !== 'SELECT') applyParam(el);
  });
  controls.addEventListener('change', event => {
    const el = event.target.closest('[data-param]');
    if (el && (el.type === 'checkbox' || el.tagName === 'SELECT')) applyParam(el);
  });
  // 数值框：Enter/失焦提交（钳制+步进对齐），Esc 还原；聚焦全选便于整段覆盖
  controls.addEventListener('focusin', event => {
    const el = event.target.closest('[data-param-value]');
    if (el) el.select();
  });
  controls.addEventListener('keydown', event => {
    const el = event.target.closest('[data-param-value]');
    if (!el) return;
    if (event.key === 'Enter') { event.preventDefault(); el.blur(); }
    else if (event.key === 'Escape') { event.preventDefault(); syncControls(); el.blur(); }
  });
  controls.addEventListener('focusout', event => {
    const el = event.target.closest('[data-param-value]');
    if (el) commitParamValue(el.dataset.paramValue, el.value);
  });
  // 标签拖动微调（scrub）：按住左右拖改变数值，Shift 减速 10 倍
  controls.addEventListener('pointerdown', event => {
    const name = event.target.closest('.style-lab-param-name');
    if (!name || event.button !== 0) return;
    const row = name.closest('.style-lab-param');
    const key = row && row.dataset.paramRow;
    const prm = key && paramMeta(key);
    if (!prm || prm.type !== 'range') return;
    event.preventDefault();
    const startX = event.clientX;
    const startV = Number(config[key]) || 0;
    const range = prm.max - prm.min;
    const onMove = ev => {
      const speed = ev.shiftKey ? range / 2000 : range / 200;
      let v = startV + (ev.clientX - startX) * speed;
      v = Math.round(v / (prm.step || 1)) * (prm.step || 1);
      config[key] = Math.min(prm.max, Math.max(prm.min, v));
      syncControls();
      schedule();
    };
    const onUp = () => {
      window.removeEventListener('pointermove', onMove);
      window.removeEventListener('pointerup', onUp);
    };
    window.addEventListener('pointermove', onMove);
    window.addEventListener('pointerup', onUp);
  });
  // 双击参数行：仅恢复该参数默认值
  controls.addEventListener('dblclick', event => {
    const row = event.target.closest('.style-lab-param');
    if (!row) return;
    const key = row.dataset.paramRow;
    const def = (STYLE_DEFAULTS[config.style] || {})[key];
    if (def === undefined) return;
    config[key] = def;
    syncControls();
    schedule();
  });
  // 滑杆上滚轮步进：Shift ×10，Alt ×0.2（需 passive:false 才能 preventDefault）
  controls.addEventListener('wheel', event => {
    const slider = event.target.closest('[data-param]');
    if (!slider || slider.type !== 'range') return;
    const prm = paramMeta(slider.dataset.param);
    if (!prm || prm.type !== 'range') return;
    event.preventDefault();
    const step = (prm.step || 1) * (event.shiftKey ? 10 : event.altKey ? 0.2 : 1);
    const dir = event.deltaY < 0 ? 1 : -1;
    let v = (Number(config[slider.dataset.param]) || 0) + dir * step;
    v = Math.round(v / (prm.step || 1)) * (prm.step || 1);
    config[slider.dataset.param] = Math.min(prm.max, Math.max(prm.min, v));
    syncControls();
    schedule();
  }, { passive: false });
  const resetButton = document.createElement('button');
  resetButton.className = 'secondary-action'; resetButton.type = 'button'; resetButton.textContent = '恢复默认';
  resetButton.onclick = () => { config = restoreStyleLabSettings({ style: config.style, ...STYLE_DEFAULTS[config.style] }); syncControls(); schedule(); };
  controls.append(resetButton);
  const zoomInput = $('zoom');
  zoomInput.oninput = () => { zoom = Number(zoomInput.value); fit(); };
  $('fit').onclick = () => { zoom = 1; zoomInput.value = 1; fit(); };
  // 滚轮缩放（与 ascii.js 同机制）：以指针下的图像点为锚点，缩放后调整
  // 滚动位置让该点在屏幕上不动。
  $('stage').addEventListener('wheel', event => {
    const canvas = $('canvas');
    if (canvas.hidden) return;
    event.preventDefault();
    const before = canvas.getBoundingClientRect();
    const fx = before.width ? (event.clientX - before.left) / before.width : 0.5;
    const fy = before.height ? (event.clientY - before.top) / before.height : 0.5;
    const next = Math.max(0.25, Math.min(3, zoom * Math.exp(-event.deltaY * 0.0015)));
    if (next === zoom) return;
    zoom = next;
    $('zoom').value = String(zoom);
    fit();
    const after = canvas.getBoundingClientRect();
    $('stage').scrollLeft += after.left + fx * after.width - event.clientX;
    $('stage').scrollTop += after.top + fy * after.height - event.clientY;
  }, { passive: false });
  // 拖拽平移：左键在预览上拖动即滚动容器；pointer capture 保证移出元素后
  // 仍跟手。空状态（画布隐藏）时不接管，保证「选择图片」按钮可点。
  let drag = null;
  $('stage').addEventListener('pointerdown', event => {
    const canvas = $('canvas');
    if (canvas.hidden || event.button !== 0) return;
    drag = { id: event.pointerId, x: event.clientX, y: event.clientY, left: $('stage').scrollLeft, top: $('stage').scrollTop };
    $('stage').classList.add('style-lab-dragging');
    try { $('stage').setPointerCapture(event.pointerId); } catch { /* noop */ }
    event.preventDefault();
  });
  $('stage').addEventListener('pointermove', event => {
    if (!drag || event.pointerId !== drag.id) return;
    $('stage').scrollLeft = drag.left - (event.clientX - drag.x);
    $('stage').scrollTop = drag.top - (event.clientY - drag.y);
  });
  const endDrag = () => {
    if (!drag) return;
    drag = null;
    $('stage').classList.remove('style-lab-dragging');
  };
  $('stage').addEventListener('pointerup', endDrag);
  $('stage').addEventListener('pointercancel', endDrag);
  // Alt+Tab 等让窗口失去指针所有权时 pointerup 可能收不到：不收尾会导致
  // 未按键状态下鼠标划过预览仍持续平移（拖拽"粘住"）。
  $('stage').addEventListener('lostpointercapture', endDrag);
  window.addEventListener('blur', endDrag);
  root.querySelectorAll('[data-stylize-view]').forEach(el => el.onclick = () => { view = el.dataset.stylizeView; draw(); });
  const resize = new ResizeObserver(fit);
  function refresh() {
    const locked = exporting || importing;
    for (const el of controls.querySelectorAll('input,select,button')) el.disabled = locked;
    $('import').disabled = locked; $('empty-import').disabled = locked;
    $('clear').disabled = !source || locked;
    run.disabled = Boolean(blocker()) || busy();
    changed();
  }
  function blocker() {
    return exporting ? '正在导出…' : importing ? '正在读取图片…' : computing ? '正在生成效果…' : !result ? '请导入图片并生成有效预览。' : null;
  }
  const resizeObserver = new ResizeObserver(fit);
  resizeObserver.observe($('stage'));
  syncControls();
  return {
    blocker,
    // 采用 ASCII 系加载的图片：不回发 onImage，避免互相触发
    setSharedImage(image) {
      if (!image || !image.canvas || disposed || importing || exporting) return;
      source = image.canvas;
      sourceName = image.name;
      result = null;
      $('name').textContent = sourceName; $('import').textContent = '替换图片';
      zoom = 1; $('zoom').value = 1;
      view = 'result';
      schedule();
      refresh();
    },
    activate(mode) {
      active = STYLE_IDS.some(id => mode === `stylize-${id}`);
      controls.hidden = !active;
      if (!active) { ++revision; return; }
      const style = mode.replace('stylize-', '');
      // 首次激活（控件从未渲染）或切换样式时都重建：只比较 config.style
      // 会让首次打开停留在空白面板上。
      if (controlsBuiltFor !== style) {
        config.style = style;
        config = restoreStyleLabSettings({ style, ...STYLE_DEFAULTS[style] });
        renderControls();
      }
      refresh(); schedule();
    },
    addFiles(paths) { if (paths.length === 1) loadFile(paths[0]); else status('请一次拖入一张图片。'); },
    dispose() { disposed = true; ++revision; clearTimeout(saveTimer); resizeObserver.disconnect(); window.removeEventListener('blur', endDrag); controls.remove(); },
  };
  // 上面 return 之后不可达； disposed/refresh 引用在前文闭包内
}
// ===========================================================================
// 风格处理引擎：每套样式 = prepare（全图预计算）+ renderRows（按行写出）。
// 均为纯函数：输入 ImageData 数据与参数，输出写进目标数组，可被 node 测试。
// ===========================================================================

function toGray(src, w, h, y0, y1, out) {
  for (let y = y0; y < y1; y++) {
    for (let x = 0; x < w; x++) {
      const i = y * w + x;
      out[i] = 0.2126 * src[i * 4] + 0.7152 * src[i * 4 + 1] + 0.0722 * src[i * 4 + 2];
    }
  }
  return out;
}

function prepareGray(src, w, h) {
  const gray = new Float32Array(w * h);
  // 参数顺序是 (src, w, h, y0, y1)：先前 (0, h, 0, w) 把宽高当成了行界，
  // 内层 x<0 永不执行，灰度图恒为全 0。
  toGray(src, w, h, 0, h, gray);
  return gray;
}

// 盒式模糊（两次累积，O(n) 与半径无关）。
function boxBlur(src, w, h, radius) {
  const tmp = new Float32Array(w * h);
  const out = new Float32Array(w * h);
  for (let y = 0; y < h; y++) {
    let acc = 0;
    const row = y * w;
    for (let x = -radius; x <= radius; x++) acc += src[row + Math.min(w - 1, Math.max(0, x))];
    for (let x = 0; x < w; x++) {
      tmp[row + x] = acc / (2 * radius + 1);
      acc += src[row + Math.min(w - 1, x + radius + 1)] - src[row + Math.max(0, x - radius)];
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

function sobelMagnitude(gray, w, h) {
  const out = new Float32Array(w * h);
  for (let y = 1; y < h - 1; y++) {
    for (let x = 1; x < w - 1; x++) {
      const i = y * w + x;
      const tl = gray[i - w - 1], tc = gray[i - w], tr = gray[i - w + 1];
      const ml = gray[i - 1], mr = gray[i + 1];
      const bl = gray[i + w - 1], bc = gray[i + w], br = gray[i + w + 1];
      const gx = tr + 2 * mr + br - tl - 2 * ml - bl;
      const gy = bl + 2 * bc + br - tl - 2 * tc - tr;
      out[i] = Math.hypot(gx, gy) / 4;
    }
  }
  return out;
}

function prepareStyle(style, src, w, h, p) {
  const pre = {};
  if (style === 'camo') {
    pre.fbm = makeFbm(101 + (p.rotation || 15) * 7, 5);
    pre.colors = extractPalette(src, p.colors);
  }
  if (style === 'glitch') {
    const rand = mulberry32(9124);
    pre.shifts = [];
    const blocks = Math.round((p.blocks / 100) * 24) + 1;
    for (let b = 0; b < blocks; b++) {
      const y0 = Math.floor(rand() * h);
      const len = Math.max(2, Math.floor(rand() * h * 0.12));
      pre.shifts.push({ y0, y1: Math.min(h, y0 + len), dx: Math.round((rand() * 2 - 1) * (p.split / 100) * w * 0.06) });
    }
  }
  if (style === 'sketch') {
    pre.gray = prepareGray(src, w, h);
    pre.blur = boxBlur(pre.gray, w, h, Math.max(2, Math.round(Math.min(w, h) / 90)));
    pre.edge = sobelMagnitude(pre.gray, w, h);
  }
  if (style === 'thermal' || style === 'halftone' || style === 'cross') {
    pre.gray = prepareGray(src, w, h);
  }
  if (style === 'film') {
    // 胶片的 halation 需要低频亮度和：同 Neon 辉光一样的理由整体预计算
    pre.blur = boxBlur(pre.gray, w, h, Math.max(2, Math.round(Math.min(w, h) / 80)));
  }
  if (style === 'datamosh') {
    // 块列表在 prepare 阶段按全图生成：渲染按条带分帧，块不能依赖 y0/y1
    const rand = mulberry32(555 + Math.round(p.density) * 7 + Math.round(p.size));
    const cell = Math.max(8, Math.round(p.size));
    const cols = Math.ceil(w / cell), rows = Math.ceil(h / cell);
    const count = Math.round(cols * rows * (p.density / 100) * 0.5);
    pre.blocks = [];
    for (let i = 0; i < count; i++) {
      pre.blocks.push({
        x: Math.floor(rand() * cols) * cell,
        y: Math.floor(rand() * rows) * cell,
        w: cell, h: cell,
        dx: Math.round((rand() * 2 - 1) * (1 + (p.smear / 100) * 8)),
        dy: Math.round((rand() < 0.4 ? rand() * 2 - 1 : 0) * (1 + (p.smear / 100) * 4)),
        repeat: Math.max(1, Math.round(rand() * (p.smear / 100) * 10)),
        jr: Math.round((rand() * 2 - 1) * p.tint * 0.5),
        jg: Math.round((rand() * 2 - 1) * p.tint * 0.35),
        jb: Math.round((rand() * 2 - 1) * -p.tint * 0.45),
      });
    }
  }
  if (style === 'tear') {
    const rand = mulberry32(777 + Math.round(p.bands) * 13 + Math.round(p.shift));
    const n = Math.max(2, Math.round(p.bands));
    pre.tears = [];
    for (let i = 0; i < n; i++) {
      pre.tears.push({
        yEnd: Math.round(((i + 1) * h) / n),
        dx: Math.round((rand() * 2 - 1) * (p.shift / 100) * w * 0.06),
        noisy: rand() < 0.3 + p.noise / 100,
      });
    }
  }
  if (style === 'decay') {
    const rand = mulberry32(888 + h + Math.round(p.strength));
    pre.rowJitter = new Array(h);
    for (let y = 0; y < h; y++) pre.rowJitter[y] = Math.round((rand() * 2 - 1) * Math.max(2, w * 0.012));
    const rand2 = mulberry32(889 + Math.round(p.strength) * 3);
    const cell = Math.max(4, Math.round(p.size));
    const cols = Math.ceil(w / cell), rows = Math.ceil(h / cell);
    const count = Math.round(cols * rows * (p.strength / 100) * 0.35);
    pre.ops = [];
    for (let i = 0; i < count; i++) {
      const roll = rand2();
      pre.ops.push({
        x: Math.floor(rand2() * cols) * cell,
        y: Math.floor(rand2() * rows) * cell,
        w: cell, h: cell,
        kind: roll < 0.4 ? 'invert' : roll < 0.75 ? 'swap' : 'boost',
      });
    }
  }
  if (style === 'crt') {
    pre.gray = prepareGray(src, w, h);
    pre.blur = boxBlur(pre.gray, w, h, 2);
    const rand = mulberry32(4455 + Math.round(p.roll));
    pre.rollY = Math.floor(rand() * h);
  }
  if (style === 'echo') {
    const rand = mulberry32(999 + Math.round(p.ghosts) * 31 + Math.round(p.offset));
    const n = Math.max(1, Math.round(p.ghosts));
    const budget = 0.55 + (p.fade / 100) * 0.35;
    const per = budget / n;
    const dist = Math.max(2, (p.offset / 100) * Math.min(w, h) * 0.06);
    pre.ghosts = [];
    for (let i = 0; i < n; i++) {
      pre.ghosts.push({
        dx: Math.round((rand() * 2 - 1) * dist),
        dy: Math.round((rand() * 2 - 1) * dist * 0.4),
        weight: per,
        dr: rand() * 0.5,
        db: rand() * 0.5,
      });
    }
  }
  if (style === 'duotone' || style === 'watercolor' || style === 'film' || style === 'woodcut') {
    pre.gray = prepareGray(src, w, h);
  }
  if (style === 'lowpoly' || style === 'pixel') {
    pre.palette = (p.palette === 'warm' || p.palette === 'cool') ? p.palette : 'auto';
  }
  if (style === 'watercolor') {
    pre.fbm = makeFbm(7777, 4);
  }
  if (style === 'glass' || style === 'pastel' || style === 'holo') {
    // 三个样式的渲染器都读单通道 pre.gray / pre.blur：整图预计算一次，
    // 不放渲染函数里（分帧会重算 14 次）。玻璃的模糊半径由参数驱动。
    pre.gray = prepareGray(src, w, h);
    const radius = style === 'glass'
      ? Math.max(1, Math.round((p.blur || 12) / 5))
      : Math.max(2, Math.round(Math.min(w, h) / (style === 'holo' ? 60 : 80)));
    pre.blur = boxBlur(pre.gray, w, h, radius);
  }
  return pre;
}

const HALFTONE_LUT = {
  iron: [[0,0,0],[64,0,64],[128,0,128],[192,32,96],[255,96,0],[255,200,0],[255,255,220]],
  rainbow: [[20,0,80],[0,60,255],[0,200,90],[255,230,0],[255,60,0]],
  nightvision: [[0,0,0],[6,60,18],[16,140,40],[90,230,90],[210,255,200]],
  gold: [[20,10,0],[90,50,0],[180,120,10],[255,210,90],[255,250,210]],
  ice: [[0,10,30],[0,70,140],[40,170,220],[160,230,255],[240,252,255]],
};
function lutColor(stops, t) {
  const n = stops.length - 1;
  const f = Math.max(0, Math.min(1, t)) * n;
  const i = Math.min(n - 1, Math.floor(f));
  const k = f - i;
  const a = stops[i], b = stops[i + 1];
  return [a[0] + (b[0] - a[0]) * k, a[1] + (b[1] - a[1]) * k, a[2] + (b[2] - a[2]) * k];
}

function renderRows(style, dst, src, w, h, y0, y1, p, pre) {
  switch (style) {
    case 'glitch': return renderGlitch(dst, src, w, h, y0, y1, p, pre);
    case 'camo': return renderCamo(dst, src, w, h, y0, y1, p, pre);
    case 'pixelsort': return renderPixelsort(dst, src, w, h, y0, y1, p);
    case 'datamosh': return renderDatamosh(dst, src, w, h, y0, y1, p, pre);
    case 'tear': return renderTear(dst, src, w, h, y0, y1, p, pre);
    case 'decay': return renderDecay(dst, src, w, h, y0, y1, p, pre);
    case 'crt': return renderCrt(dst, src, w, h, y0, y1, p, pre);
    case 'echo': return renderEcho(dst, src, w, h, y0, y1, p, pre);
    case 'oil': return renderOil(dst, src, w, h, y0, y1, p);
    case 'halftone': return renderHalftone(dst, src, w, h, y0, y1, p);
    case 'sketch': return renderSketch(dst, src, w, h, y0, y1, p, pre);
    case 'thermal': return renderThermal(dst, src, w, h, y0, y1, p, pre);
    case 'cross': return renderCross(dst, src, w, h, y0, y1, p);
    case 'duotone': return renderDuotone(dst, w, h, y0, y1, p, pre.gray);
    case 'watercolor': return renderWatercolor(dst, src, w, h, y0, y1, p, pre);
    case 'lowpoly': return renderLowpoly(dst, src, w, h, y0, y1, p, pre);
    case 'pixel': return renderPixelArt(dst, src, w, h, y0, y1, p);
    case 'woodcut': return renderWoodcut(dst, src, w, h, y0, y1, p, pre.gray);
    case 'film': return renderFilm(dst, src, w, h, y0, y1, p, pre.gray, pre.blur);
    case 'ripple': return renderRipple(dst, src, w, h, y0, y1, p, pre);
    case 'glass': return renderGlassR(dst, src, w, h, y0, y1, p, pre);
    case 'mosaic': return renderMosaic(dst, src, w, h, y0, y1, p);
    case 'heatwave': return renderHeatwave(dst, src, w, h, y0, y1, p, pre);
    case 'pastel': return renderPastel(dst, src, w, h, y0, y1, p, pre);
    case 'holo': return renderHolo(dst, src, w, h, y0, y1, p, pre);
    default: return copyRows(dst, src, w, y0, y1);
  }
}

function copyRows(dst, src, w, y0, y1) {
  for (let y = y0; y < y1; y++) {
    const row = y * w * 4;
    for (let x = 0; x < w * 4; x++) dst[row + x] = src[row + x];
  }
}

// 故障艺术：通道色散 + 条带位移 + 行波 + 扫描线 + 噪点。
function renderGlitch(dst, src, w, h, y0, y1, p, pre) {
  const rand = mulberry32(31);
  for (let y = y0; y < y1; y++) {
    let wave = 0;
    if (p.wave > 0) wave = Math.sin(y * 0.07) * (p.wave / 100) * 26;
    const scan = p.scanline > 0 && y % 2 === 0 ? 1 - p.scanline / 160 : 1;
    for (let x = 0; x < w; x++) {
      const i = y * w + x;
      const shift = (p.split / 100) * 14;
      const rX = Math.min(w - 1, Math.max(0, x + Math.round(shift + wave * 0.4)));
      const gX = Math.min(w - 1, Math.max(0, x + Math.round(wave * 0.2)));
      const bX = Math.min(w - 1, Math.max(0, x - Math.round(shift - wave * 0.4)));
      let r = src[(y * w + rX) * 4], g = src[(y * w + gX) * 4 + 1], b = src[(y * w + bX) * 4 + 2];
      for (const s of pre.shifts) {
        if (y >= s.y0 && y < s.y1) {
          const sx = Math.min(w - 1, Math.max(0, x - s.dx));
          r = src[(y * w + sx) * 4]; g = src[(y * w + sx) * 4 + 1]; b = src[(y * w + sx) * 4 + 2];
          break;
        }
      }
      const n = p.noise > 0 ? (rand() - 0.5) * (p.noise / 100) * 90 : 0;
      dst[i * 4] = Math.max(0, Math.min(255, r * scan + n));
      dst[i * 4 + 1] = Math.max(0, Math.min(255, g * scan + n * 0.6));
      dst[i * 4 + 2] = Math.max(0, Math.min(255, b * scan + n * 0.3));
      dst[i * 4 + 3] = src[i * 4 + 3];
    }
  }
}

// 迷彩：fBm 场量化到调色板；图案控制噪声形态，contrast 控制档间过渡。
function renderCamo(dst, src, w, h, y0, y1, p, pre) {
  const palette = pre.colors;
  const freq = 100 / (p.scale * 4);
  const rot = (p.rotation || 15) * Math.PI / 180;
  const cosR = Math.cos(rot), sinR = Math.sin(rot);
  for (let y = y0; y < y1; y++) {
    for (let x = 0; x < w; x++) {
      const u = x * cosR - y * sinR, v = x * sinR + y * cosR;
      let n = pre.fbm(u * freq * 0.02, v * freq * 0.02);
      if (p.pattern === 'digital') {
        const cellSize = Math.max(2, Math.round(p.scale / 4));
        n = pre.fbm(Math.floor(x / cellSize) * 0.13, Math.floor(y / cellSize) * 0.13);
      }
      const t = Math.min(p.colors - 1, Math.max(0, Math.floor(n * p.colors * (0.75 + p.sharp / 200))));
      const c = palette[Math.min(palette.length - 1, t)];
      const i = (y * w + x) * 4;
      dst[i] = c[0]; dst[i + 1] = c[1]; dst[i + 2] = c[2]; dst[i + 3] = src ? src[i + 3] : 255;
    }
  }
}

// 油画厚涂：四象限最小方差均值（Kuwahara）+ 色阶量化，smooth 与原图混合。
function renderOil(dst, src, w, h, y0, y1, p) {
  const r = Math.round(p.radius);
  const levels = Math.round(p.levels);
  const mix = p.smooth / 100;
  for (let y = y0; y < y1; y++) {
    for (let x = 0; x < w; x++) {
      let best = null, bestVar = Infinity;
      const quads = [[-1, -1], [1, -1], [-1, 1], [1, 1]];
      for (const [qx, qy] of quads) {
        let sr = 0, sg = 0, sb = 0, sl = 0, sl2 = 0, count = 0;
        const xa = qx < 0 ? -r : 0, xb = qx < 0 ? 0 : r;
        const ya = qy < 0 ? -r : 0, yb = qy < 0 ? 0 : r;
        for (let dy = ya; dy <= yb; dy++) {
          const yy = Math.min(h - 1, Math.max(0, y + dy));
          for (let dx = xa; dx <= xb; dx++) {
            const xx = Math.min(w - 1, Math.max(0, x + dx));
            const i = (yy * w + xx) * 4;
            const l = 0.299 * src[i] + 0.587 * src[i + 1] + 0.114 * src[i + 2];
            sr += src[i]; sg += src[i + 1]; sb += src[i + 2]; sl += l; sl2 += l * l; count++;
          }
        }
        const variance = sl2 / count - (sl / count) * (sl / count);
        if (variance < bestVar) {
          bestVar = variance;
          best = [sr / count, sg / count, sb / count];
        }
      }
      const i4 = (y * w + x) * 4;
      const q = v => Math.round(Math.round(v / 255 * (levels - 1)) / (levels - 1) * 255);
      const blended = [
        best[0] * (1 - mix) + src[i4] * mix,
        best[1] * (1 - mix) + src[i4 + 1] * mix,
        best[2] * (1 - mix) + src[i4 + 2] * mix,
      ];
      dst[i4] = q(blended[0]);
      dst[i4 + 1] = q(blended[1]);
      dst[i4 + 2] = q(blended[2]);
      dst[i4 + 3] = src[i4 + 3];
    }
  }
}

// 半调印刷：灰度 = 旋转网格阈值网点；四色 = CMY 三屏不同角度叠印。
function renderHalftone(dst, src, w, h, y0, y1, p) {
  const cell = Math.max(2, Math.round(p.cell));
  const sharpen = 1 + p.sharpen / 60;
  const angle = p.angle * Math.PI / 180;
  const cosA = Math.cos(angle), sinA = Math.sin(angle);
  const half = cell / 2;
  const inDot = (lx, ly, k) => {
    if (k <= 0.02) return false;
    const r = Math.sqrt(k) * half * 1.35;
    if (p.shape === 'circle') return lx * lx + ly * ly <= r * r;
    if (p.shape === 'square') return Math.abs(lx) <= r && Math.abs(ly) <= r;
    if (p.shape === 'diamond') return Math.abs(lx) + Math.abs(ly) <= r * 1.25;
    if (p.shape === 'line') return Math.abs(ly) <= k * half;
    return Math.abs(lx) <= r * 0.45 || Math.abs(ly) <= r * 0.45 ? Math.hypot(lx, ly) <= r * 1.1 : false;
  };
  const threshold = v => Math.max(0, Math.min(1, (v - 0.5) * sharpen + 0.5));
  for (let y = y0; y < y1; y++) {
    for (let x = 0; x < w; x++) {
      const i = (y * w + x) * 4;
      const r0 = src[i] / 255, g0 = src[i + 1] / 255, b0 = src[i + 2] / 255;
      const gray = Math.max(0, Math.min(1, threshold(0.2126 * r0 + 0.7152 * g0 + 0.0722 * b0)));
      let out = [255, 255, 255];
      if (p.mode === 'gray') {
        const sx = x * cosA - y * sinA, sy = x * sinA + y * cosA;
        const cellX = Math.floor(sx / cell) * cell + half;
        const cellY = Math.floor(sy / cell) * cell + half;
        const lx = sx - cellX, ly = sy - cellY;
        out = inDot(lx, ly, 1 - gray) ? [20, 20, 20] : [250, 250, 250];
      } else {
        const c = 1 - r0, m = 1 - g0, yl = 1 - b0;
        const angles = [15, 75, 0];
        const inks = [[0, 160, 190], [200, 40, 110], [250, 210, 20]];
        let acc = [255, 255, 255];
        for (let ch = 0; ch < 3; ch++) {
          const a2 = angles[ch] * Math.PI / 180;
          const c2 = Math.cos(a2), s2 = Math.sin(a2);
          const sx = x * c2 - y * s2, sy = x * s2 + y * c2;
          const cellX = Math.floor(sx / cell) * cell + half;
          const cellY = Math.floor(sy / cell) * cell + half;
          const lx = sx - cellX, ly = sy - cellY;
          const k = [c, m, yl][ch];
          if (inDot(lx, ly, Math.min(1, k * 1.15))) {
            const ink = inks[ch];
            acc = [acc[0] * (ink[0] / 255), acc[1] * (ink[1] / 255), acc[2] * (ink[2] / 255)];
          }
        }
        out = acc;
      }
      dst[i] = out[0]; dst[i + 1] = out[1]; dst[i + 2] = out[2]; dst[i + 3] = src[i + 3];
    }
  }
}

// 素描炭笔：反色减淡 + Sobel 轮廓 + 纸面颗粒。
function renderSketch(dst, src, w, h, y0, y1, p, pre) {
  const pencil = p.pencil / 100;
  const rand = mulberry32(4321);
  for (let y = y0; y < y1; y++) {
    for (let x = 0; x < w; x++) {
      const i = y * w + x;
      const g = pre.gray[i];
      const b = pre.blur[i];
      const dodge = Math.min(255, (g * 255) / Math.max(1, 255 - b));
      const edge = Math.min(1, pre.edge[i] / 90) * (p.edge / 100);
      let v = dodge * (1 - pencil * 0.55) + 20 * pencil - edge * 90;
      const grain = p.grain > 0 ? (rand() - 0.5) * (p.grain / 100) * 40 : 0;
      v = Math.max(0, Math.min(255, v + grain));
      if (p.invert) v = 255 - v;
      dst[i * 4] = dst[i * 4 + 1] = dst[i * 4 + 2] = v;
      dst[i * 4 + 3] = src[i * 4 + 3];
    }
  }
}

// 热感假彩：亮度 → 多段渐变 LUT，与原图按 mix 混合。
function renderThermal(dst, src, w, h, y0, y1, p, pre) {
  const stops = HALFTONE_LUT[p.lut] || HALFTONE_LUT.iron;
  const mix = p.mix / 100;
  const contrast = 1 + p.contrast / 60;
  for (let y = y0; y < y1; y++) {
    for (let x = 0; x < w; x++) {
      const i = (y * w + x) * 4;
      // gray 是每像素单通道（步长 1），i 是 RGBA 步长 4
      const g = Math.max(0, Math.min(255, (pre.gray[i / 4] - 128) * contrast + 128));
      const c = lutColor(stops, g / 255);
      dst[i] = src[i] * (1 - mix) + c[0] * mix;
      dst[i + 1] = src[i + 1] * (1 - mix) + c[1] * mix;
      dst[i + 2] = src[i + 2] * (1 - mix) + c[2] * mix;
      dst[i + 3] = src[i + 3];
    }
  }
}

// 像素排序：行内把超过（或低于）亮度阈值的连续段按亮度排序，段上限与
// 打散程度控制“融化感”。纯确定性：无全局随机，行内局部可并行。
function renderPixelsort(dst, src, w, h, y0, y1, p) {
  const thr = (p.threshold / 100) * 255;
  const span = Math.max(2, Math.round((p.span / 100) * w));
  const shuffle = (p.shuffle / 100) * 0.35;
  const light = p.mode !== 'dark';
  const lumBuf = new Float64Array(w);
  for (let y = y0; y < y1; y++) {
    const row = y * w * 4;
    for (let x = 0; x < w; x++) {
      const i = row + x * 4;
      lumBuf[x] = 0.299 * src[i] + 0.587 * src[i + 1] + 0.114 * src[i + 2];
    }
    const rowRand = mulberry32(9127 + y * 131);
    let x = 0;
    while (x < w) {
      const qualifies = light ? lumBuf[x] >= thr : lumBuf[x] <= thr;
      if (!qualifies) { x++; continue; }
      let end = x + 1;
      while (end < w && end - x < span && (light ? lumBuf[end] >= thr : lumBuf[end] <= thr)) end++;
      const len = end - x;
      const idx = Array.from({ length: len }, (_, k) => k);
      idx.sort((a, b) => lumBuf[x + a] - lumBuf[x + b]);
      if (shuffle > 0) {
        for (let k = 0; k + 1 < len; k++) {
          if (rowRand() < shuffle) { const t = idx[k]; idx[k] = idx[k + 1]; idx[k + 1] = t; }
        }
      }
      for (let k = 0; k < len; k++) {
        const from = row + (x + idx[k]) * 4, to = row + (x + k) * 4;
        dst[to] = src[from]; dst[to + 1] = src[from + 1]; dst[to + 2] = src[from + 2]; dst[to + 3] = src[from + 3];
      }
      x = end;
    }
  }
}

// 坏块流动：视频编码运动向量损坏的观感——块从原位沿方向逐帧拷贝拖影，
// 叠加每块独立的通道色偏。块列表在 prepareStyle 生成，条带间稳定。
function renderDatamosh(dst, src, w, h, y0, y1, p, pre) {
  const blocks = pre.blocks || [];
  for (const b of blocks) {
    const by0 = Math.max(y0, b.y), by1 = Math.min(y1, b.y + b.h);
    if (by1 <= by0) continue;
    for (let k = 1; k <= b.repeat; k++) {
      const decayK = 1 - (k / (b.repeat + 1)) * 0.25;
      for (let yy = by0; yy < by1; yy++) {
        const ty = yy + b.dy * k;
        if (ty < y0 || ty >= y1) continue;
        for (let xx = 0; xx < b.w; xx++) {
          const sx = b.x + xx, tx = sx + b.dx * k;
          if (tx < 0 || tx >= w) continue;
          const s = (yy * w + sx) * 4, t = (ty * w + tx) * 4;
          dst[t] = Math.max(0, Math.min(255, src[s] * decayK + b.jr));
          dst[t + 1] = Math.max(0, Math.min(255, src[s + 1] * decayK + b.jg));
          dst[t + 2] = Math.max(0, Math.min(255, src[s + 2] * decayK + b.jb));
          dst[t + 3] = src[s + 3];
        }
      }
    }
  }
}

// 信号撕裂：水平撕裂带每段整体横向错位，r/b 通道附加反向色散；
// 撕裂边界按行撒种子亮点模拟信号丢失。
function renderTear(dst, src, w, h, y0, y1, p, pre) {
  const tears = pre.tears || [];
  const split = Math.round((p.split / 100) * 14);
  const noiseK = (p.noise / 100) * 0.06;
  for (let y = y0; y < y1; y++) {
    const seg = tears.find(t => y < t.yEnd) || tears[tears.length - 1];
    if (!seg) continue;
    const row = y * w * 4;
    const rowRand = mulberry32(6600 + y * 17);
    const speckle = seg.noisy && noiseK > 0;
    for (let x = 0; x < w; x++) {
      const i = row + x * 4;
      if (speckle && rowRand() < noiseK) {
        const v = 110 + rowRand() * 145;
        dst[i] = dst[i + 1] = dst[i + 2] = v;
        continue;
      }
      const sx = Math.min(w - 1, Math.max(0, x + seg.dx));
      const sxr = Math.min(w - 1, Math.max(0, x + seg.dx + split));
      const sxb = Math.min(w - 1, Math.max(0, x + seg.dx - split));
      dst[i] = src[row + sxr * 4];
      dst[i + 1] = src[row + sx * 4 + 1];
      dst[i + 2] = src[row + sxb * 4 + 2];
      dst[i + 3] = src[i + 3];
    }
  }
}

// 数据腐蚀：低色深量化 + 随机行错扫 + 块级损坏（反转/通道交换/过饱和）。
function renderDecay(dst, src, w, h, y0, y1, p, pre) {
  const levels = Math.max(2, Math.round(p.levels));
  const step = 255 / (levels - 1);
  const scanK = (p.scan / 100) * 0.3;
  const jitter = pre.rowJitter || [];
  for (let y = y0; y < y1; y++) {
    const rowRand = mulberry32(7700 + y * 29);
    const scanRow = scanK > 0 && rowRand() < scanK;
    const jx = scanRow ? jitter[y % jitter.length] || 0 : 0;
    const row = y * w * 4;
    for (let x = 0; x < w; x++) {
      const sx = scanRow ? Math.min(w - 1, Math.max(0, x + jx)) : x;
      const i = row + x * 4, s = row + sx * 4;
      dst[i] = Math.round(src[s] / step) * step;
      dst[i + 1] = Math.round(src[s + 1] / step) * step;
      dst[i + 2] = Math.round(src[s + 2] / step) * step;
      dst[i + 3] = src[s + 3];
    }
  }
  for (const op of pre.ops || []) {
    const oy0 = Math.max(y0, op.y), oy1 = Math.min(y1, op.y + op.h);
    if (oy1 <= oy0) continue;
    for (let y = oy0; y < oy1; y++) {
      const row = y * w * 4;
      for (let x = op.x; x < Math.min(w, op.x + op.w); x++) {
        const i = row + x * 4;
        if (op.kind === 'invert') {
          dst[i] = 255 - dst[i]; dst[i + 1] = 255 - dst[i + 1]; dst[i + 2] = 255 - dst[i + 2];
        } else if (op.kind === 'swap') {
          const t = dst[i]; dst[i] = dst[i + 2]; dst[i + 2] = t;
        } else {
          dst[i] = Math.min(255, dst[i] * 1.6 + 30);
          dst[i + 1] = Math.min(255, dst[i + 1] * 0.7);
        }
      }
    }
  }
}

// CRT 显像管：桶形畸变采样 + 荫罩三色栅格 + 模糊辉光 + 扫描线 + 暗角
// + 滚动亮带。全部逐像素确定性计算。
function renderCrt(dst, src, w, h, y0, y1, p, pre) {
  const curve = (p.curve / 100) * 0.12;
  const mask = (p.mask / 100) * 0.5;
  const glow = p.glow / 100;
  const vig = (p.vignette / 100) * 0.55;
  const roll = (p.roll / 100) * 0.5;
  const cx = w / 2, cy = h / 2;
  const rollY = pre.rollY || 0;
  for (let y = y0; y < y1; y++) {
    const ny = (y - cy) / cy;
    for (let x = 0; x < w; x++) {
      const nx = (x - cx) / cx;
      const r2 = nx * nx + ny * ny;
      const f = 1 + curve * r2;
      const sx = Math.min(w - 1, Math.max(0, Math.round(cx + nx * f * cx)));
      const sy = Math.min(h - 1, Math.max(0, Math.round(cy + ny * f * cy)));
      const si = (sy * w + sx) * 4, i = (y * w + x) * 4;
      let r = src[si], g = src[si + 1], b = src[si + 2];
      if (glow > 0) {
        const g2 = pre.blur[sy * w + sx] / 255;
        const gb = g2 * 60 * glow;
        r += gb; g += gb; b += gb;
      }
      const m = x % 3;
      if (m === 0) { r *= 1 + mask; g *= 1 - mask * 0.35; b *= 1 - mask * 0.35; }
      else if (m === 1) { r *= 1 - mask * 0.35; g *= 1 + mask; b *= 1 - mask * 0.35; }
      else { r *= 1 - mask * 0.35; g *= 1 - mask * 0.35; b *= 1 + mask; }
      if (y % 2 === 1) { r *= 0.85; g *= 0.85; b *= 0.85; }
      const vk = 1 - vig * Math.min(1, r2);
      r *= vk; g *= vk; b *= vk;
      if (roll > 0) {
        const d = Math.abs(((y - rollY) % h + h) % h);
        if (d < 18) {
          const boost = (1 - d / 18) * roll;
          r *= 1 + boost; g *= 1 + boost; b *= 1 + boost;
        }
      }
      dst[i] = Math.max(0, Math.min(255, r));
      dst[i + 1] = Math.max(0, Math.min(255, g));
      dst[i + 2] = Math.max(0, Math.min(255, b));
      dst[i + 3] = src[i + 3];
    }
  }
}

// 信号重影：多份错位半透明叠加模拟天线重影，残影 r/b 通道附加色散。
function renderEcho(dst, src, w, h, y0, y1, p, pre) {
  const ghosts = pre.ghosts || [];
  const ab = p.aberration / 100;
  let total = 0;
  for (const gh of ghosts) total += gh.weight;
  const baseW = Math.max(0, 1 - total);
  for (let y = y0; y < y1; y++) {
    for (let x = 0; x < w; x++) {
      const i = (y * w + x) * 4;
      let r = 0, g = 0, b = 0;
      for (const gh of ghosts) {
        const sx = Math.min(w - 1, Math.max(0, x + gh.dx));
        const sy = Math.min(h - 1, Math.max(0, y + gh.dy));
        const si = (sy * w + sx) * 4;
        r += src[si] * gh.weight * (1 + ab * gh.dr);
        g += src[si + 1] * gh.weight;
        b += src[si + 2] * gh.weight * (1 - ab * gh.db);
      }
      r += src[i] * baseW;
      g += src[i + 1] * baseW;
      b += src[i + 2] * baseW;
      dst[i] = Math.max(0, Math.min(255, r));
      dst[i + 1] = Math.max(0, Math.min(255, g));
      dst[i + 2] = Math.max(0, Math.min(255, b));
      dst[i + 3] = src[i + 3];
    }
  }
}

// 十字绣：色阶量化到网格，每格两针交叉 + 布纹 + 可选格线。
function renderCross(dst, src, w, h, y0, y1, p) {
  const cell = Math.max(3, Math.round(p.stitch));
  const levels = Math.max(2, Math.round(p.levels));
  for (let y = y0; y < y1; y++) {
    for (let x = 0; x < w; x++) {
      const i = (y * w + x) * 4;
      const cellX = Math.floor(x / cell), cellY = Math.floor(y / cell);
      const cx = Math.min(w - 1, cellX * cell + (cell >> 1));
      const cy = Math.min(h - 1, cellY * cell + (cell >> 1));
      const ci = (cy * w + cx) * 4;
      const q = v => Math.round(Math.round(v / 255 * (levels - 1)) / (levels - 1) * 255);
      let r = q(src[ci]), g = q(src[ci + 1]), b = q(src[ci + 2]);
      const lx = x % cell, ly = y % cell;
      const border = p.grid && (lx === 0 || ly === 0);
      if (border) {
        r = Math.max(0, r - 46); g = Math.max(0, g - 46); b = Math.max(0, b - 46);
      } else {
        const onStitch = Math.abs(lx - ly) <= 0.8 || Math.abs(lx + ly - cell) <= 0.8;
        const fabric = p.fabric > 0 ? ((cellX + cellY) % 2 ? 8 : -8) * (p.fabric / 100) : 0;
        const k = onStitch ? 14 : -10;
        r += k + fabric; g += k + fabric; b += k * 0.8 + fabric;
      }
      dst[i] = Math.max(0, Math.min(255, r));
      dst[i + 1] = Math.max(0, Math.min(255, g));
      dst[i + 2] = Math.max(0, Math.min(255, b));
      dst[i + 3] = src[i + 3];
    }
  }
}


function glassDispatch(dst, src, w, h, y0, y1, p, pre) {
  const gray3 = pre.gray3 || prepareGray(src, w, h);
  const blur3 = pre.blur3 || boxBlur(gray3, w, h, Math.max(1, Math.round((p.blur || 12) / 5)));
  renderGlassR(dst, src, w, h, y0, y1, p, { gray: gray3, blur: blur3 });
}
function mosaicDispatch(dst, src, w, h, y0, y1, p) {
  renderMosaic(dst, src, w, h, y0, y1, p);
}
function heatwaveDispatch(dst, src, w, h, y0, y1, p) {
  renderHeatwave(dst, src, w, h, y0, y1, p);
}
function pastelDispatch(dst, src, w, h, y0, y1, p, pre) {
  const gray3 = pre.gray3 || prepareGray(src, w, h);
  const blur3 = pre.blur3 || boxBlur(gray3, w, h, Math.max(2, Math.round(Math.min(w, h) / 80)));
  renderPastel(dst, src, w, h, y0, y1, p, { gray: gray3, blur: blur3 });
}
function holoDispatch(dst, src, w, h, y0, y1, p, pre) {
  const gray3 = pre.gray3 || prepareGray(src, w, h);
  const blur3 = pre.blur3 || boxBlur(gray3, w, h, Math.max(2, Math.round(Math.min(w, h) / 60)));
  renderHolo(dst, src, w, h, y0, y1, p, { gray: gray3, blur: blur3 });
}
