import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import vm from 'node:vm';
import { Image as ImageIcon } from 'lucide';

// Run the actual gallery functions with the same icon binding as app.js and a
// controllable files_exist backend; no ONNX or desktop bridge is needed.
const source = readFileSync(new URL('../src/renderer/scripts/app.js', import.meta.url), 'utf8');
const gallery = source.slice(source.indexOf('function animeStem('), source.indexOf('function renderAnimeGallery('));
function fixture() {
  const filesExistCalls = [];
  const resolvers = [];
  const controls = { 'anime-model': { value: 'anime-specialist' }, 'anime-output': { value: 'C:/output' } };
  const state = { animeFiles: ['C:/input/hero.png', 'C:/input/hero_pose.png'], animeResults: new Map(), animeProbed: new Set() };
  const calls = { rebuilds: 0, compareRefreshes: 0 };
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
    wantsDetailRecovery: () => false, wantsHairRefiner: () => false,
    // 探测命中应走增量更新：全量重建与对比图刷新只计数，供断言使用
    renderAnimeGallery() { calls.rebuilds += 1; },
    renderAnimeCompare() { calls.compareRefreshes += 1; },
    refreshIcons() {},
  });
  vm.runInContext(gallery, context);
  const flush = () => new Promise(resolve => setTimeout(resolve, 0));
  return { context, state, filesExistCalls, resolvers, flush, controls, calls };
}

test('probe stores the first candidate reported existing by files_exist', async () => {
  const { context, state, filesExistCalls, resolvers, flush } = fixture();
  context.probeAnimeResult('C:/input/hero.png');
  assert.equal(filesExistCalls.length, 1);
  assert.deepEqual(filesExistCalls[0], ['C:/output/hero_anime-specialist.png']);
  resolvers[0].resolve([true]);
  await flush();
  assert.equal(state.animeResults.get('hero_anime-specialist'), 'C:/output/hero_anime-specialist.png');
});

test('a probe hit flips the card badge in place instead of rebuilding the gallery', async () => {
  const { context, state, resolvers, flush, controls, calls } = fixture();
  const badge = { className: 'thumb-badge pending', title: '待处理', innerHTML: '' };
  const card = {
    dataset: { path: 'C:/input/hero.png' },
    title: 'C:/input/hero.png',
    querySelector: selector => (selector === '.thumb-badge' ? badge : null),
  };
  controls['anime-thumbs'] = { children: [card] };
  context.probeAnimeResult('C:/input/hero.png');
  resolvers[0].resolve([true]);
  await flush();
  assert.equal(state.animeResults.get('hero_anime-specialist'), 'C:/output/hero_anime-specialist.png');
  assert.equal(badge.className, 'thumb-badge done');
  assert.equal(badge.title, '已有抠图结果');
  assert.match(badge.innerHTML, /data-lucide="check"/);
  assert.equal(calls.rebuilds, 0);
});

test('a probe hit without a rendered card skips the incremental update entirely', async () => {
  const { context, state, resolvers, flush, calls } = fixture();
  context.probeAnimeResult('C:/input/hero.png');
  resolvers[0].resolve([true]);
  await flush();
  assert.equal(state.animeResults.size, 1);
  assert.equal(calls.rebuilds, 0);
  assert.equal(calls.compareRefreshes, 0);
});

test('only a probe hit on the active item refreshes the compare view', async () => {
  const { context, state, resolvers, flush, calls } = fixture();
  state.animeActiveIndex = 1; // 活动项是 hero_pose，先命中 hero 不应刷新对比图
  context.probeAnimeResult('C:/input/hero.png');
  resolvers[0].resolve([true]);
  await flush();
  assert.equal(calls.compareRefreshes, 0);
  context.probeAnimeResult('C:/input/hero_pose.png');
  resolvers[1].resolve([true]);
  await flush();
  assert.equal(calls.compareRefreshes, 1);
  assert.equal(calls.rebuilds, 0);
});

test('similar file prefixes map to the exact input, including model fallback', () => {
  const { context, state } = fixture();
  context.applyAnimeOutputs(['C:/output/hero_pose_simple.png']);
  assert.equal(state.animeResults.get('hero_pose_anime-specialist'), 'C:/output/hero_pose_simple.png');
  assert.equal(state.animeResults.has('hero_anime-specialist'), false);
});

test('completed outputs keep the model key captured when the task started', () => {
  const { context, state, controls } = fixture();
  const keys = new Map(state.animeFiles.map(file => [file, context.animeResultKey(file)]));
  controls['anime-model'].value = 'simple';
  context.applyAnimeOutputs(['C:/output/hero_anime-specialist.png'], keys);
  assert.equal(state.animeResults.has('hero_anime-specialist'), true);
  assert.equal(state.animeResults.has('hero_simple'), false);
});

test('toonout fallback candidates are probed in one batch and the first hit wins', async () => {
  const { context, state, filesExistCalls, resolvers, controls, flush } = fixture();
  controls['anime-model'].value = 'toonout';
  context.probeAnimeResult('C:/input/hero.png');
  assert.deepEqual(filesExistCalls[0], [
    'C:/output/hero_toonout.png',
    'C:/output/hero_anime-specialist.png',
    'C:/output/hero_birefnet-general.png',
    'C:/output/hero_advanced.png',
    'C:/output/hero_simple.png'
  ]);
  resolvers[0].resolve([false, false, true, false, false]);
  await flush();
  assert.equal(state.animeResults.get('hero_toonout'), 'C:/output/hero_birefnet-general.png');
});

test('late probes cannot restore a result from a previous output directory', async () => {
  const { context, state, resolvers, controls, flush } = fixture();
  context.probeAnimeResult('C:/input/hero.png');
  controls['anime-output'].value = 'C:/other-output';
  context.resetAnimeResults();
  resolvers[0].resolve([true]);
  await flush();
  assert.equal(state.animeResults.size, 0);
});

test('a probe without existing candidates stays marked and is not repeated', async () => {
  const { context, filesExistCalls, resolvers, flush } = fixture();
  context.probeAnimeResult('C:/input/hero.png');
  resolvers[0].resolve([false]);
  await flush();
  context.probeAnimeResult('C:/input/hero.png');
  assert.equal(filesExistCalls.length, 1);
});

test('a rejected probe call unmarks the key so a later render can retry', async () => {
  const { context, filesExistCalls, resolvers, flush } = fixture();
  context.probeAnimeResult('C:/input/hero.png');
  resolvers[0].reject(new Error('backend busy'));
  await flush();
  context.probeAnimeResult('C:/input/hero.png');
  assert.equal(filesExistCalls.length, 2);
});
