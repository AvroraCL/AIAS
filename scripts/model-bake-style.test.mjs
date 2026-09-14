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
  assert.match(script, /controls\.mouseButtons\.LEFT = event\.type === 'keydown' \? THREE\.MOUSE\.ROTATE : null/);
  assert.match(script, /removeEventListener\('keyup', keyboard\)/);
  assert.match(script, /const standardViews = \{ 1: 'front', 3: 'side', 7: 'top' \}/);
  assert.match(script, /Alt\+左键 旋转 · 中键 平移/);
});


test('model bake results are cached locally and exported on demand', () => {
  const script = readFileSync(new URL('../src/renderer/scripts/model-bake.js', import.meta.url), 'utf8');
  const state = readFileSync(new URL('../src/renderer/scripts/model-bake-state.mjs', import.meta.url), 'utf8');
  assert.doesNotMatch(script, /请选择输出目录/);
  assert.doesNotMatch(state, /output:/);
  assert.match(script, /invoke\('bake_export', \{ files: results\.map\(file => file\.path\), directory \}\)/);
  assert.match(script, /querySelectorAll\('\[data-bake-export\]'\)/);
  assert.match(script, /结果先缓存在应用数据目录，导出时选择目标文件夹/);
  assert.match(script, /打开缓存目录/);
});
