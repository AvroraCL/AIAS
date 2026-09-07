import { test } from 'node:test';
import assert from 'node:assert/strict';
import { createUpdateController, scheduleUpdateCheck } from '../src/renderer/scripts/updater.mjs';

function fixture(overrides = {}) {
  const events = [];
  const update = {
    version: '5.4.4',
    async download() { events.push('download'); },
    async install() { events.push('install'); },
    async close() { events.push('close'); }
  };
  const options = {
    isDesktop: true,
    check: async () => { events.push('check'); return update; },
    checkMirror: async () => null,
    getVersion: async () => '5.4.3',
    ui: {
      busy: value => events.push(['busy', value]),
      available: value => events.push(['available', value]),
      activity: (...args) => events.push(args),
      status: value => events.push(value),
      confirm: async () => true,
      message: async (...args) => events.push(args),
      formatSize: String
    },
    ...overrides
  };
  return { events, update, options, run: createUpdateController(options) };
}

test('disabled startup checks, including disabling before timer fires', () => {
  let callback;
  let calls = 0;
  const settings = { autoUpdate: false };
  const schedule = fn => { callback = fn; };
  scheduleUpdateCheck(true, settings, schedule, () => calls++);
  assert.equal(callback, undefined);
  settings.autoUpdate = true;
  scheduleUpdateCheck(true, settings, schedule, () => calls++);
  callback();
  assert.equal(calls, 1);
  settings.autoUpdate = false;
  callback();
  assert.equal(calls, 1);
});

test('browser explains desktop requirement without invoking updater', async () => {
  const f = fixture({ isDesktop: false });
  await f.run(false);
  assert.equal(f.events.includes('check'), false);
  assert.equal(f.events[0][0], '桌面版功能');
});

test('concurrent clicks perform a single check, download and install', async () => {
  const f = fixture();
  await Promise.all([f.run(false), f.run(false)]);
  for (const event of ['check', 'download', 'install', 'close']) {
    assert.equal(f.events.filter(e => e === event).length, 1);
  }
  assert.deepEqual(f.events.at(-1), ['busy', false]);
});

test('current version comes from running app', async () => {
  const f = fixture({ check: async () => null });
  await f.run(false);
  assert.ok(f.events.some(e => e[1] === '当前版本 5.4.3'));
});

test('silent check and cancellation release resources without downloading', async () => {
  for (const silent of [true, false]) {
    const f = fixture();
    f.options.ui.confirm = async () => false;
    await f.run(silent);
    assert.ok(f.events.includes('close'));
    assert.equal(f.events.includes('download'), false);
  }
});

test('download failure retries mirror of the approved version', async () => {
  let downloaded = 0, installed = 0, closed = 0;
  const f = fixture({ checkMirror: async () => ({
    version: '5.4.4',
    download: async () => downloaded++,
    install: async () => installed++,
    close: async () => closed++
  }) });
  f.update.download = async () => { throw Error('network'); };
  await f.run(false);
  assert.deepEqual([downloaded, installed, closed], [1, 1, 1]);
  assert.equal(f.events.includes('install'), false);
});

test('stale mirror never downloads or installs another version', async () => {
  const f = fixture({ checkMirror: async () => ({
    version: '5.4.2',
    download: async () => assert.fail('wrong version'),
    close: async () => {}
  }) });
  f.update.download = async () => { throw Error('network'); };
  await f.run(false);
  assert.ok(f.events.some(e => e[0] === '更新失败'));
  assert.equal(f.events.includes('install'), false);
});

test('installation error does not trigger another download or installer', async () => {
  const f = fixture({ checkMirror: async () => assert.fail('must not retry installation') });
  f.update.install = async () => { throw Error('installer failed'); };
  await f.run(false);
  assert.ok(f.events.some(e => e[0] === '更新失败'));
  assert.deepEqual(f.events.at(-1), ['busy', false]);
});
