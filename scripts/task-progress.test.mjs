import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import vm from 'node:vm';
const source = readFileSync(new URL('../src/renderer/scripts/app.js', import.meta.url), 'utf8');
function fixture() {
  const elements = new Map();
  // 运行记录改为 DOM 行结构后，withLog 依赖 document.createElement 与
  // log.append/children；桩只需满足"可追加、有 children 数组"的最小形态。
  const makeElement = () => ({ textContent: '', dataset: {}, className: '', children: [], firstElementChild: null, scrollTop: 0, scrollHeight: 100, clientHeight: 100, parentElement: null, classList: { remove() {}, toggle() {} }, style: { setProperty() {} }, setAttribute() {}, append() {}, remove() {} });
  const $ = id => {
    if (!elements.has(id)) elements.set(id, makeElement());
    return elements.get(id);
  };
  let cleared = false;
  const context = vm.createContext({ $, state: {}, Date, setText: (id, text) => $(id).textContent = text,
    document: { createElement: () => makeElement(), createTextNode: text => ({ textContent: text }) },
    setInterval: () => 1, clearInterval: () => { cleared = true; }, setBusy() {}, setActivityPanel() {}, addActivity() {}, reportRunBlocker() {}, getModeOutputPath: () => '', updateStatus() {} });
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


test('concurrent task is rejected without disabling the active progress', async () => {
  const { context } = fixture();
  let finish;
  const first = context.withLog('first', null, () => new Promise(resolve => { finish = resolve; }), 'first');
  let invoked = false;
  const second = await context.withLog('second', null, async () => { invoked = true; return { completed: 1, total: 1 }; }, 'second');
  assert.equal(second, null);
  assert.equal(invoked, false);
  assert.equal(context.state.taskProgressActive, true);
  finish({ completed: 1, total: 1 });
  await first;
  assert.equal(context.state.taskProgressActive, false);
});

test('batch blocker catches same stems across directories and extensions', () => {
  const { context } = fixture();
  context.basename = path => path.split(/[\\/]/).pop();
  context.state.superresFiles = ['C:/one/Hero.png', 'C:/two/hero.jpg'];
  vm.runInContext(source.slice(source.indexOf('function getRunBlocker('), source.indexOf('function reportRunBlocker(')), context);
  assert.match(context.getRunBlocker('superres-anime'), /重名/);
});
