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
  camo: [
    { key: 'pattern', type: 'select', label: '图案', options: [['blotch', '斑块'], ['digital', '数码'], ['leopard', '豹纹'], ['stripe', '条纹'], ['crack', '裂纹']] },
    { key: 'colors', type: 'range', label: '色彩数', min: 2, max: 8, step: 1 },
    { key: 'scale', type: 'range', label: '斑块尺度', min: 8, max: 100, step: 1 },
    { key: 'sharp', type: 'range', label: '边缘锐度', min: 0, max: 100, step: 1 },
    { key: 'rotation', type: 'range', label: '旋转', min: 0, max: 90, step: 1 },
    { key: 'contrast', type: 'range', label: '明暗对比', min: 10, max: 100, step: 1 },
  ],
  wear: [
    { key: 'strength', type: 'range', label: '磨损强度', min: 0, max: 100, step: 1 },
    { key: 'edge', type: 'range', label: '边缘增强', min: 0, max: 100, step: 1 },
    { key: 'scratches', type: 'range', label: '划痕数量', min: 0, max: 100, step: 1 },
    { key: 'grain', type: 'range', label: '颗粒', min: 0, max: 100, step: 1 },
    { key: 'baseColor', type: 'check', label: '露出底漆色' },
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
  neon: [
    { key: 'edge', type: 'range', label: '边缘阈值', min: 10, max: 100, step: 1 },
    { key: 'glow', type: 'range', label: '辉光', min: 0, max: 100, step: 1 },
    { key: 'hue', type: 'range', label: '色相', min: 0, max: 360, step: 5 },
    { key: 'dark', type: 'range', label: '背景压暗', min: 0, max: 100, step: 1 },
  ],
  cross: [
    { key: 'levels', type: 'range', label: '色彩级数', min: 3, max: 24, step: 1 },
    { key: 'stitch', type: 'range', label: '绣格尺寸', min: 4, max: 24, step: 1 },
    { key: 'fabric', type: 'range', label: '布纹', min: 0, max: 100, step: 1 },
    { key: 'grid', type: 'check', label: '显示格线' },
  ],
  marble: [
    { key: 'palette', type: 'select', label: '调色板', options: [['auto', '取色于原图'], ['blackwhite', '黑白'], ['jade', '青玉'], ['amber', '琥珀']] },
    { key: 'scale', type: 'range', label: '纹理尺度', min: 10, max: 100, step: 1 },
    { key: 'octaves', type: 'range', label: '细节倍频', min: 2, max: 7, step: 1 },
    { key: 'turbulence', type: 'range', label: '湍流强度', min: 5, max: 100, step: 1 },
    { key: 'vein', type: 'range', label: '脉络对比', min: 10, max: 90, step: 1 },
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
  ],
  camo: [
    { name: '林地迷彩', settings: { pattern: 'blotch', colors: 4, scale: 55, sharp: 35, contrast: 65 } },
    { name: '数码迷彩', settings: { pattern: 'digital', colors: 4, scale: 30, sharp: 90, contrast: 70 } },
    { name: '豹纹点', settings: { pattern: 'leopard', colors: 3, scale: 22, sharp: 70, contrast: 80 } },
  ],
  wear: [
    { name: '战损掉漆', settings: { strength: 70, edge: 80, scratches: 55, grain: 35, baseColor: true } },
    { name: '轻度旧化', settings: { strength: 30, edge: 50, scratches: 15, grain: 20, baseColor: false } },
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
  neon: [
    { name: '赛博青蓝', settings: { edge: 65, glow: 60, hue: 185, dark: 75 } },
    { name: '霓虹粉紫', settings: { edge: 55, glow: 70, hue: 305, dark: 70 } },
  ],
  cross: [
    { name: '复古十字绣', settings: { levels: 8, stitch: 12, fabric: 45, grid: true } },
    { name: '精细绣格', settings: { levels: 16, stitch: 6, fabric: 25, grid: false } },
  ],
  marble: [
    { name: '黑白大理石', settings: { palette: 'blackwhite', scale: 55, octaves: 5, turbulence: 55, vein: 55 } },
    { name: '青玉纹理', settings: { palette: 'jade', scale: 40, octaves: 6, turbulence: 65, vein: 45 } },
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

export function createStyleLab({ root, inspector, runArea, desktop, open, saveDialog, convertFileSrc, invoke, settings, save, setBusy, busy, changed, notify }) {
  let config = restoreStyleLabSettings(settings), source = null, sourceName = "", result = null, disposed = false;
  let active = false, exporting = false, importing = false, computing = false, revision = 0, importRevision = 0;
  let view = 'result', zoom = 1, saveTimer;
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
      chip.onclick = () => { config = restoreStyleLabSettings({ style: config.style, ...preset.settings, style: config.style }); syncControls(); schedule(); };
      presetWrap.append(chip);
    }
    // 用户自定义预设
    const saved = config.customPresets || [];
    for (const preset of saved) {
      const chip = document.createElement('button');
      chip.type = 'button'; chip.className = 'style-lab-preset-chip'; chip.textContent = preset.name;
      chip.onclick = () => { config = restoreStyleLabSettings({ style: config.style, ...preset.settings, style: config.style }); syncControls(); schedule(); };
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
        const label = document.createElement('label');
        label.innerHTML = `${p.label} <output hidden>${p.default ?? ''}</output><input type="range" min="${p.min}" max="${p.max}" step="${p.step}" data-param="${p.key}">`;
        grid.append(label);
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
    if (resetButton) controls.append(resetButton);

    // 导出设置
    const exportSection = document.createElement('section');
    exportSection.className = 'inspector-group';
    exportSection.innerHTML = `<button class="group-toggle" type="button" aria-expanded="true"><span>导出设置</span><i data-lucide="chevron-down"></i></button><div class="group-content"><label>导出倍率<select data-stylize-exportScale><option value="1">1×</option><option value="2">2×</option><option value="4">4×</option></select></label><small>倍率越高导出越清晰（不影响预览）。</small></div>`;
    controls.append(exportSection);
    const exportScaleEl = exportSection.querySelector('[data-stylize-exportScale]');
    if (exportScaleEl) exportScaleEl.value = String(config.exportScale || 1);
    if (exportScaleEl) exportScaleEl.onchange = () => { config.exportScale = Number(exportScaleEl.value); persist(); };
    syncControls();
  }

  function syncControls() {
    for (const el of controls.querySelectorAll('[data-param]')) {
      const key = el.dataset.param;
      if (el.type === 'checkbox') el.checked = Boolean(config[key]);
      else el.value = String(config[key]);
    }
    controls.querySelectorAll('select').forEach(select => { if (select.dataset.selectSkip !== 'true') select.dispatchEvent(new Event('styled')); });
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
      if (current !== revision || disposed) return;
      try {
        const w = source.width, h = source.height;
        const src = source.getContext('2d').getImageData(0, 0, w, h);
        const pre = prepareStyle(config.style, src.data, w, h, config);
        const out = new ImageData(new Uint8ClampedArray(src.data), w, h);
        const band = Math.max(1, Math.ceil(h / 14));
        let y0 = 0;
        const step = () => {
          if (current !== revision || disposed) { computing = false; return; }
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
  for (const el of controls.querySelectorAll('[data-param]')) {
    el.addEventListener(el.tagName === 'SELECT' || el.type === 'checkbox' ? 'change' : 'input', () => {
      const key = el.dataset.param;
      config[key] = el.type === 'checkbox' ? el.checked : el.type === 'range' ? Number(el.value) : el.value;
      schedule();
    });
  }
  const resetButton = document.createElement('button');
  resetButton.className = 'secondary-action'; resetButton.type = 'button'; resetButton.textContent = '恢复默认';
  resetButton.onclick = () => { config = restoreStyleLabSettings({ style: config.style, ...STYLE_DEFAULTS[config.style] }); syncControls(); schedule(); };
  controls.append(resetButton);
  const zoomInput = $('zoom');
  zoomInput.oninput = () => { zoom = Number(zoomInput.value); fit(); };
  $('fit').onclick = () => { zoom = 1; zoomInput.value = 1; fit(); };
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
    activate(mode) {
      active = STYLE_IDS.some(id => mode === `stylize-${id}`);
      controls.hidden = !active;
      if (!active) { ++revision; return; }
      const style = mode.replace('stylize-', '');
      if (config.style !== style) {
        config.style = style;
        config = restoreStyleLabSettings({ style, ...STYLE_DEFAULTS[style] });
        renderControls();
      }
      refresh(); schedule();
    },
    addFiles(paths) { if (paths.length === 1) loadFile(paths[0]); else status('请一次拖入一张图片。'); },
    dispose() { disposed = true; ++revision; clearTimeout(saveTimer); resizeObserver.disconnect(); controls.remove(); },
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
  toGray(src, 0, h, 0, w, gray);
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
  if (style === 'wear') {
    pre.gray = prepareGray(src, w, h);
    pre.edge = sobelMagnitude(pre.gray, w, h);
    pre.fbm = makeFbm(4242, 5);
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
  if (style === 'sketch' || style === 'neon') {
    pre.gray = prepareGray(src, w, h);
    pre.blur = boxBlur(pre.gray, w, h, Math.max(2, Math.round(Math.min(w, h) / 90)));
    pre.edge = sobelMagnitude(pre.gray, w, h);
  }
  if (style === 'thermal' || style === 'halftone' || style === 'cross') {
    pre.gray = prepareGray(src, w, h);
  }
  if (style === 'marble') {
    pre.fbm = makeFbm(3157, Math.round(p.octaves));
    pre.palette = p.palette === 'auto' ? extractPalette(src, 4) : null;
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
  if (style === 'ripple' || style === 'glass' || style === 'heatwave' || style === 'pastel' || style === 'holo') {
    pre.gray3 = prepareGray(src, w, h);
    if (style === 'glass' || style === 'pastel' || style === 'holo') {
      pre.blur3 = boxBlur(pre.gray3, w, h, Math.max(2, Math.round(Math.min(w, h) / 80)));
      pre.edge3 = sobelMagnitude(pre.gray3, w, h);
    }
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
    case 'wear': return renderWear(dst, src, w, h, y0, y1, p, pre);
    case 'oil': return renderOil(dst, src, w, h, y0, y1, p);
    case 'halftone': return renderHalftone(dst, src, w, h, y0, y1, p);
    case 'sketch': return renderSketch(dst, w, h, y0, y1, p, pre);
    case 'thermal': return renderThermal(dst, w, h, y0, y1, p, pre);
    case 'neon': return renderNeon(dst, w, h, y0, y1, p, pre);
    case 'cross': return renderCross(dst, src, w, h, y0, y1, p);
    case 'marble': return renderMarble(dst, w, h, y0, y1, p, pre);
    case 'duotone': return renderDuotone(dst, w, h, y0, y1, p, pre.gray);
    case 'watercolor': return renderWatercolor(dst, src, w, h, y0, y1, p, pre);
    case 'lowpoly': return renderLowpoly(dst, src, w, h, y0, y1, p, pre);
    case 'pixel': return renderPixelArt(dst, src, w, h, y0, y1, p);
    case 'woodcut': return renderWoodcut(dst, src, w, h, y0, y1, p, pre.gray);
    case 'film': return renderFilm(dst, src, w, h, y0, y1, p, pre.gray, pre.blur || boxBlur(pre.gray, w, h, Math.max(2, Math.round(Math.min(w, h) / 80))));
    case 'ripple': return rippleImpl(dst, src, w, h, y0, y1, p, pre);
    case 'glass': return glassDispatch(dst, src, w, h, y0, y1, p, pre);
    case 'mosaic': return mosaicDispatch(dst, src, w, h, y0, y1, p);
    case 'heatwave': return heatwaveDispatch(dst, src, w, h, y0, y1, p);
    case 'pastel': return pastelDispatch(dst, src, w, h, y0, y1, p, pre);
    case 'holo': return holoDispatch(dst, src, w, h, y0, y1, p, pre);
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
      dst[i] = c[0]; dst[i + 1] = c[1]; dst[i + 2] = c[2]; dst[i + 3] = src ? src[i * 4 + 3] : 255;
    }
  }
}

// 磨损掉漆：噪声 + 边缘驱动掉漆露出底漆，划痕为随机短亮线。
function renderWear(dst, src, w, h, y0, y1, p, pre) {
  const rand = mulberry32(77);
  const scratchCount = Math.round((p.scratches / 100) * (w * h) / 26000);
  const scratches = [];
  for (let s = 0; s < scratchCount; s++) {
    scratches.push({
      x: rand() * w, y: rand() * h,
      angle: rand() * Math.PI * 2,
      len: 8 + rand() * Math.min(w, h) * 0.08,
      width: 0.6 + rand() * 1.6,
    });
  }
  const primer = [196, 178, 140];
  for (let y = y0; y < y1; y++) {
    for (let x = 0; x < w; x++) {
      const i = y * w + x;
      const wearNoise = pre.fbm(x * 0.012, y * 0.012);
      const wearMask = Math.max(0, wearNoise - 0.42) / 0.58;
      const edge = Math.min(1, pre.edge[i] / 120);
      const wear = Math.min(1, wearMask * (p.strength / 100) * 1.6 + edge * (p.edge / 100) * 0.55);
      let r = src[i * 4], g = src[i * 4 + 1], b = src[i * 4 + 2];
      if (wear > 0.12) {
        const base = p.baseColor ? primer : [r * 0.55 + 90, g * 0.55 + 80, b * 0.55 + 60];
        const k = Math.min(1, (wear - 0.12) / 0.4);
        r += (base[0] - r) * k; g += (base[1] - g) * k; b += (base[2] - b) * k;
      }
      const grain = p.grain > 0 ? (rand() - 0.5) * (p.grain / 100) * 46 : 0;
      dst[i * 4] = Math.max(0, Math.min(255, r + grain));
      dst[i * 4 + 1] = Math.max(0, Math.min(255, g + grain));
      dst[i * 4 + 2] = Math.max(0, Math.min(255, b + grain));
      dst[i * 4 + 3] = src[i * 4 + 3];
    }
  }
  for (const s of scratches) {
    const dx = Math.cos(s.angle), dy = Math.sin(s.angle);
    for (let t = -s.len / 2; t <= s.len / 2; t += 0.7) {
      const px = Math.round(s.x + dx * t), py = Math.round(s.y + dy * t);
      if (px < 0 || py < 0 || px >= w || py >= h || py < y0 || py >= y1) continue;
      const i = (py * w + px) * 4;
      const k = (1 - Math.abs(t) / (s.len / 2)) * (p.scratches / 100);
      dst[i] = Math.min(255, dst[i] + 90 * k);
      dst[i + 1] = Math.min(255, dst[i + 1] + 90 * k);
      dst[i + 2] = Math.min(255, dst[i + 2] + 82 * k);
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
function renderSketch(dst, w, h, y0, y1, p, pre) {
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
function renderThermal(dst, w, h, y0, y1, p, pre) {
  const stops = HALFTONE_LUT[p.lut] || HALFTONE_LUT.iron;
  const mix = p.mix / 100;
  const contrast = 1 + p.contrast / 60;
  for (let y = y0; y < y1; y++) {
    for (let x = 0; x < w; x++) {
      const i = (y * w + x) * 4;
      const g = Math.max(0, Math.min(255, (pre.gray[i] - 128) * contrast + 128));
      const c = lutColor(stops, g / 255);
      dst[i] = src[i] * (1 - mix) + c[0] * mix;
      dst[i + 1] = src[i + 1] * (1 - mix) + c[1] * mix;
      dst[i + 2] = src[i + 2] * (1 - mix) + c[2] * mix;
      dst[i + 3] = src[i + 3];
    }
  }
}

// 赛博霓虹：Sobel 边缘按色相着色，辉光 = 边缘图盒模糊，背景压暗。
function renderNeon(dst, src, w, h, y0, y1, p, pre) {
  const edges = pre.edge;
  const glow = boxBlur(edges, w, h, Math.max(2, Math.round(Math.min(w, h) / 120)));
  const rad = p.hue * Math.PI / 180;
  const core = [
    127.5 + 127.5 * Math.cos(rad),
    127.5 + 127.5 * Math.sin(rad - Math.PI / 3),
    127.5 + 127.5 * Math.sin(rad + Math.PI / 3),
  ];
  const dark = 1 - p.dark / 100;
  for (let y = y0; y < y1; y++) {
    for (let x = 0; x < w; x++) {
      const i = (y * w + x) * 4;
      const e = Math.min(1, edges[i / 4] / 90);
      const gl = Math.min(1, glow[i / 4] * (p.glow / 100) * 1.6);
      dst[i] = src[i] * dark * (1 - e) + core[0] * (e + gl * 0.55) + src[i] * 0.06;
      dst[i + 1] = src[i + 1] * dark * (1 - e) + core[1] * (e + gl * 0.55) + src[i + 1] * 0.06;
      dst[i + 2] = src[i + 2] * dark * (1 - e) + core[2] * (e + gl * 0.55) + src[i + 2] * 0.06;
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

// 大理石：fBm 湍流域扭曲脉络，映射到调色板（自动取色或预设）。
function renderMarble(dst, w, h, y0, y1, p, pre) {
  const scale = p.scale / 100;
  const turb = p.turbulence / 100;
  const vein = p.vein / 100;
  let palette = pre.palette;
  if (p.palette === 'blackwhite' || !palette) palette = [[10, 10, 12], [235, 235, 238]];
  if (p.palette === 'jade') palette = [[8, 60, 44], [30, 140, 100], [120, 210, 170], [220, 245, 230]];
  if (p.palette === 'amber') palette = [[40, 20, 4], [140, 80, 20], [220, 160, 70], [250, 225, 170]];
  for (let y = y0; y < y1; y++) {
    for (let x = 0; x < w; x++) {
      const i = (y * w + x) * 4;
      const base = pre.fbm(x * 0.0035 * scale, y * 0.0035 * scale);
      const warp = pre.fbm(x * 0.011 * scale + 31.7, y * 0.011 * scale - 17.3) - 0.5;
      const v = Math.abs(Math.sin((x * 0.5 + y * 0.9) * 0.02 + (base + warp * turb * 0.45) * Math.PI * 2 * (0.5 + vein)));
      const f = Math.min(0.999, Math.max(0, (1 - v) / vein * 2));
      const c = palette[Math.min(palette.length - 1, Math.floor(f * palette.length))];
      dst[i] = c[0]; dst[i + 1] = c[1]; dst[i + 2] = c[2]; dst[i + 3] = 255;
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
