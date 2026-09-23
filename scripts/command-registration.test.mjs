import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync, readdirSync } from 'node:fs';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';

test('renderer IPC calls are registered in the Tauri command handler', () => {
  const directory = new URL('../src/renderer/scripts/', import.meta.url);
  const source = readdirSync(directory)
    .filter(name => /\.(?:m?js)$/.test(name))
    .map(name => readFileSync(join(fileURLToPath(directory), name), 'utf8'))
    .join('\n');
  const main = readFileSync(new URL('../src-tauri/src/main.rs', import.meta.url), 'utf8');
  const handler = main.match(/\.invoke_handler\(tauri::generate_handler!\[([\s\S]*?)\]\)/)?.[1];
  assert.ok(handler, 'Tauri command handler exists');
  const commands = [...new Set([...source.matchAll(/\binvoke\(['"]([a-z][a-z0-9_]*)['"]/g)].map(match => match[1]))];
  const missing = commands.filter(command => !new RegExp(`(?:^|\\W)${command}(?:\\W|$)`).test(handler));
  assert.deepEqual(missing, [], `Unregistered IPC commands: ${missing.join(', ')}`);
});
