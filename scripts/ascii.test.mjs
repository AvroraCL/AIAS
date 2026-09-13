import { test } from 'node:test';
import assert from 'node:assert/strict';
import { DEFAULTS, restoreAsciiSettings, characterRamp, gridSize, convertAscii } from '../src/renderer/scripts/ascii-state.mjs';
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
