import test from 'node:test';
import assert from 'node:assert/strict';
import { decodeDdsThumbnail } from './blk-dds.mjs';

function dds(width, height, format, payload, mips = 1) {
  const header = new Uint8Array(128);
  const view = new DataView(header.buffer);
  view.setUint32(0, 0x20534444, true);
  view.setUint32(4, 124, true);
  view.setUint32(12, height, true);
  view.setUint32(16, width, true);
  view.setUint32(28, mips, true);
  if (format === 'DXT5') {
    header.set(new TextEncoder().encode('DXT5'), 84);
  } else {
    view.setUint32(88, 32, true);
    view.setUint32(92, 0xff, true);
    view.setUint32(96, 0xff00, true);
    view.setUint32(100, 0xff0000, true);
    view.setUint32(104, 0xff000000, true);
  }
  return new Blob([header, payload]);
}

test('reads RGBA8 pixels including transparency', async () => {
  const source = dds(2, 1, 'RGBA8', Uint8Array.of(10, 20, 30, 0, 90, 80, 70, 128));
  const result = await decodeDdsThumbnail(source);
  assert.deepEqual([result.width, result.height], [2, 1]);
  assert.deepEqual([...result.pixels], [10, 20, 30, 0, 90, 80, 70, 128]);
});

test('reads DXT5 color and alpha from a compressed block', async () => {
  const block = new Uint8Array(16);
  block[0] = 128;
  block[1] = 0;
  block[8] = 0x00; block[9] = 0xf8;
  block[10] = 0x00; block[11] = 0xf8;
  const result = await decodeDdsThumbnail(dds(4, 4, 'DXT5', block));
  assert.deepEqual([...result.pixels.slice(0, 4)], [255, 0, 0, 128]);
  assert.equal(result.pixels.length, 4 * 4 * 4);
});

test('rejects damaged and unsupported DDS without affecting mapping', async () => {
  await assert.rejects(decodeDdsThumbnail(new Blob([new Uint8Array(6)])), /不完整/);
  await assert.rejects(decodeDdsThumbnail(dds(4, 4, 'DXT5', new Uint8Array(1))), /不完整/);
  const unsupported = dds(4, 4, 'DXT5', new Uint8Array(16));
  const bytes = new Uint8Array(await unsupported.arrayBuffer());
  bytes.set(new TextEncoder().encode('DX10'), 84);
  await assert.rejects(decodeDdsThumbnail(new Blob([bytes])), /暂不支持/);
});
