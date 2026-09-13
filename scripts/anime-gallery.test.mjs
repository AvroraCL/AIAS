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
    renderAnimeGallery() {},
  });
  vm.runInContext(gallery, context);
  const flush = () => new Promise(resolve => setTimeout(resolve, 0));
  return { context, state, filesExistCalls, resolvers, flush, controls };
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
