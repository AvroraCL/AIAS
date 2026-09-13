import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync, readdirSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import path from 'node:path';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const scriptsDir = path.join(root, 'src', 'renderer', 'scripts');
const appSource = readFileSync(path.join(scriptsDir, 'app.js'), 'utf8');
const indexSource = readFileSync(path.join(root, 'src', 'renderer', 'index.html'), 'utf8');

// lucide 走显式导入清单：漏导入 = 图标静默消失（侧栏图标缺失事故的根因）。
// 这组测试从源码层面锁死「使用 → 导入 → iconSet → modeRegistry」的同步。

const kebabToPascal = name => name.split('-').map(part => part[0].toUpperCase() + part.slice(1)).join('');

const importBlock = appSource.match(/import\s*\{([^}]+)\}\s*from\s*"lucide"/s)?.[1];
const imported = new Set(
  importBlock?.split(',').map(name => name.trim()).filter(name => name && name !== 'createIcons'),
);

const iconSetBlock = appSource.match(/const iconSet = \{([^}]+)\}/s)?.[1];
const iconSetKeys = new Set([...(iconSetBlock?.matchAll(/([A-Z][A-Za-z0-9]*)/g) ?? [])].map(match => match[1]));

const scriptSources = readdirSync(scriptsDir)
  .filter(name => /\.(js|mjs)$/.test(name))
  .map(name => readFileSync(path.join(scriptsDir, name), 'utf8'));
const usedIcons = new Set();
for (const source of [indexSource, ...scriptSources]) {
  for (const match of source.matchAll(/data-lucide="([a-z0-9-]+)"/g)) usedIcons.add(match[1]);
}

test('iconSet 键集合与 lucide 显式导入清单完全一致', () => {
  assert.ok(importBlock, 'app.js 应包含 lucide 显式导入块');
  assert.ok(iconSetBlock, 'app.js 应包含 iconSet 定义');
  assert.equal(iconSetKeys.size, imported.size, `iconSet 与导入清单数量不同：${[...imported].join(', ')}`);
  for (const name of imported) assert.ok(iconSetKeys.has(name), `已导入 ${name} 但 iconSet 缺失`);
  for (const name of iconSetKeys) assert.ok(imported.has(name), `iconSet 含 ${name} 但未导入`);
});

test('每一个 data-lucide 引用都有对应导入，不会静默丢图标', () => {
  assert.ok(usedIcons.size > 20, `应扫描到大量图标引用，实际 ${usedIcons.size}`);
  for (const name of usedIcons) {
    assert.ok(imported.has(kebabToPascal(name)), `data-lucide="${name}" 未在 lucide 导入清单中，图标会静默缺失`);
  }
});

test('index.html 每个 data-view 都在 modeRegistry 中有接线表项', () => {
  const views = [...indexSource.matchAll(/data-view="([a-z0-9-]+)"/g)].map(match => match[1]);
  assert.ok(views.length >= 14, `应扫描到全部侧栏模式按钮，实际 ${views.length}`);
  const registryBlock = appSource.match(/const modeRegistry = \{[\s\S]*?\n\};/)?.[0];
  assert.ok(registryBlock, 'app.js 应包含 modeRegistry 定义');
  const registered = new Set([...registryBlock.matchAll(/^ {2}"?([a-z0-9-]+)"?:/gm)].map(match => match[1]));
  for (const view of views) {
    assert.ok(registered.has(view), `模式 ${view} 在 modeRegistry 中没有表项（运行按钮/日志/拖放将静默失效）`);
  }
});
