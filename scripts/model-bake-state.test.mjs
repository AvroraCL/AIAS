import test from 'node:test';
import assert from 'node:assert/strict';
import { restoreBakeSettings } from '../src/renderer/scripts/model-bake-state.mjs';
test('bake settings restore defaults without model handles and reject invalid quality', () => {
  assert.equal(restoreBakeSettings(null).resolution, 2048);
  assert.equal(restoreBakeSettings(null).uvMode, 'preserveValid');
  assert.equal(restoreBakeSettings(null).deviceLuid, '');
  for (const key of ['ao', 'normal', 'worldNormal', 'curvature', 'position', 'thickness', 'id']) assert.equal(restoreBakeSettings(null)[key], true);
  assert.equal(restoreBakeSettings(null).uv, false);
  const value = restoreBakeSettings({ handle: 'expired', resolution: 2047, samples: 0, bits: 32, margin: -1, distanceRatio: NaN, device: -1 });
  assert.equal(value.resolution, 2048); assert.equal(value.samples, 128); assert.equal(value.bits, 8); assert.equal(value.margin, 16); assert.equal(value.distanceRatio, 0.1); assert.equal(value.device, 0); assert.equal(value.handle, undefined);
  assert.deepEqual(restoreBakeSettings({ resolution: 4096, selfOnly: true, distanceRatio: 0.2 }).selfOnly, true);
  assert.equal(restoreBakeSettings({ uvMode: 'regenerateAll', deviceLuid: '00000000:1' }).uvMode, 'regenerateAll');
  assert.equal(restoreBakeSettings({ uvMode: 'invalid' }).uvMode, 'preserveValid');
});

test('workspace preferences have independent safe defaults and accept known display options', () => {
  const defaults = restoreBakeSettings(null);
  assert.deepEqual(defaults.workspace, {
    outlinerOpen: true, settingsOpen: true, projection: 'perspective', grid: true, axes: true, wireframe: false, mapPreview: 'ao',
  });
  const restored = restoreBakeSettings({
    workspace: { outlinerOpen: false, settingsOpen: false, projection: 'orthographic', grid: false, axes: false, wireframe: true, mapPreview: 'position' },
  });
  assert.deepEqual(restored.workspace, {
    outlinerOpen: false, settingsOpen: false, projection: 'orthographic', grid: false, axes: false, wireframe: true, mapPreview: 'position',
  });
  assert.equal(restoreBakeSettings({ workspace: { projection: 'invalid', grid: 'yes' } }).workspace.projection, 'perspective');
  assert.equal(restoreBakeSettings({ workspace: { projection: 'invalid', grid: 'yes' } }).workspace.grid, true);
  assert.equal(restoreBakeSettings({ workspace: { mapPreview: 'invalid' } }).workspace.mapPreview, 'ao');
});
