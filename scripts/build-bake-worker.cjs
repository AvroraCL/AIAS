// Build-only toolchain. End users run the bundled worker and embedded DXIL offline.
const fs = require('node:fs');
const path = require('node:path');
const crypto = require('node:crypto');
const { spawnSync } = require('node:child_process');
const root = path.resolve(__dirname, '..');
const tools = path.join(root, 'tools', 'dxc');
const compiler = path.join(tools, 'bin', 'x64', 'dxc.exe');
const url = 'https://github.com/microsoft/DirectXShaderCompiler/releases/download/v1.8.2505.1/dxc_2025_07_14.zip';
const sha256 = '9ad895a6b039e3a8f8c22a1009f866800b840a74b50db9218d13319e215ea8a4';
function run(command, args) { const result = spawnSync(command, args, { cwd: root, stdio: 'inherit', windowsHide: true }); if (result.error) throw result.error; if (result.status !== 0) throw Error(`${command} exited ${result.status}`); }
(async () => {
  fs.mkdirSync(tools, { recursive: true });
  const archive = path.join(tools, 'dxc.zip');
  if (!fs.existsSync(archive)) { const response = await fetch(url, { signal: AbortSignal.timeout(60_000) }); if (!response.ok) throw Error(`DXC download: ${response.status}`); fs.writeFileSync(archive, Buffer.from(await response.arrayBuffer())); }
  if (crypto.createHash('sha256').update(fs.readFileSync(archive)).digest('hex') !== sha256) {
    // 损坏的缓存会让后续每次构建都在同一份坏档上失败，删掉让它下次自动重下。
    fs.unlinkSync(archive);
    throw Error('DXC checksum mismatch (cached archive removed; retry to re-download)');
  }
  // Always extract the verified archive; a modified cached executable is never trusted.
  const quote = value => `'${value.replaceAll("'", "''")}'`;
  run('powershell.exe', ['-NoProfile', '-NonInteractive', '-Command', `Expand-Archive -LiteralPath ${quote(archive)} -DestinationPath ${quote(tools)} -Force`]);
  run(compiler, ['-T', 'cs_6_5', '-E', 'main', '-O3', '-Fo', 'bake-worker/shaders/ao.dxil', 'bake-worker/shaders/ao.hlsl']);
  run('cargo', ['build', '--locked', '--release', '--manifest-path', 'bake-worker/Cargo.toml']);
  const out = path.join(root, 'build', 'bake'); fs.mkdirSync(out, { recursive: true });
  fs.copyFileSync(path.join(root, 'bake-worker', 'target', 'release', 'aias-bake-worker.exe'), path.join(out, 'aias-bake-worker.exe'));
  fs.copyFileSync(path.join(root, 'bake-worker', 'shaders', 'ao.dxil'), path.join(out, 'ao.dxil'));
  // OIDN 降噪器 CPU 忒需文件
  const oidnOut = path.join(out, 'oidn');
  fs.mkdirSync(oidnOut, { recursive: true });
  const oidnBin = path.join(root, '测试区', 'oidn-test', 'bin');
  for (const name of ['OpenImageDenoise.dll', 'OpenImageDenoise_core.dll', 'OpenImageDenoise_device_cpu.dll', 'tbb12.dll', 'OIDN-LICENSE.txt']) {
    fs.copyFileSync(path.join(oidnBin, name), path.join(oidnOut, name));
  }
})().catch(error => { console.error(error); process.exitCode = 1; });
