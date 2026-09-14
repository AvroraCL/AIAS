import { test } from 'node:test';
import assert from 'node:assert/strict';
import { DEFAULTS, restoreAsciiSettings, characterRamp, gridSize, convertAscii, detectTransparency } from '../src/renderer/scripts/ascii-state.mjs';
const pixel = (rgba, settings = {}) => convertAscii({pixels: rgba, columns: rgba.length/4, rows:1, settings:{...DEFAULTS,...settings}});
test('black, white and grayscale map to sparse through dense characters', () => {
  assert.equal(pixel([0,0,0,255, 128,128,128,255, 255,255,255,255]).text, ' +@');
  assert.equal(pixel([0,0,0,255,255,255,255,255], {invert:true}).text, '@ ');
  assert.equal(pixel([0,0,0,255,255,255,255,255], {background:'white'}).text, '@ ');
});
test('transparent pixels composite onto chosen background and retain color', () => {
  assert.equal(pixel([255,0,0,0]).text, ' ');
  assert.equal(pixel([255,0,0,0], {background:'white'}).text, ' ');
  assert.deepEqual([...pixel([255,0,0,128]).colors], [128,0,0]);
  assert.deepEqual([...pixel([23,45,67,255], {color:true}).colors], [23,45,67]);
});
test('TXT preserves blank cells, rows and custom character ordering', () => {
  const result = convertAscii({pixels:[0,0,0,255,255,255,255,255,255,255,255,255,0,0,0,255],columns:2,rows:2,settings:{...DEFAULTS,charset:'custom',custom:' x'}});
  assert.equal(result.text, ' x\nx ');
  assert.throws(() => characterRamp({...DEFAULTS,charset:'custom',custom:'中文字'}));
  assert.throws(() => characterRamp({...DEFAULTS,charset:'custom',custom:'  '}));
});
test('grid respects font aspect ratio and caps long images', () => {
  assert.deepEqual(gridSize(100,100,120,9,18),{columns:120,rows:60});
  assert.deepEqual(gridSize(100,200,120,9,18),{columns:120,rows:120});
  const tall = gridSize(10,1000,120,9,18);
  assert.deepEqual(tall,{columns:8,rows:400});
  assert.equal(gridSize(10000,10,120,9,18).rows,1);
});
test('settings restore old and corrupt values safely', () => {
  assert.deepEqual(restoreAsciiSettings(null),DEFAULTS);
  assert.deepEqual(restoreAsciiSettings({columns:Infinity,contrast:-3,format:'html'}),DEFAULTS);
  assert.equal(restoreAsciiSettings({color:true,columns:240}).columns,240);
});
test('PNG becomes the default while new explicit TXT preferences are preserved', () => {
  assert.equal(DEFAULTS.format, 'png');
  assert.equal(restoreAsciiSettings({format:'txt'}).format, 'png');
  assert.equal(restoreAsciiSettings({version:2,format:'txt'}).format, 'txt');
});
test('transparent output keeps source RGB and alpha without a black matte', () => {
  const result = pixel([255,80,20,128,255,255,255,0], {background:'transparent',color:true,invert:true});
  assert.deepEqual([...result.colors], [255,80,20,255,255,255]);
  assert.deepEqual([...result.alphas], [128,0]);
  assert.equal(result.chars[1], ' ');
});

test('style setting restores only known values and keeps versioned format', () => {
  assert.equal(restoreAsciiSettings({version:2, style:'block'}).style, 'block');
  assert.equal(restoreAsciiSettings({version:2, style:'dot'}).style, 'dot');
  assert.equal(restoreAsciiSettings({version:2, style:'halftone'}).style, 'ascii');
  assert.equal(restoreAsciiSettings(null).style, 'ascii');
});

test('lights expose the final per-cell ink level driving block and dot sizes', () => {
  const plain = pixel([0,0,0,255, 255,255,255,255, 128,128,128,255]);
  assert.deepEqual([...plain.lights], [0, 255, Math.round((128/255) * 255 / 2 * 2)]);
  assert.deepEqual([...pixel([0,0,0,255,255,255,255,255], {invert:true}).lights], [255, 0]);
  assert.deepEqual([...pixel([0,0,0,255,255,255,255,255], {background:'white'}).lights], [255, 0]);
});

test('graphic styles skip character ramp validation and blank the text', () => {
  const settings = {...DEFAULTS, style:'dot', charset:'custom', custom:'  '};
  const result = convertAscii({pixels:[255,255,255,255], columns:1, rows:1, settings});
  assert.equal(result.text, ' ');
  assert.equal(result.lights[0], 255);
});

test('transparency detection samples decoded pixels and ignores opaque sources', () => {
  
  assert.equal(detectTransparency(new Uint8ClampedArray([10,20,30,255, 40,50,60,255])), false);
  assert.equal(detectTransparency(new Uint8ClampedArray([10,20,30,255, 40,50,60,128])), true);
  assert.equal(detectTransparency(new Uint8ClampedArray([10,20,30,0])), true);
  assert.equal(detectTransparency(new Uint8ClampedArray([10,20,30])), false);
  assert.equal(detectTransparency(undefined), false);
});

test('shape size and ratio persist with bounds for block and dot styles', () => {
  assert.equal(DEFAULTS.shapeSize, 92);
  assert.equal(DEFAULTS.shapeRatio, 100);
  const kept = restoreAsciiSettings({version:2, style:'dot', shapeSize:35, shapeRatio:180});
  assert.equal(kept.shapeSize, 35);
  assert.equal(kept.shapeRatio, 180);
  assert.equal(restoreAsciiSettings({version:2, shapeSize:300}).shapeSize, DEFAULTS.shapeSize);
  assert.equal(restoreAsciiSettings({version:2, shapeSize:'x'}).shapeSize, DEFAULTS.shapeSize);
  assert.equal(restoreAsciiSettings({version:2, shapeRatio:10}).shapeRatio, DEFAULTS.shapeRatio);
});

test('dither settings restore only known values', () => {
  assert.equal(DEFAULTS.dither, 'none');
  assert.equal(restoreAsciiSettings({version:2, dither:'ordered'}).dither, 'ordered');
  assert.equal(restoreAsciiSettings({version:2, dither:'diffusion'}).dither, 'diffusion');
  assert.equal(restoreAsciiSettings({version:2, dither:'noise'}).dither, 'none');
  assert.equal(restoreAsciiSettings({version:2, style:'hatch'}).style, 'hatch');
  assert.equal(restoreAsciiSettings({version:2, hatchAngle:90, hatchRounds:2}).hatchRounds, 2);
  assert.equal(restoreAsciiSettings({version:2, hatchAngle:999}).hatchAngle, 45);
  assert.equal(restoreAsciiSettings({version:2, hatchRounds:0}).hatchRounds, 3);
});

test('ordered dithering breaks a flat 50% gray band into mixed levels', () => {
  // 4×4 全 128 灰：无抖动时 16 格全同字符；有序抖动应出现两种以上档位。
  const pixels = new Array(16 * 4).fill(0).map((_, i) => i % 4 === 3 ? 255 : 128);
  const plain = convertAscii({pixels, columns: 4, rows: 4, settings: {...DEFAULTS}});
  const ordered = convertAscii({pixels, columns: 4, rows: 4, settings: {...DEFAULTS, dither: 'ordered'}});
  assert.equal(new Set(plain.chars).size, 1, '无抖动时平场应只有一种字符');
  assert.ok(new Set(ordered.chars).size >= 2, '有序抖动应产生至少两种字符档位');
  // 均值近似保持（抖动不改变整体明暗）。
  const mean = text => [...text].reduce((sum, ch) => sum + (' .:-=+*#%@'.indexOf(ch)), 0) / text.length;
  assert.ok(Math.abs(mean(plain.text) - mean(ordered.text)) < 1, '抖动应保持整体明暗水平');
});

test('diffusion dithering spreads error across a horizontal ramp', () => {
  // 16×1 从黑到白的平滑渐变：无抖动时量化步长内多格同字符；扩散应更细腻。
  const pixels = [];
  for (let x = 0; x < 16; x++) pixels.push(Math.round(x / 15 * 255), Math.round(x / 15 * 255), Math.round(x / 15 * 255), 255);
  const plain = convertAscii({pixels, columns: 16, rows: 1, settings: {...DEFAULTS}});
  const diffusion = convertAscii({pixels, columns: 16, rows: 1, settings: {...DEFAULTS, dither: 'diffusion'}});
  const rampIndex = text => [...text].map(ch => ' .:-=+*#%@'.indexOf(ch));
  const diffusionIndices = rampIndex(diffusion.text);
  assert.ok(diffusionIndices.every((v, i, arr) => i === 0 || v >= arr[i-1] - 0), '扩散输出应保持单调不减的趋势');
  assert.ok(diffusionIndices.at(-1) > diffusionIndices[0], '渐变两端应有明暗差异');
  assert.equal(convertAscii({pixels, columns: 16, rows: 1, settings: {...DEFAULTS, dither: 'none'}}).chars, plain.chars);
});

test('hatch style skips ramp validation and keeps continuous lights like other graphics', () => {
  const settings = {...DEFAULTS, style: 'hatch', charset: 'custom', custom: '  '};
  const result = convertAscii({pixels: [0,0,0,255, 255,255,255,255], columns: 2, rows: 1, settings});
  assert.equal(result.text.trim(), '', '款描风格不产出字符文本');
  assert.deepEqual([...result.lights], [0, 255]);
  assert.equal(restoreAsciiSettings({version:2, style:'hatch'}).style, 'hatch');
});

test('stage wires wheel zoom and drag pan with pointer capture', async () => {
  const { readFileSync } = await import('node:fs');
  const script = readFileSync(new URL('../src/renderer/scripts/ascii.js', import.meta.url), 'utf8');
  // 滚轮缩放必须非 passive 才能 preventDefault；缩放边界与滑块一致。
  assert.match(script, /addEventListener\('wheel', event => \{/);
  assert.match(script, /\{ passive: false \}/);
  assert.match(script, /const ZOOM_MIN = 0\.25, ZOOM_MAX = 3/);
  // 拖拽平移走 pointer capture，且拖拽态有独立类名供 CSS 切换光标。
  assert.match(script, /addEventListener\('pointerdown', event => \{/);
  assert.match(script, /setPointerCapture\(event\.pointerId\)/);
  assert.match(script, /classList\.add\('ascii-dragging'\)/);
  assert.match(script, /scrollLeft = drag\.left - \(event\.clientX - drag\.x\)/);
});
