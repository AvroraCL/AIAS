import test from 'node:test';
import assert from 'node:assert/strict';
import { restoreStyleLabSettings, STYLE_DEFAULTS, STYLE_ENUMS } from '../src/renderer/scripts/style-lab-state.mjs';

test('customPresets 恢复不再抛错并剔除嵌套键', () => {
  const restored = restoreStyleLabSettings({
    style: 'glitch',
    customPresets: [
      { name: '我的预设', settings: { style: 'camo', split: 30, customPresets: [] } },
      { name: '', settings: {} },
      null,
      { name: 'x'.repeat(30), settings: { split: 40 } },
    ],
  });
  assert.equal(restored.customPresets.length, 2);
  assert.equal(restored.customPresets[0].name, '我的预设');
  assert.equal(restored.customPresets[0].settings.style, undefined);
  assert.equal(restored.customPresets[0].settings.customPresets, undefined);
  assert.equal(restored.customPresets[1].name, 'xxxxxxxxxx'.repeat(2).slice(0, 20));
});

test('枚举参数按当前样式的枚举表恢复（值而非标签）', () => {
  const camo = restoreStyleLabSettings({ style: 'camo', pattern: 'digital' });
  assert.equal(camo.pattern, 'digital');
  assert.equal(STYLE_DEFAULTS.camo.pattern, 'blotch');
  // 跨样式的同名键不串扰：thermal 无 pattern 参数
  const thermal = restoreStyleLabSettings({ style: 'thermal', pattern: 'digital', lut: 'rainbow' });
  assert.equal(thermal.lut, 'rainbow');
  assert.equal(thermal.pattern, undefined);
  // 非法枚举值回落默认
  const bad = restoreStyleLabSettings({ style: 'camo', pattern: '不存在' });
  assert.equal(bad.pattern, 'blotch');
});

test('exportScale 支持 1/2/4，非法值回落 1', () => {
  assert.equal(restoreStyleLabSettings({ exportScale: 4 }).exportScale, 4);
  assert.equal(restoreStyleLabSettings({ exportScale: 3 }).exportScale, 1);
});

test('未知样式回落 glitch 且数值参数受范围钳制', () => {
  const restored = restoreStyleLabSettings({ style: '不存在', split: 999 });
  assert.equal(restored.style, 'glitch');
  assert.ok(restored.split <= 80);
});

test('枚举表形状不变式：每项都是 [参数键, 值标签对列表|null]', () => {
  for (const entries of Object.values(STYLE_ENUMS)) {
    for (const [key, options] of entries) {
      assert.equal(typeof key, 'string');
      if (options) {
        for (const [value, label] of options) {
          assert.equal(typeof value, 'string', `${key} 枚举值应为字符串`);
          assert.equal(typeof label, 'string', `${key} 枚举标签应为字符串`);
        }
      }
    }
  }
});
