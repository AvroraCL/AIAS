import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import vm from 'node:vm';
const source = readFileSync(new URL('../src/renderer/scripts/app.js', import.meta.url), 'utf8');
const slice = (from, to) => source.slice(source.indexOf(from), source.indexOf(to));

function fixture() {
  const elements = new Map();
  // textContent 按 DOM 语义从子节点合成；直接赋值时清空子节点并存为直设文本。
  const makeElement = () => {
    const el = { dataset: {}, className: '', children: [], disabled: false, __parent: null, __direct: '', firstElementChild: null, scrollTop: 0, scrollHeight: 100, clientHeight: 100, parentElement: null, classList: { add() {}, remove() {}, toggle() {} }, style: { setProperty() {} }, setAttribute() {}, addEventListener() {}, append(...nodes) { for (const node of nodes) { node.__parent = this; this.children.push(node); } }, remove() { const list = this.__parent?.children; const i = list ? list.indexOf(this) : -1; if (i >= 0) list.splice(i, 1); } };
    Object.defineProperty(el, 'firstElementChild', { get: () => el.children[0] ?? null });
    Object.defineProperty(el, 'textContent', {
      get: () => (el.children.length ? el.children.map(child => child.textContent).join('') : el.__direct),
      set(value) { el.children.length = 0; el.__direct = value; }
    });
    return el;
  };
  const $ = id => {
    if (!elements.has(id)) elements.set(id, makeElement());
    return elements.get(id);
  };
  const context = vm.createContext({ $, state: {}, Date, document: { createElement: () => makeElement(), createTextNode: text => ({ textContent: text }) }, setText: (id, text) => $(id).textContent = text, setTaskProgress() {},
    setInterval: () => 1, clearInterval() {}, setBusy() {}, setActivityPanel() {}, addActivity() {}, reportRunBlocker() {}, getModeOutputPath: () => '', updateStatus() {} });
  vm.runInContext(slice('const LOG_MAX_LINES', 'function collectSettings('), context);
  return { context, $ };
}

test('appendLogLine stamps time, applies level, and falls back to prefix classification', () => {
  const { context, $ } = fixture();
  const log = $('log');
  context.appendLogLine(log, '完成 a → b', 'success');
  context.appendLogLine(log, '失败 x：boom');
  context.appendLogLine(log, '跳过（文件不存在）：c');
  context.appendLogLine(log, '普通信息');
  assert.deepEqual(log.children.map(line => line.className), ['log-line success', 'log-line error', 'log-line warn', 'log-line info']);
  assert.match(log.children[0].textContent, /^\[\d{2}:\d{2}:\d{2}\] 完成 a → b$/);
});

test('run separator preserves previous run lines instead of clearing them', async () => {
  const { context, $ } = fixture();
  await context.withLog('log', null, async () => ({ completed: 1, total: 1, logs: ['第一次的行'] }), 't');
  const afterFirst = log => log.children.map(line => line.textContent);
  await context.withLog('log', null, async () => ({ completed: 2, total: 2, logs: [] }), 't');
  const texts = afterFirst($('log'));
  assert.ok(texts.some(text => text.includes('第一次的行')), 'previous run content survives');
  assert.equal(texts.filter(text => text.includes('── 运行 ·')).length, 2);
});

test('streaming run skips batch logs but keeps the completion line', async () => {
  const { context, $ } = fixture();
  // 时序对齐真实流式：事件在 action 执行期间到达（streamingReceived 置位）。
  await context.withLog('log', null, async () => { context.state.streamingReceived = true; return { completed: 3, total: 3, logs: ['已被实时推送的行'] }; }, 't');
  const texts = $('log').children.map(line => line.textContent);
  assert.ok(!texts.some(text => text.includes('已被实时推送的行')), 'streamed lines are not re-appended');
  assert.ok(texts.some(text => text.includes('完成：3 / 3')));
});

test('log lines are trimmed to the 2000-line cap', () => {
  const { context, $ } = fixture();
  const log = $('log');
  for (let i = 0; i < 2005; i += 1) context.appendLogLine(log, `行 ${i}`);
  assert.equal(log.children.length, 2000);
  assert.ok(log.children[0].textContent.includes('行 5'), 'oldest lines are dropped from the head');
});

test('auto-scroll follows only when the viewer is near the bottom', () => {
  const { context, $ } = fixture();
  const log = $('log');
  const viewer = { scrollTop: 0, scrollHeight: 100, clientHeight: 100 };
  log.parentElement = viewer;
  context.appendLogLine(log, '近底部一行');
  assert.equal(viewer.scrollTop, 100, 'sticks to bottom when within 40px');
  viewer.scrollTop = 0;
  viewer.scrollHeight = 5000;
  context.appendLogLine(log, '远底部一行');
  assert.equal(viewer.scrollTop, 0, 'does not hijack manual scrolling');
});
