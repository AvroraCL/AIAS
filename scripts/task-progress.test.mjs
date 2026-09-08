import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import vm from 'node:vm';
const source = readFileSync(new URL('../src/renderer/scripts/app.js', import.meta.url), 'utf8');
function fixture() {
  const elements = new Map();
  const $ = id => {
    if (!elements.has(id)) elements.set(id, { textContent: '', dataset: {}, classList: { remove() {}, toggle() {} }, style: { setProperty() {} }, setAttribute() {} });
    return elements.get(id);
  };
  let cleared = false;
  const context = vm.createContext({ $, state: {}, Date, setText: (id, text) => $(id).textContent = text,
    setInterval: () => 1, clearInterval: () => { cleared = true; }, setBusy() {}, setActivityPanel() {}, addActivity() {}, getModeOutputPath: () => '', updateStatus() {} });
  vm.runInContext(source.slice(source.indexOf('function setTaskProgress('), source.indexOf('function collectSettings(')), context);
  return { context, $, cleared: () => cleared };
}
test('save-stage percentage cannot round up to 100', () => {
  const { context, $ } = fixture();
  context.setTaskProgress(0, 1, 'saving', 99.99);
  assert.equal($('task-progress-value').textContent, '99%');
  context.setTaskProgress(0, 1, 'invalid', NaN);
  assert.equal($('task-progress-value').textContent, '0%');
});
test('partial success ends processing with a distinct status and clears timer', async () => {
  const { context, $, cleared } = fixture();
  await context.withLog('log', null, async () => ({ completed: 1, total: 2 }), 'test');
  assert.equal($('task-progress').dataset.status, 'partial');
  assert.equal($('task-progress-value').textContent, '100%');
  assert.match($('task-progress-label').textContent, /1\/2/);
  assert.ok(cleared());
});
test('failure preserves reached progress and exposes the error', async () => {
  const { context, $, cleared } = fixture();
  await context.withLog('log', null, async () => { context.setTaskProgress(0, 1, 'work', 42); throw Error('disk full'); }, 'test');
  assert.equal($('task-progress').dataset.status, 'error');
  assert.equal($('task-progress-value').textContent, '42%');
  assert.match($('task-progress-label').textContent, /disk full/);
  assert.equal(context.state.taskProgressActive, false);
  assert.ok(cleared());
});
