import test from 'node:test';
import assert from 'node:assert/strict';
import { restoreBakeSettings, selectReliablePreviewFiles, assessUvCoverage } from '../src/renderer/scripts/model-bake-state.mjs';
test('bake settings restore defaults without model handles and reject invalid quality', () => {
  assert.equal(restoreBakeSettings(null).resolution, 2048);
  assert.equal(restoreBakeSettings(null).uvMode, 'preserveValid');
  assert.equal(restoreBakeSettings(null).deviceLuid, '');
  assert.equal(restoreBakeSettings(null).distanceRatio, 0.1);
  assert.equal(restoreBakeSettings(null).aoDistanceRatio, 0.01);
  for (const key of ['ao', 'normal', 'worldNormal', 'curvature', 'position', 'thickness', 'id']) assert.equal(restoreBakeSettings(null)[key], true);
  assert.equal(restoreBakeSettings(null).uv, false);
  const value = restoreBakeSettings({ handle: 'expired', resolution: 2047, samples: 0, bits: 32, margin: -1, distanceRatio: NaN, device: -1 });
  assert.equal(value.resolution, 2048); assert.equal(value.samples, 128); assert.equal(value.bits, 8); assert.equal(value.margin, 16); assert.equal(value.distanceRatio, 0.1); assert.equal(value.aoDistanceRatio, 0.01); assert.equal(value.device, 0); assert.equal(value.handle, undefined);
  assert.deepEqual(restoreBakeSettings({ resolution: 4096, selfOnly: true, distanceRatio: 0.2 }).selfOnly, true);
  assert.equal(restoreBakeSettings({ distanceRatio: 0.1 }).aoDistanceRatio, 0.01);
  assert.equal(restoreBakeSettings({ distanceRatio: 0.2 }).aoDistanceRatio, 0.2);
  assert.equal(restoreBakeSettings({ distanceRatio: 0.1, aoDistanceRatio: 0.1 }).aoDistanceRatio, 0.1);
  assert.equal(restoreBakeSettings({ uvMode: 'regenerateAll', deviceLuid: '00000000:1' }).uvMode, 'regenerateAll');
  assert.equal(restoreBakeSettings({ uvMode: 'invalid' }).uvMode, 'preserveValid');
});

test('workspace preferences have independent safe defaults and accept known display options', () => {
  const defaults = restoreBakeSettings(null);
  assert.deepEqual(defaults.workspace, {
    outlinerOpen: true, settingsOpen: true, projection: 'perspective', wireframe: false, mapPreview: 'ao',
  });
  const restored = restoreBakeSettings({
    workspace: { outlinerOpen: false, settingsOpen: false, projection: 'orthographic', grid: false, axes: false, wireframe: true, mapPreview: 'position' },
  });
  assert.deepEqual(restored.workspace, {
    outlinerOpen: false, settingsOpen: false, projection: 'orthographic', wireframe: true, mapPreview: 'position',
  });
  assert.equal(restoreBakeSettings({ workspace: { projection: 'invalid' } }).workspace.projection, 'perspective');
  // 旧版本遗留的网格/坐标轴开关已随视口简化移除，恢复时必须被忽略。
  assert.equal(restoreBakeSettings({ workspace: { grid: 'yes', axes: true } }).workspace.grid, undefined);
  assert.equal(restoreBakeSettings({ workspace: { grid: 'yes', axes: true } }).workspace.axes, undefined);
  assert.equal(restoreBakeSettings({ workspace: { mapPreview: 'invalid' } }).workspace.mapPreview, 'ao');
  assert.equal(restoreBakeSettings({ workspace: { mapPreview: 'ao_unique' } }).workspace.mapPreview, 'ao_unique');
  assert.equal(restoreBakeSettings({ workspace: { mapPreview: 'curvature_unique' } }).workspace.mapPreview, 'curvature_unique');
  assert.equal(restoreBakeSettings({ workspace: { mapPreview: 'thickness_unique' } }).workspace.mapPreview, 'thickness_unique');
  assert.equal(restoreBakeSettings({ workspace: { mapPreview: 'world_normal_unique' } }).workspace.mapPreview, 'world_normal_unique');
  assert.equal(restoreBakeSettings({ workspace: { mapPreview: 'position_unique' } }).workspace.mapPreview, 'position_unique');
});

test('preview prefers reliable scalar maps independently of result order and keeps materials without UV reuse', () => {
  const files = [
    { material: 0, kind: 'ao_unique', path: 'safe0' },
    { material: 1, kind: 'ao', path: 'raw1' },
    { material: 0, kind: 'ao', path: 'raw0' },
    { material: 1, kind: 'thickness', path: 'rawThickness1' },
    { material: 0, kind: 'thickness_unique', path: 'safeThickness0' },
    { material: 0, kind: 'thickness', path: 'rawThickness0' },
    { material: 0, kind: 'curvature_unique', path: 'safeCurvature0' },
    { material: 1, kind: 'curvature', path: 'rawCurvature1' },
    { material: 0, kind: 'curvature', path: 'rawCurvature0' },
    { material: 1, kind: 'world_normal', path: 'rawWorldNormal1' },
    { material: 0, kind: 'world_normal_unique', path: 'safeWorldNormal0' },
    { material: 0, kind: 'world_normal', path: 'rawWorldNormal0' },
    { material: 1, kind: 'position', path: 'rawPosition1' },
    { material: 0, kind: 'position_unique', path: 'safePosition0' },
    { material: 0, kind: 'position', path: 'rawPosition0' },
  ];
  assert.deepEqual(selectReliablePreviewFiles(files, 'ao').map(file => [file.material, file.path]), [[1, 'raw1'], [0, 'safe0']]);
  assert.deepEqual(selectReliablePreviewFiles(files, 'thickness').map(file => [file.material, file.path]), [[1, 'rawThickness1'], [0, 'safeThickness0']]);
  assert.deepEqual(selectReliablePreviewFiles(files, 'curvature').map(file => [file.material, file.path]), [[1, 'rawCurvature1'], [0, 'safeCurvature0']]);
  assert.deepEqual(selectReliablePreviewFiles(files, 'world_normal').map(file => [file.material, file.path]), [[1, 'rawWorldNormal1'], [0, 'safeWorldNormal0']]);
  assert.deepEqual(selectReliablePreviewFiles(files, 'position').map(file => [file.material, file.path]), [[1, 'rawPosition1'], [0, 'safePosition0']]);
});

test('UV coverage explicitly reports materials with no reliable geometry pixels', () => {
  assert.deepEqual(assessUvCoverage([
    { material: 0, coveredPixels: 176312, sharedPixels: 55384 },
    { material: 5, coveredPixels: 262144, sharedPixels: 262144 },
    { material: 20, coveredPixels: 262144, sharedPixels: 262144 },
    { material: 24, coveredPixels: 46319, sharedPixels: 0 },
    { material: 25, coveredPixels: 0, sharedPixels: 0 },
  ]), { shared: 3, noReliablePixels: 2 });
});
