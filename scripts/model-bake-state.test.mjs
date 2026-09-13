import test from 'node:test';
import assert from 'node:assert/strict';
import { restoreBakeSettings } from '../src/renderer/scripts/model-bake-state.mjs';
test('bake settings restore defaults without model handles and reject invalid quality', () => {
  assert.equal(restoreBakeSettings(null).resolution, 2048);
  const value = restoreBakeSettings({ handle: 'expired', resolution: 2047, samples: 0, bits: 32, margin: -1, distanceRatio: NaN, device: -1 });
  assert.equal(value.resolution, 2048); assert.equal(value.samples, 128); assert.equal(value.bits, 8); assert.equal(value.margin, 16); assert.equal(value.distanceRatio, 0.1); assert.equal(value.device, 0); assert.equal(value.handle, undefined);
  assert.deepEqual(restoreBakeSettings({ resolution: 4096, selfOnly: true, distanceRatio: 0.2 }).selfOnly, true);
});

test('workspace preferences have independent safe defaults and accept known display options', () => {
  const defaults = restoreBakeSettings(null);
  assert.deepEqual(defaults.workspace, {
    outlinerOpen: true, settingsOpen: true, projection: 'perspective', grid: true, axes: true, wireframe: false,
  });
  const restored = restoreBakeSettings({
    workspace: { outlinerOpen: false, settingsOpen: false, projection: 'orthographic', grid: false, axes: false, wireframe: true },
  });
  assert.deepEqual(restored.workspace, {
    outlinerOpen: false, settingsOpen: false, projection: 'orthographic', grid: false, axes: false, wireframe: true,
  });
  assert.equal(restoreBakeSettings({ workspace: { projection: 'invalid', grid: 'yes' } }).workspace.projection, 'perspective');
  assert.equal(restoreBakeSettings({ workspace: { projection: 'invalid', grid: 'yes' } }).workspace.grid, true);
});
