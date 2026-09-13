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