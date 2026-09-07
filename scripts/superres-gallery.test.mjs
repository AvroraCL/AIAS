import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import vm from 'node:vm';
import { Image as ImageIcon } from 'lucide';

// Run the actual superres helpers with a controllable slider and output
// directory; no ONNX or desktop bridge is needed.
const source = readFileSync(new URL('../src/renderer/scripts/app.js', import.meta.url), 'utf8');
const helpers = source.slice(source.indexOf('function animeStem('), source.indexOf('function renderSuperresGallery('));
function fixture({ scale = '4' } = {}) {
  const probes = [];
  const controls = {
    'superres-scale': { value: scale },
    'superres-scale-slider': { style: { setProperty() {} } },
    'superres-scale-text': {},
    'superres-output': { value: 'C:/output' }
  };
  const state = { superresFiles: ['C:/input/hero.png'], superresResults: new Map(), superresProbed: new Set() };
  const context = vm.createContext({
    state, Image: ImageIcon,
    window: { Image: class { constructor() { probes.push(this); } } },
    basename: path => path.split(/[\\/]/).pop(),
    $: id => controls[id], isTauriRuntime: true, convertFileSrc: path => path,
    setText: (id, value) => { controls[id].textContent = value; },
    renderSuperresGallery() {}
  });
  vm.runInContext(helpers, context);
  return { context, state, probes, controls };
}

test('slider label tracks the selected scale between 2 and 4', () => {
  const { context, controls } = fixture({ scale: '3' });
  context.updateSuperresScaleControl();
  assert.equal(controls['superres-scale-text'].textContent, '放大 3 倍');
  assert.equal(context.superresScale(), 3);
});

test('thumb position variable maps scale 2/3/4 to 0/0.5/1', () => {
  const { context, controls } = fixture();
  const pos = [];
  controls['superres-scale-slider'].style.setProperty = (k, v) => { if (k === '--pos') pos.push(v); };
  for (const s of ['2', '3', '4']) {
    controls['superres-scale'].value = s;
    context.updateSuperresScaleControl();
  }
  assert.deepEqual(pos, ['0', '0.5', '1']);
});

test('a label change retriggers the pop animation class', () => {
  const { context, controls } = fixture();
  // 真实 DOM 中 HTML 已渲染默认文字
  controls['superres-scale-text'].textContent = '放大 4 倍';
  const applied = [];
  controls['superres-scale-text'].classList = {
    remove: () => applied.push('remove'),
    add: cls => applied.push('add:' + cls)
  };
  context.updateSuperresScaleControl();
  // 值未变化时无需重触发
  assert.equal(applied.length, 0);
  controls['superres-scale'].value = '2';
  context.updateSuperresScaleControl();
  assert.deepEqual(applied, ['remove', 'add:scale-pop']);
});

test('out-of-range slider values are clamped to the 2-4 window', () => {
  const { context, controls } = fixture({ scale: '9' });
  context.updateSuperresScaleControl();
  assert.equal(context.superresScale(), 4);
  assert.equal(controls['superres-scale'].value, '4');
  assert.equal(controls['superres-scale-text'].textContent, '放大 4 倍');
});

test('outputs with an explicit scale map back to the scale captured at run start', () => {
  const { context, state } = fixture();
  const requestKeys = new Map([['C:/input/hero.png', context.superresResultKey('C:/input/hero.png', 'anime', 3)]]);
  context.applySuperresOutputs(['C:/output/hero_3x_anime.png'], requestKeys);
  assert.equal(state.superresResults.get(context.superresResultKey('C:/input/hero.png', 'anime', 3)), 'C:/output/hero_3x_anime.png');
  // 当前倍率(4)下该结果不应误标为已完成。
  assert.equal(state.superresResults.has(context.superresResultKey('C:/input/hero.png', 'anime', 4)), false);
});

test('legacy-style 4x outputs still map without run-start keys', () => {
  const { context, state } = fixture();
  context.applySuperresOutputs(['C:/output/hero_4x_general.png']);
  assert.equal(state.superresResults.get(context.superresResultKey('C:/input/hero.png', 'general', 4)), 'C:/output/hero_4x_general.png');
});

test('result probes look for the file named after the current scale', () => {
  const { context, probes } = fixture({ scale: '2' });
  context.probeSuperresResult('C:/input/hero.png', 'anime');
  assert.equal(probes.length, 1);
  assert.equal(probes[0].src, 'C:/output/hero_2x_anime.png');
});
