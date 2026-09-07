import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import vm from 'node:vm';
import { Image as ImageIcon } from 'lucide';

// Run the actual gallery functions with the same icon binding as app.js and a
// controllable browser image loader; no ONNX or desktop bridge is needed.
const source = readFileSync(new URL('../src/renderer/scripts/app.js', import.meta.url), 'utf8');
const gallery = source.slice(source.indexOf('function animeStem('), source.indexOf('function renderAnimeGallery('));
function fixture() {
  const probes = [];
  const controls = { 'anime-model': { value: 'anime-specialist' }, 'anime-output': { value: 'C:/output' } };
  const state = { animeFiles: ['C:/input/hero.png', 'C:/input/hero_pose.png'], animeResults: new Map(), animeProbed: new Set() };
  const context = vm.createContext({
    state, Image: ImageIcon,
    window: { Image: class { constructor() { probes.push(this); } } },
    basename: path => path.split(/[\\/]/).pop(),
    $: id => controls[id], isTauriRuntime: true, convertFileSrc: path => path,
    wantsDetailRecovery: () => false, wantsHairRefiner: () => false,
    renderAnimeGallery() {},
  });
  vm.runInContext(gallery, context);
  return { context, state, probes, controls };
}

test('existing results use a browser image loader despite the Image icon import', () => {
  const { context, probes } = fixture();
  context.probeAnimeResult('C:/input/hero.png');
  assert.equal(probes.length, 1);
  assert.equal(probes[0].src, 'C:/output/hero_anime-specialist.png');
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

test('late probes cannot restore a result from a previous output directory', () => {
  const { context, state, probes, controls } = fixture();
  context.probeAnimeResult('C:/input/hero.png');
  controls['anime-output'].value = 'C:/other-output';
  context.resetAnimeResults();
  probes[0].onload();
  assert.equal(state.animeResults.size, 0);
});
