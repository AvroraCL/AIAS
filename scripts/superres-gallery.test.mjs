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
  const filesExistCalls = [];
  const resolvers = [];
  const controls = {
    'superres-scale': { value: scale },
    'superres-scale-slider': { style: { setProperty() {} } },
    'superres-scale-text': {},
    'superres-output': { value: 'C:/output' }
  };
  const state = { superresFiles: ['C:/input/hero.png'], superresResults: new Map(), superresProbed: new Set() };
  const calls = { rebuilds: 0 };
  const context = vm.createContext({
    state, Image: ImageIcon,
    api: {
      // Deferred backend: probes stay pending until the test resolves them,
      // so stale callbacks can be flushed in any order.
      filesExist: (paths) => {
        filesExistCalls.push([...paths]); // spread：把 VM realm 的数组拷回宿主 realm，供 deepEqual 比较
        return new Promise((resolve, reject) => resolvers.push({ resolve, reject }));
      }
    },
    basename: path => path.split(/[\\/]/).pop(),
    $: id => controls[id], isTauriRuntime: true, convertFileSrc: path => path,
    setText: (id, value) => { controls[id].textContent = value; },
    // 探测命中应走增量更新：全量重建只计数，供断言使用
    renderSuperresGallery() { calls.rebuilds += 1; },
    refreshIcons() {}
  });
  vm.runInContext(helpers, context);
  const flush = () => new Promise(resolve => setTimeout(resolve, 0));
  return { context, state, filesExistCalls, resolvers, flush, controls, file: state.superresFiles[0], calls };
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
  const { context, filesExistCalls } = fixture({ scale: '2' });
  context.probeSuperresResult('C:/input/hero.png', 'anime');
  assert.equal(filesExistCalls.length, 1);
  assert.deepEqual(filesExistCalls[0], ['C:/output/hero_2x_anime.png']);
});

test('an existing result file is stored under its key', async () => {
  const { context, state, resolvers, flush, file } = fixture();
  context.probeSuperresResult(file, 'anime');
  resolvers[0].resolve([true]);
  await flush();
  assert.equal(state.superresResults.get(context.superresResultKey(file, 'anime')), 'C:/output/hero_4x_anime.png');
});

test('a probe hit flips the model badge in place without rebuilding the grid', async () => {
  const { context, state, resolvers, flush, controls, file, calls } = fixture();
  const badge = { className: 'thumb-badge pending', title: '待处理', textContent: '' };
  const img = { title: '原图 · 当前模型和倍率尚无结果' };
  const card = {
    dataset: { path: file },
    querySelector: selector => {
      if (selector === '.thumb-badge[data-model="anime"]') return badge;
      if (selector === 'img') return img;
      return null;
    }
  };
  controls['superres-grid'] = { children: [card] };
  context.probeSuperresResult(file, 'anime');
  resolvers[0].resolve([true]);
  await flush();
  assert.equal(state.superresResults.get(context.superresResultKey(file, 'anime')), 'C:/output/hero_4x_anime.png');
  assert.equal(badge.className, 'thumb-badge done');
  assert.equal(badge.title, '已生成 4x 结果');
  assert.equal(img.title, '4x 动漫超分结果');
  assert.equal(calls.rebuilds, 0);
});

test('a probe hit without a rendered card skips the incremental update entirely', async () => {
  const { context, state, resolvers, flush, file, calls } = fixture();
  context.probeSuperresResult(file, 'anime');
  resolvers[0].resolve([true]);
  await flush();
  assert.equal(state.superresResults.size, 1);
  assert.equal(calls.rebuilds, 0);
});

test('missing results unmark the probe so later renders can retry', async () => {
  const { context, state, filesExistCalls, resolvers, flush, file } = fixture();
  context.probeSuperresResult(file, 'anime');
  resolvers[0].resolve([false]);
  await flush();
  assert.equal(state.superresProbed.has(context.superresResultKey(file, 'anime')), false);
  context.probeSuperresResult(file, 'anime');
  assert.equal(filesExistCalls.length, 2);
});

test('late superres probes cannot restore old directory results', async () => {
  const { context, state, controls, resolvers, flush, file } = fixture();
  context.probeSuperresResult(file, 'anime');
  controls['superres-output'].value = 'C:/other';
  context.resetSuperresResults();
  resolvers[0].resolve([true]);
  await flush();
  assert.equal(state.superresResults.size, 0);
});
test('empty directory does not prevent a later result probe', () => {
  const { context, controls, filesExistCalls, file } = fixture();
  controls['superres-output'].value = '';
  context.probeSuperresResult(file, 'anime');
  controls['superres-output'].value = 'C:/output';
  context.probeSuperresResult(file, 'anime');
  assert.equal(filesExistCalls.length, 1);
});
test('late probes preserve newer completed outputs', async () => {
  const { context, state, resolvers, flush, file } = fixture();
  context.probeSuperresResult(file, 'anime');
  const key = context.superresResultKey(file, 'anime');
  state.superresResults.set(key, 'new-result');
  resolvers[0].resolve([true]);
  await flush();
  assert.equal(state.superresResults.get(key), 'new-result');
});
test('output keys preserve captured scale after changing slider', () => {
  const { context, state, controls, file } = fixture();
  const key = context.superresResultKey(file, 'anime');
  controls['superres-scale'].value = '2';
  context.applySuperresOutputs(['C:/output/hero_4x_anime.png'], new Map([[file, key]]));
  assert.equal(state.superresResults.get(key), 'C:/output/hero_4x_anime.png');
  assert.equal(state.superresResults.has(context.superresResultKey(file, 'anime')), false);
});

test('directory round trip cannot revive a probe from an earlier revision', async () => {
  const { context, state, controls, resolvers, flush, file } = fixture();
  context.probeSuperresResult(file, 'anime');
  controls['superres-output'].value = 'C:/other';
  context.resetSuperresResults();
  controls['superres-output'].value = 'C:/output';
  context.resetSuperresResults();
  context.probeSuperresResult(file, 'anime');
  resolvers[0].resolve([true]);
  await flush();
  assert.equal(state.superresResults.size, 0);
  resolvers[1].resolve([true]);
  await flush();
  assert.equal(state.superresResults.size, 1);
});

test('old failed probes cannot clear the new in-flight probe marker', async () => {
  const { context, state, resolvers, flush, file } = fixture();
  context.probeSuperresResult(file, 'anime');
  context.resetSuperresResults();
  context.probeSuperresResult(file, 'anime');
  resolvers[0].reject(new Error('stale probe'));
  await flush();
  assert.equal(state.superresProbed.has(context.superresResultKey(file, 'anime')), true);
});

test('remove and re-add invalidates previous callbacks and all cached scales', async () => {
  const { context, state, resolvers, flush, file } = fixture();
  context.updateStatus = () => {};
  vm.runInContext(source.slice(source.indexOf('function removeSuperresFile('), source.indexOf('async function runSuperres(')), context);
  context.probeSuperresResult(file, 'anime');
  state.superresResults.set(context.superresResultKey(file, 'anime', 2), 'old');
  context.removeSuperresFile(file);
  state.superresFiles.push(file);
  resolvers[0].resolve([true]);
  await flush();
  assert.equal(state.superresResults.size, 0);
  assert.equal(state.superresProbed.size, 0);
});
