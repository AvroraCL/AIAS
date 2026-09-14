// 固定发布构建环境：
// - thin LTO（见 src-tauri/Cargo.toml）在低内存构建机上仍需限制并行 rustc 数量，
//   否则可能 OOM；在此固化，保证任何机器产物一致。
// - tauri.conf.json 的 beforeBuildCommand 里调 `vite` 依赖 PATH 中有 node_modules/.bin，
//   在部分 shell（Git Bash）下不存在；这里直接补 PATH，避免每次手工绕过。
const { spawnSync } = require('node:child_process');
const path = require('node:path');
const root = path.resolve(__dirname, '..');
const binDir = path.join(root, 'node_modules', '.bin');
const tauriCli = path.join(root, 'node_modules', '@tauri-apps', 'cli', 'tauri.js');
const env = {
  ...process.env,
  CARGO_BUILD_JOBS: process.env.CARGO_BUILD_JOBS || '2',
  PATH: `${binDir}${path.delimiter}${process.env.PATH || ''}`,
};
const result = spawnSync(process.execPath, [tauriCli, 'build'], { cwd: root, env, stdio: 'inherit' });
if (result.error) throw result.error;
process.exit(result.status ?? 1);
