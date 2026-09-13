import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import vm from 'node:vm';

const source = readFileSync(new URL('../src/renderer/scripts/app.js', import.meta.url), 'utf8');
function fixture() {
  const label = { textContent: '开始处理' };
  const classes = new Set();
  let click;
  const button = {
    dataset: {}, disabled: false,
    querySelector: () => label,
    setAttribute(name, value) { this[name] = value; },
    classList: { toggle(name, value) { value ? classes.add(name) : classes.delete(name); } },
    addEventListener(type, callback) { if (type === 'click') click = callback; },
  };
  const context = vm.createContext({
    $: () => button, state: {}, addActivity() {}, updateStatus() {},
  });
  vm.runInContext(source.slice(source.indexOf('function setBusy('), source.indexOf('function setTaskProgress(')), context);
  vm.runInContext(source.slice(source.indexOf('function bindRunAction('), source.indexOf('function bindRunActions(')), context);
  return { context, button, label, classes, click: () => click() };
}

test('run button locks before asynchronous setup and rejects repeated clicks', async () => {
  const f = fixture();
  let finish, calls = 0;
  f.context.bindRunAction('run', async button => {
    ++calls;
    assert.equal(button, f.button);
    await new Promise(resolve => { finish = resolve; });
  });
  const first = f.click();
  assert.equal(f.button.disabled, true);
  assert.equal(f.button['aria-busy'], 'true');
  assert.ok(f.classes.has('busy'));
  await f.click();
  assert.equal(calls, 1);
  finish();
  await first;
  assert.equal(f.button.disabled, false);
  assert.equal(f.button['aria-busy'], 'false');
  assert.equal(f.label.textContent, '开始处理');
});

test('setup failure restores the button and permits retry', async () => {
  const f = fixture();
  let calls = 0;
  f.context.bindRunAction('run', async () => { ++calls; throw Error('save failed'); });
  await f.click();
  assert.equal(f.button.disabled, false);
  assert.ok(!f.classes.has('busy'));
  await f.click();
  assert.equal(calls, 2);
});

test('shared material button restores its current mode label', () => {
  const f = fixture();
  f.context.setBusy(f.button, true);
  f.context.setBusy(f.button, false);
  f.label.textContent = '生成高度图';
  f.context.setBusy(f.button, true);
  f.context.setBusy(f.button, false);
  assert.equal(f.label.textContent, '生成高度图');
});

test('all standard run actions receive a stable button instead of a released event', () => {
  const bound = source.slice(source.indexOf('function bindRunActions('), source.indexOf('  $("superres-scale")', source.indexOf('function bindRunActions(')));
  assert.doesNotMatch(bound, /event\.currentTarget/);
  for (const mode of ['merge', 'split', 'mipmap', 'image-dds', 'anime-cutout', 'superres-anime', 'superres-general']) {
    assert.ok(bound.includes(`bindRunAction("run-${mode}"`));
  }
});
