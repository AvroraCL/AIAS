import test from 'node:test';
import assert from 'node:assert/strict';
import { buildBrowserPack, parsePackBlk, safePackName, scanPackFiles } from './skin-pack-core.mjs';
import { unzipSync } from 'fflate';

const blk = `name:t="user"\r\n\r\nreplace_tex{\r\n from:t="body_c*"\r\n to:t="body_c.dds"\r\n}\r\nset_tex{ // fixed camo\r\n from:t="body_alt*"\r\n to:t="body_c.dds"\r\n param:t="camo_skin_tex"\r\n}\r\n`;

function dds() {
  const bytes = new Uint8Array(128 + 64);
  const view = new DataView(bytes.buffer);
  bytes.set([68, 68, 83, 32]);
  view.setUint32(4, 124, true);
  view.setUint32(12, 4, true);
  view.setUint32(16, 4, true);
  view.setUint32(28, 1, true);
  view.setUint32(88, 32, true);
  return bytes;
}

function tga() {
  const bytes = new Uint8Array(18 + 4 * 4 * 3);
  bytes[2] = 2;
  bytes[12] = 4;
  bytes[14] = 4;
  bytes[16] = 24;
  return bytes;
}

test('reads comments and multiple source mappings to one DDS', () => {
  const parsed = parsePackBlk(blk, 'skin.blk');
  assert.deepEqual(parsed.issues, []);
  assert.deepEqual(parsed.references.map(item => item.to), ['body_c.dds', 'body_c.dds']);
});

test('includes one BLK and referenced DDS/TGA, excluding drafts', async () => {
  const files = [new File([blk + 'replace_tex{\nfrom:t="track*"\nto:t="track.tga"\n}\n'], 'skin.blk'),
    new File([dds()], 'body_c.dds'), new File([tga()], 'track.tga'), new File(['draft'], 'old.dds')];
  const result = await scanPackFiles(files);
  assert.deepEqual(result.included.map(item => item.name), ['skin.blk', 'body_c.dds', 'track.tga']);
  assert.deepEqual(result.excluded, ['old.dds']);
  assert.equal(result.issues.some(item => item.level === 'error'), false);
  const archive = unzipSync(await buildBrowserPack(result, 'skin'));
  assert.deepEqual(Object.keys(archive).sort(), ['skin/', 'skin/body_c.dds', 'skin/skin.blk', 'skin/track.tga']);
  assert.equal(new TextDecoder().decode(archive['skin/skin.blk']).startsWith('name:t="user"'), true);
});

test('blocks case mismatch, traversal and corrupt texture', async () => {
  const content = 'name:t="user"\nreplace_tex{\nfrom:t="x*"\nto:t="Body.DDS"\n}\n';
  const mismatch = await scanPackFiles([new File([content], 'skin.blk'), new File([dds()], 'body.dds')]);
  assert.match(mismatch.issues.find(item => item.level === 'error').message, /大小写/);
  const traversal = parsePackBlk(content.replace('Body.DDS', '../body.dds'), 'skin.blk');
  assert.equal(traversal.issues.length, 0);
  assert.equal(safePackName('../body.dds'), false);
  const unsafe = await scanPackFiles([new File([content.replace('Body.DDS', '../body.dds')], 'skin.blk')]);
  assert.match(unsafe.issues.find(item => item.level === 'error').message, /不安全/);
  const corrupt = await scanPackFiles([new File([content.replace('Body.DDS', 'body.dds')], 'skin.blk'), new File(['bad'], 'body.dds')]);
  assert.match(corrupt.issues.find(item => item.level === 'error').message, /无法解码/);
});

test('reports a folder without a BLK', async () => {
  const scanned = await scanPackFiles([new File([dds()], 'body_c.dds')]);
  assert.equal(scanned.included.length, 0);
  assert.match(scanned.issues.find(item => item.level === 'error').message, /没有 BLK/);
});

test('requires one BLK selection and rejects changed files before ZIP creation', async () => {
  const texture = new File([dds()], 'body_c.dds');
  const files = [new File([blk], 'skin.blk'), new File([blk], 'alternate.blk'), texture];
  const pending = await scanPackFiles(files);
  assert.match(pending.issues.find(item => item.level === 'error').message, /多份 BLK/);
  const selected = await scanPackFiles(files, 'skin.blk');
  assert.equal(selected.issues.some(item => item.level === 'error'), false);
  await assert.rejects(buildBrowserPack(selected, '../skin'), /包名无效/);
  texture.arrayBuffer = async () => new TextEncoder().encode('changed').buffer;
  await assert.rejects(buildBrowserPack(selected, 'skin'), /发生变化/);
});

test('rejects non-UTF-8 BLK and truncated RLE TGA', async () => {
  const invalidText = await scanPackFiles([new File([new Uint8Array([0xff, 0xfe])], 'skin.blk')]);
  assert.match(invalidText.issues.find(item => item.level === 'error').message, /UTF-8/);
  const content = 'name:t="user"\nreplace_tex{\nfrom:t="track*"\nto:t="track.tga"\n}\n';
  const rle = tga().subarray(0, 18);
  rle[2] = 10;
  const damaged = await scanPackFiles([new File([content], 'skin.blk'), new File([rle], 'track.tga')]);
  assert.match(damaged.issues.find(item => item.level === 'error').message, /无法解码/);
  const truncatedMip = dds();
  new DataView(truncatedMip.buffer).setUint32(28, 2, true);
  const mipContent = 'name:t="user"\nreplace_tex{\nfrom:t="body_c*"\nto:t="body_c.dds"\n}\n';
  const mipScan = await scanPackFiles([new File([mipContent], 'skin.blk'), new File([truncatedMip], 'body_c.dds')]);
  assert.match(mipScan.issues.find(item => item.level === 'error').message, /无法解码/);
});
