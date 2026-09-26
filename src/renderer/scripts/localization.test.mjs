import test from 'node:test';
import assert from 'node:assert/strict';
import { translateText } from './localization.mjs';

test('language switch translates interface copy while preserving whitespace', () => {
  assert.equal(translateText('  默认输出目录\n', 'en'), '  Default Output Folder\n');
  assert.equal(translateText('默认输出目录', 'zh-CN'), '默认输出目录');
  assert.equal(translateText('中文', 'en'), '中文');
});

test('dynamic status values translate without changing filenames or paths', () => {
  assert.equal(translateText('当前版本 5.7.0', 'en'), 'Version 5.7.0');
  assert.equal(translateText('输出到 F:\\AIAS\\Output', 'en'), 'Output to F:\\AIAS\\Output');
  assert.equal(translateText('sample.blk · 3 条规则', 'en'), 'sample.blk · 3 rules');
  assert.equal(translateText('F:\\测试区\\材质.dds', 'en'), 'F:\\测试区\\材质.dds');
});

test('bake effect settings and previous-result status translate', () => {
  assert.equal(translateText('AO 强度（%）', 'en'), 'AO Strength (%)');
  assert.equal(translateText('自定义 · 16 位 · 留边 8 px', 'en'), 'Custom · 16-bit · 8 px padding');
  assert.match(translateText('上次烘焙设置 · 烘焙贴图与清单缓存在应用数据目录，导出时选择目标文件夹；贴图供智能材质制作使用', 'en'), /^Earlier bake settings · Maps/);
});
