import { buildBrowserPack, safePackName, scanPackFiles } from './skin-pack-core.mjs';

const $ = id => document.getElementById(id);

export function createSkinPack({ desktop, invoke, saveDialog, pickDirectory, saveDirectory, confirm, notify, changed, goBlk, syncSelect }) {
  const input = $('pack-browser-directory');
  const directory = $('pack-directory');
  if (!desktop) directory.value = '';
  const name = $('pack-name');
  const select = $('pack-blk');
  let scan = null;
  let lastOutputDir = '';
  let browserFiles = [];
  let version = 0;
  let scanning = false;

  function blocker() {
    if (scanning) return '正在检查文件，请稍候。';
    if (!directory.value) return '请选择涂装目录。';
    if (!scan) return '请重新扫描涂装目录。';
    if (!safePackName(name.value.trim())) return '包名无效。';
    return scan.issues.find(item => item.level === 'error')?.message || null;
  }

  function render() {
    select.replaceChildren();
    for (const blk of scan?.blks || []) {
      const option = document.createElement('option');
      option.value = blk;
      option.textContent = blk;
      select.append(option);
    }
    select.disabled = (scan?.blks.length || 0) < 2;
    if (scan?.selectedBlk) select.value = scan.selectedBlk;
    syncSelect(select);
    $('pack-blk-row').hidden = !(scan?.blks.length);
    $('pack-empty').hidden = Boolean(directory.value);
    $('pack-summary').textContent = scan?.selectedBlk
      ? `${scan.included.length} 个文件 · ${scan.included.reduce((sum, file) => sum + file.size, 0).toLocaleString()} 字节`
      : (directory.value ? '请选择一份 BLK' : '等待选择目录');
    const issues = $('pack-issues');
    issues.replaceChildren();
    for (const item of scan?.issues || []) {
      const row = document.createElement('li');
      row.className = `pack-issue ${item.level}`;
      row.textContent = `${item.level === 'error' ? '需修复' : '建议'} · ${item.file ? `${item.file}${item.line ? `:${item.line}` : ''} · ` : ''}${item.message}`;
      issues.append(row);
    }
    $('pack-errors-heading').textContent = scan?.issues.some(item => item.level === 'error') ? '检查未通过' : scan ? '检查通过' : '等待检查';
    const included = $('pack-included');
    included.replaceChildren();
    for (const file of scan?.included || []) {
      const row = document.createElement('li');
      const label = document.createElement('strong');
      label.textContent = file.name;
      const detail = document.createElement('small');
      detail.textContent = file.width ? `${file.width} × ${file.height}${file.mipLevels ? ` · ${file.mipLevels} Mipmap` : ''}` : 'BLK 配置';
      row.append(label, detail);
      included.append(row);
    }
    const excluded = $('pack-excluded');
    excluded.replaceChildren();
    for (const file of scan?.excluded || []) {
      const row = document.createElement('li');
      row.textContent = file;
      excluded.append(row);
    }
    $('pack-excluded-wrap').hidden = !scan?.excluded.length;
    $('pack-go-blk').hidden = !scan || Boolean(scan.blks.length);
    changed();
  }

  async function rescan(selectedBlk = null) {
    if (!directory.value) return;
    const run = ++version;
    scanning = true;
    scan = null;
    render();
    try {
      const result = desktop
        ? await invoke('skin_pack_scan', { directory: directory.value, selectedBlk })
        : await scanPackFiles(browserFiles, selectedBlk);
      if (run !== version) return;
      scan = result;
      if (result.selectedBlk && (!name.value.trim() || name.dataset.auto === 'true')) {
        name.value = result.selectedBlk.replace(/\.blk$/i, '');
        name.dataset.auto = 'true';
      }
    } catch (error) {
      if (run === version) {
        scan = { blks: [], selectedBlk: null, included: [], excluded: [], issues: [{ level: 'error', message: error.message || String(error) }], fingerprint: '' };
      }
      throw error;
    } finally {
      if (run === version) { scanning = false; render(); }
    }
  }

  async function setDirectory(value) {
    directory.value = value;
    lastOutputDir = '';
    name.value = '';
    name.dataset.auto = 'true';
    await rescan();
    if (desktop) await saveDirectory();
  }

  async function chooseDirectory() {
    if (!desktop) { input.click(); return; }
    const chosen = await pickDirectory();
    if (chosen) await setDirectory(chosen);
  }

  async function generate() {
    const error = blocker();
    if (error) throw new Error(error);
    const packageName = name.value.trim();
    const current = scan;
    if (desktop) {
      const latest = await invoke('skin_pack_scan', { directory: directory.value, selectedBlk: current.selectedBlk });
      if (latest.fingerprint !== current.fingerprint || latest.issues.some(item => item.level === 'error')) throw new Error('涂装文件发生变化，请重新扫描。');
      const outputPath = await saveDialog({ defaultPath: `${packageName}.zip`, filters: [{ name: 'ZIP', extensions: ['zip'] }] });
      if (!outputPath) return null;
      const expectedOutput = await invoke('skin_pack_output_hash', { path: outputPath });
      if (expectedOutput && !await confirm('确认覆盖 ZIP', `已有文件：${outputPath}\n确认替换吗？`, { confirmText: '覆盖', danger: true })) return null;
      const result = await invoke('skin_pack_export', { options: { directory: directory.value, selectedBlk: current.selectedBlk, packageName, outputPath, expectedFingerprint: current.fingerprint, expectedOutput } });
      lastOutputDir = result.replace(/[\\/][^\\/]+$/, '');
      changed();
      return result;
    }
    const bytes = await buildBrowserPack(current, packageName);
    const url = URL.createObjectURL(new Blob([bytes], { type: 'application/zip' }));
    const link = document.createElement('a');
    link.href = url;
    link.download = `${packageName}.zip`;
    document.body.append(link);
    link.click();
    link.remove();
    setTimeout(() => URL.revokeObjectURL(url), 30000);
    return link.download;
  }

  $('pack-pick-directory').addEventListener('click', () => chooseDirectory().catch(error => notify(error.message, 'error')));
  $('pack-rescan').addEventListener('click', () => rescan(select.value || null).catch(error => notify(error.message, 'error')));
  $('pack-go-blk').addEventListener('click', goBlk);
  select.addEventListener('change', () => rescan(select.value).catch(error => notify(error.message, 'error')));
  name.addEventListener('input', () => { name.dataset.auto = 'false'; changed(); });
  input.addEventListener('change', () => {
    const selected = [...input.files];
    if (!selected.length) return;
    const folder = selected[0].webkitRelativePath.split('/')[0];
    browserFiles = selected.filter(file => file.webkitRelativePath.split('/').length === 2 && file.webkitRelativePath.split('/')[0] === folder);
    setDirectory(folder).catch(error => notify(error.message, 'error'));
  });
  if (desktop && directory.value) rescan().catch(error => notify(error.message, 'error'));
  else render();
  return { blocker, generate, rescan, setDirectory, outputPath: () => lastOutputDir };
}
