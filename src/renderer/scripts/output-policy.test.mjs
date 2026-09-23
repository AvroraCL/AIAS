import test from 'node:test';
import assert from 'node:assert/strict';
import { inputDirectory, resolveOutputDirectory } from './output-policy.mjs';

test('directory inputs stay in their directory, file inputs use their parent', () => {
  const settings = { outputStrategy: 'input' };
  assert.equal(resolveOutputDirectory('', 'F:\\AIAS\\Textures', settings, true), 'F:\\AIAS\\Textures');
  assert.equal(resolveOutputDirectory('', 'F:\\AIAS\\Textures\\', settings, true), 'F:\\AIAS\\Textures');
  assert.equal(resolveOutputDirectory('', 'F:\\AIAS\\Textures\\tank.dds', settings), 'F:\\AIAS\\Textures');
  assert.equal(inputDirectory('C:\\image.png'), 'C:\\');
  assert.equal(inputDirectory('/image.png'), '/');
});

test('fixed policy requires a configured directory and ask uses the per-mode value', () => {
  assert.equal(resolveOutputDirectory('D:\\old', 'C:\\input.png', { outputStrategy: 'fixed', defaultOutputDir: 'F:\\out' }), 'F:\\out');
  assert.equal(resolveOutputDirectory('D:\\old', 'C:\\input.png', { outputStrategy: 'fixed' }), '');
  assert.equal(resolveOutputDirectory('D:\\old', 'C:\\input.png', { outputStrategy: 'ask' }), 'D:\\old');
  assert.equal(resolveOutputDirectory('', '', { outputStrategy: 'input' }), '');
});
