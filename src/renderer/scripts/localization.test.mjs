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
