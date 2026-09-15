import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';

const hasBlueDominantChannel = value => {
  const channels = [value.slice(0, 2), value.slice(2, 4), value.slice(4, 6)].map(channel => Number.parseInt(channel, 16));
  return channels[2] > channels[0] + 8 && channels[2] > channels[1] + 8;
};

const blueHexLiterals = (source, pattern) => [...source.matchAll(pattern)]
  .map(match => match[1])
  .filter(hasBlueDominantChannel);

const blueRgbLiterals = source => [...source.matchAll(/rgba?\(\s*(\d+)\s*,\s*(\d+)\s*,\s*(\d+)/gi)]
  .map(match => match.slice(1, 4).map(Number))
  .filter(([red, green, blue]) => blue > red + 8 && blue > green + 8);

test('model bake workspace inherits the application surface system', () => {
  const css = readFileSync(new URL('../src/renderer/scripts/model-bake.css', import.meta.url), 'utf8');
  for (const token of ['bg', 'bg-deep', 'surface', 'surface-raised', 'surface-hover', 'border', 'text-dim']) {
    assert.match(css, new RegExp(`var\\(--${token}\\)`));
  }
  assert.doesNotMatch(css, /backdrop-filter|radial-gradient/);
});

test('model bake workspace keeps authored colors neutral', () => {
  const css = readFileSync(new URL('../src/renderer/scripts/model-bake.css', import.meta.url), 'utf8');
  const script = readFileSync(new URL('../src/renderer/scripts/model-bake.js', import.meta.url), 'utf8');
  assert.deepEqual(blueHexLiterals(css, /#([0-9a-f]{6})\b/gi), []);
  assert.deepEqual(blueRgbLiterals(css), []);
  assert.deepEqual(blueHexLiterals(script, /#([0-9a-f]{6})\b/gi), []);
  assert.deepEqual(blueHexLiterals(script, /0x([0-9a-f]{6})\b/gi), []);
});


test('model bake viewport follows the Marmoset/Substance control scheme', () => {
  const script = readFileSync(new URL('../src/renderer/scripts/model-bake.js', import.meta.url), 'utf8');
  assert.match(script, /mouseButtons = \{ LEFT: null, MIDDLE: THREE\.MOUSE\.PAN, RIGHT: THREE\.MOUSE\.PAN \}/);
  // keyup 必须无条件恢复点选（target 守卫会让焦点进输入框后的松键漏掉，LEFT=ROTATE 粘滞）。
  assert.match(script, /event\.type === 'keyup'\) \{\s*\n\s*resetAltRotate\(\);/);
  assert.match(script, /addEventListener\('blur', resetAltRotate\)/);
  assert.match(script, /removeEventListener\('keyup', keyboard\)/);
  assert.match(script, /const standardViews = \{ 1: 'front', 3: 'side', 7: 'top' \}/);
  assert.match(script, /Alt\+左键 旋转 · 中键 平移/);
});


test('model bake results are cached locally and exported on demand', () => {
  const script = readFileSync(new URL('../src/renderer/scripts/model-bake.js', import.meta.url), 'utf8');
  const state = readFileSync(new URL('../src/renderer/scripts/model-bake-state.mjs', import.meta.url), 'utf8');
  assert.doesNotMatch(script, /请选择输出目录/);
  assert.doesNotMatch(state, /output:/);
  assert.match(script, /resultHandle \? \{ resultHandle, directory \} : \{ files: results\.map\(file => file\.path\), directory \}/);
  assert.match(script, /invoke\('bake_result_release'/);
  assert.match(script, /querySelectorAll\('\[data-bake-export\]'\)/);
  assert.match(script, /结果先缓存在应用数据目录，导出时选择目标文件夹/);
  assert.match(script, /打开缓存目录/);
});

test('model bake lazily decodes result images and debounces uv inspection', () => {
  const script = readFileSync(new URL('../src/renderer/scripts/model-bake.js', import.meta.url), 'utf8');
  assert.match(script, /image\.loading = 'lazy'/);
  assert.match(script, /image\.decoding = 'async'/);
  assert.match(script, /function scheduleRefreshReports\(\)/);
  assert.match(script, /refreshReports\(\); \}, 300\)/);
  assert.match(script, /lastReportSignature/);
  assert.match(script, /reportInFlight/);
});

test('model bake toggles mesh visibility instead of rebuilding meshes on selection change', () => {
  const script = readFileSync(new URL('../src/renderer/scripts/model-bake.js', import.meta.url), 'utf8');
  assert.match(script, /function syncMeshVisibility\(\)/);
  // SP 式按材质工作流：对象不参与任何筛选，可见性只看材质勾选。
  assert.match(script, /mesh\.visible = materials\.has\(mesh\.userData\.material\)/);
  assert.match(script, /const objectBounds = new Map\(\)/);
  assert.match(script, /objectBounds\.clear\(\)/);
  assert.match(script, /function selectionBox\(\)/);
  assert.match(script, /refreshMaterialUv\(focused\)/);
  // 对象列表与对象选择状态整体移除
  assert.doesNotMatch(script, /参与烘焙的对象/);
  assert.doesNotMatch(script, /id="bake-objects"/);
  assert.doesNotMatch(script, /objects = new Set\(/);
  // 整表构建只允许导入路径触发：定义 1 处 + 调用 1 处；勾选/通道变化不得再重建。
  assert.match(script, /prepared = await buildMeshes\(revision, data, nextGeometry, nextChannels\)/);
});

test('model bake decodes a compact transferable preview instead of parsing model JSON', () => {
  const script = readFileSync(new URL('../src/renderer/scripts/model-bake.js', import.meta.url), 'utf8');
  const worker = readFileSync(new URL('../src/renderer/scripts/model-bake-preview-worker.js', import.meta.url), 'utf8');
  assert.doesNotMatch(script, /fetch\(convertFileSrc\(data\.meshPath\)\)/);
  assert.doesNotMatch(script, /response\.json\(\)/);
  assert.match(script, /new Worker\(new URL\('\.\/model-bake-preview-worker\.js'/);
  assert.match(script, /new Float32Array\(geometry\.buffer/);
  assert.match(script, /performance\.now\(\) - sliceStarted >= 9/);
  assert.match(worker, /postMessage\(\{ id, buffer \}, \[buffer\]\)/);
});

test('model replacement commits only after the new preview is ready', () => {
  const script = readFileSync(new URL('../src/renderer/scripts/model-bake.js', import.meta.url), 'utf8');
  const prepared = script.indexOf('prepared = await buildMeshes(revision, data, nextGeometry, nextChannels)');
  const releasePrevious = script.indexOf("invoke('bake_release', { handle: previousModel.handle })");
  assert.ok(prepared >= 0 && releasePrevious > prepared);
  assert.match(script, /if \(prepared\) disposePreparedMeshes\(prepared\.meshes\)/);
});

test('model bake export locks duplicate clicks while allowing an old result during rebake', () => {
  const script = readFileSync(new URL('../src/renderer/scripts/model-bake.js', import.meta.url), 'utf8');
  assert.match(script, /if \(!desktop \|\| !results\.length \|\| exporting\) return/);
  assert.match(script, /setActionBusy\(button, true\)/);
  assert.match(script, /上次结果 ·/);
});

test('model bake produces smart-material mesh maps and applies results to the model preview', () => {
  const script = readFileSync(new URL('../src/renderer/scripts/model-bake.js', import.meta.url), 'utf8');
  const state = readFileSync(new URL('../src/renderer/scripts/model-bake-state.mjs', import.meta.url), 'utf8');
  for (const kind of ['ao', 'normal', 'worldNormal', 'curvature', 'position', 'thickness', 'id']) {
    assert.match(state, new RegExp(`${kind}: true`));
  }
  assert.match(script, /async function applyMapPreview/);
  assert.match(script, /if \(view === 'model'\) await applyMapPreview\(defaultPreview, false, false\)/);
  assert.match(script, /apply\.textContent = '在模型上预览'/);
  assert.match(script, /new THREE\.MeshBasicMaterial\(\{ map: texture/);
  assert.match(script, /imageOrientation: 'flipY'/);
});

test('model bake keeps the current view and exposes live structured stages', () => {
  const script = readFileSync(new URL('../src/renderer/scripts/model-bake.js', import.meta.url), 'utf8');
  const css = readFileSync(new URL('../src/renderer/scripts/model-bake.css', import.meta.url), 'utf8');
  assert.match(script, /id="bake-live-progress"/);
  assert.match(script, /function updateBakeProgress\(data = \{\}\)/);
  assert.match(script, /Math\.max\(lastBakeProgress, raw\)/);
  assert.match(script, /data\.materialPosition/);
  assert.match(script, /data\.mapPosition/);
  assert.match(script, /if \(switchView\) setView\('model'\)/);
  assert.doesNotMatch(script, /setView\('results'\)/);
  assert.match(css, /\.bake-live-stages span\.active/);
});
