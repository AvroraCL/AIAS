import { test } from 'node:test';
import assert from 'node:assert/strict';
import { DEFAULTS, restoreAsciiSettings, characterRamp, gridSize, convertAscii, detectTransparency } from '../src/renderer/scripts/ascii-state.mjs';
const pixel = (rgba, settings = {}) => convertAscii({pixels: rgba, columns: rgba.length/4, rows:1, settings:{...DEFAULTS,...settings}});
test('black, white and grayscale map to sparse through dense characters', () => {
  assert.equal(pixel([0,0,0,255, 128,128,128,255, 255,255,255,255]).text, ' +@');
  assert.equal(pixel([0,0,0,255,255,255,255,255], {invert:true}).text, '@ ');
  assert.equal(pixel([0,0,0,255,255,255,255,255], {background:'white'}).text, '@ ');
});
test('transparent pixels composite onto chosen background and retain color', () => {
  assert.equal(pixel([255,0,0,0]).text, ' ');
  assert.equal(pixel([255,0,0,0], {background:'white'}).text, ' ');
  assert.deepEqual([...pixel([255,0,0,128]).colors], [128,0,0]);
  assert.deepEqual([...pixel([23,45,67,255], {color:true}).colors], [23,45,67]);
});
test('TXT preserves blank cells, rows and custom character ordering', () => {
  const result = convertAscii({pixels:[0,0,0,255,255,255,255,255,255,255,255,255,0,0,0,255],columns:2,rows:2,settings:{...DEFAULTS,charset:'custom',custom:' x'}});
  assert.equal(result.text, ' x\nx ');
  assert.throws(() => characterRamp({...DEFAULTS,charset:'custom',custom:'中文字'}));
  assert.throws(() => characterRamp({...DEFAULTS,charset:'custom',custom:'  '}));
});
test('grid respects font aspect ratio and caps long images', () => {
  assert.deepEqual(gridSize(100,100,120,9,18),{columns:120,rows:60});
  assert.deepEqual(gridSize(100,200,120,9,18),{columns:120,rows:120});
  const tall = gridSize(10,1000,120,9,18);
  assert.deepEqual(tall,{columns:8,rows:400});
  assert.equal(gridSize(10000,10,120,9,18).rows,1);
});
test('settings restore old and corrupt values safely', () => {
  assert.deepEqual(restoreAsciiSettings(null),DEFAULTS);
  assert.deepEqual(restoreAsciiSettings({columns:Infinity,contrast:-3,format:'html'}),DEFAULTS);
  assert.equal(restoreAsciiSettings({color:true,columns:240}).columns,240);
});
test('PNG becomes the default while new explicit TXT preferences are preserved', () => {
  assert.equal(DEFAULTS.format, 'png');
  assert.equal(restoreAsciiSettings({format:'txt'}).format, 'png');
  assert.equal(restoreAsciiSettings({version:2,format:'txt'}).format, 'txt');
});
test('transparent output keeps source RGB and alpha without a black matte', () => {
  const result = pixel([255,80,20,128,255,255,255,0], {background:'transparent',color:true,invert:true});
  assert.deepEqual([...result.colors], [255,80,20,255,255,255]);
  assert.deepEqual([...result.alphas], [128,0]);
  assert.equal(result.chars[1], ' ');
});

test('style setting restores only known values and keeps versioned format', () => {
  assert.equal(restoreAsciiSettings({version:2, style:'block'}).style, 'block');
  assert.equal(restoreAsciiSettings({version:2, style:'dot'}).style, 'dot');
  assert.equal(restoreAsciiSettings({version:2, style:'halftone'}).style, 'ascii');
  assert.equal(restoreAsciiSettings(null).style, 'ascii');
});

test('lights expose the final per-cell ink level driving block and dot sizes', () => {
  const plain = pixel([0,0,0,255, 255,255,255,255, 128,128,128,255]);
  assert.deepEqual([...plain.lights], [0, 255, Math.round((128/255) * 255 / 2 * 2)]);
  assert.deepEqual([...pixel([0,0,0,255,255,255,255,255], {invert:true}).lights], [255, 0]);
  assert.deepEqual([...pixel([0,0,0,255,255,255,255,255], {background:'white'}).lights], [255, 0]);
});

test('graphic styles skip character ramp validation and blank the text', () => {
  const settings = {...DEFAULTS, style:'dot', charset:'custom', custom:'  '};
  const result = convertAscii({pixels:[255,255,255,255], columns:1, rows:1, settings});
  assert.equal(result.text, ' ');
  assert.equal(result.lights[0], 255);
});

test('transparency detection samples decoded pixels and ignores opaque sources', () => {
  
  assert.equal(detectTransparency(new Uint8ClampedArray([10,20,30,255, 40,50,60,255])), false);
  assert.equal(detectTransparency(new Uint8ClampedArray([10,20,30,255, 40,50,60,128])), true);
  assert.equal(detectTransparency(new Uint8ClampedArray([10,20,30,0])), true);
  assert.equal(detectTransparency(new Uint8ClampedArray([10,20,30])), false);
  assert.equal(detectTransparency(undefined), false);
});

test('shape size and ratio persist with bounds for block and dot styles', () => {
  assert.equal(DEFAULTS.shapeSize, 92);
  assert.equal(DEFAULTS.shapeRatio, 100);
  const kept = restoreAsciiSettings({version:2, style:'dot', shapeSize:35, shapeRatio:180});
  assert.equal(kept.shapeSize, 35);
  assert.equal(kept.shapeRatio, 180);
  assert.equal(restoreAsciiSettings({version:2, shapeSize:300}).shapeSize, DEFAULTS.shapeSize);
  assert.equal(restoreAsciiSettings({version:2, shapeSize:'x'}).shapeSize, DEFAULTS.shapeSize);
  assert.equal(restoreAsciiSettings({version:2, shapeRatio:10}).shapeRatio, DEFAULTS.shapeRatio);
});
