import { DEFAULTS, restoreAsciiSettings, characterRamp, gridSize, detectTransparency } from './ascii-state.mjs';
import './ascii.css';

export function createAscii({ root, inspector, runArea, desktop, open, saveDialog, convertFileSrc, invoke, settings, save, setBusy, syncSelect, busy, changed, notify }) {
  let config = restoreAsciiSettings(settings), source = null, sourceName = '', result = null;
  let active = false, exporting = false, importing = false, computing = false, revision = 0, importRevision = 0, alphaAutoSwitch = false;
  let worker, timer, saveTimer, disposed = false, view = 'ascii', zoom = 1, saveChain = Promise.resolve();
  const font = '16px Consolas, "Courier New", monospace', lineHeight = 18;
  const measure = document.createElement('canvas').getContext('2d'); measure.font = font;
  const cellWidth = measure.measureText('M').width;
  root.innerHTML = `<div class="ascii-toolbar"><button id="ascii-import" class="secondary-action" type="button">选择图片</button><button id="ascii-clear" class="secondary-action" type="button" disabled>清空</button><span id="ascii-name">PNG · JPG · WebP</span><input id="ascii-file" type="file" accept="image/png,image/jpeg,image/webp" hidden></div>
    <div class="ascii-toolbar"><div role="group" aria-label="预览类型"><button data-ascii-view="source" class="secondary-action" type="button">原图</button><button data-ascii-view="ascii" class="secondary-action selected" type="button">ASCII</button></div><button id="ascii-fit" class="secondary-action" type="button">适应窗口</button><label>缩放 <input id="ascii-zoom" aria-label="ASCII 预览缩放" type="range" min="0.25" max="3" step="0.05" value="1"></label></div>
    <div id="ascii-stage" class="ascii-stage" aria-label="ASCII 图片预览及拖放区域"><div id="ascii-empty"><strong>让图片变成字符画</strong><p>选择或拖入一张图片，实时调整字符与色彩。</p><button id="ascii-empty-import" class="secondary-action" type="button">选择图片</button></div><canvas id="ascii-canvas" hidden aria-label="ASCII 预览"></canvas></div>
    <div class="ascii-toolbar ascii-bottom"><button id="ascii-copy" class="secondary-action" type="button" disabled>复制字符</button><span id="ascii-size"></span></div><p id="ascii-status" role="status"></p>`;
  const controls = document.createElement('div'); controls.className = 'ascii-controls'; controls.hidden = true;
  controls.innerHTML = `<section class="inspector-group" data-modes="ascii ascii-block ascii-dot"><button class="group-toggle" type="button" aria-expanded="true"><span>字符效果</span><i data-lucide="chevron-down"></i></button><div class="group-content">
    <label id="ascii-shape-label" hidden>图形大小 <output id="ascii-shapeSize-value"></output><input id="ascii-shapeSize" type="range" min="20" max="100" step="1"></label>
    <label id="ascii-ratio-label" hidden>长宽比 <output id="ascii-shapeRatio-value"></output><input id="ascii-shapeRatio" type="range" min="50" max="200" step="5"></label>
    <label>颜色模式<select id="ascii-color"><option value="false">黑白字符</option><option value="true">保留原图颜色</option></select></label>
    <label>字符密度 <output id="ascii-columns-value"></output><input id="ascii-columns" type="range" min="40" max="240" step="1"></label>
    <label>字符集<select id="ascii-charset"><option value="standard">标准</option><option value="detailed">详细</option><option value="custom">自定义</option></select></label>
    <label id="ascii-custom-label" hidden>由疏到密的字符<input id="ascii-custom" type="text" maxlength="95" spellcheck="false" aria-describedby="ascii-custom-help"><small id="ascii-custom-help">2–95 个可打印 ASCII 字符；空格也计入。</small></label>
    <label>亮度 <output id="ascii-brightness-value"></output><input id="ascii-brightness" type="range" min="-1" max="1" step="0.05"></label>
    <label>对比度 <output id="ascii-contrast-value"></output><input id="ascii-contrast" type="range" min="0" max="3" step="0.05"></label>
    <label class="ascii-check"><input id="ascii-invert" type="checkbox">反相</label>
    <label>背景<select id="ascii-background"><option value="black">黑底</option><option value="white">白底</option><option value="transparent">透明底（PNG）</option></select></label>
    <button id="ascii-reset" class="secondary-action" type="button">恢复默认</button></div></section>
    <section class="inspector-group" data-modes="ascii ascii-block ascii-dot"><button class="group-toggle" type="button" aria-expanded="true"><span>导出设置</span><i data-lucide="chevron-down"></i></button><div class="group-content"><label>文件格式<select id="ascii-format"><option value="txt">TXT · 纯文本</option><option value="png">PNG · 字符图片</option></select></label><small id="ascii-color-note">TXT 保留字符、空格与换行。</small><small>导出使用完整字符网格，不受预览缩放影响。</small></div></section>`;
  inspector.append(controls);
  for (const el of controls.querySelectorAll('select,input')) el.setAttribute('aria-label', el.type === 'checkbox' ? '反相' : el.closest('label').firstChild.textContent.trim());
  const run = document.createElement('button'); run.id = 'ascii-run'; run.type = 'button'; run.className = 'run-button hidden'; run.innerHTML = '<span>导出 TXT</span>'; runArea.append(run);
  const elements = new Map([...root.querySelectorAll('[id]'), ...controls.querySelectorAll('[id]'), run].map(el => [el.id, el]));
  const $ = key => elements.get(`ascii-${key}`);
  const exportActions = document.createElement('div');
  exportActions.className = 'ascii-export-actions'; exportActions.hidden = true;
  exportActions.append($('copy'), run); runArea.append(exportActions);
  for (const key of ['columns', 'brightness', 'contrast', 'shapeSize', 'shapeRatio']) $(key).closest('label').classList.add('ascii-range');
  const status = text => { $('status').textContent = text; };
  function blocker() { return exporting ? '正在导出…' : importing ? '正在读取图片…' : computing ? '正在生成字符画…' : !result ? '请导入图片并生成有效预览。' : null; }
  function refresh() {
    const locked = exporting || importing;
    const graphic = config.style !== 'ascii';
    for (const el of controls.querySelectorAll('input,select,button')) el.disabled = locked;
    $('format').disabled = locked || graphic;
    $('charset').disabled = locked || graphic;
    $('custom').disabled = locked || graphic;
    controls.querySelectorAll('select').forEach(syncSelect);
    $('import').disabled = locked; $('empty-import').disabled = locked;
    $('clear').disabled = !source || locked;
    $('copy').disabled = !result || computing || locked || graphic;
    run.disabled = Boolean(blocker()) || busy();
    if (!exporting) run.querySelector('span').textContent = `导出 ${config.format.toUpperCase()}`;
    $('custom-label').hidden = config.charset !== 'custom' || graphic;
    $('shape-label').hidden = !graphic;
    $('ratio-label').hidden = !graphic;
    if (graphic) $('color-note').textContent = '方块与波点为图形风格，仅支持导出 PNG；字符复制不可用。';
    else $('color-note').textContent = config.color ? '复制或导出 TXT 只保留字符，不包含颜色；请用 PNG 保存彩色效果。' : 'TXT 保留字符、空格与换行。';
    if (config.background === 'transparent') $('color-note').textContent += ' 透明背景仅保存在 PNG 中；棋盘格不会导出。';
    changed();
  }
  function sync() {
    for (const [key, value] of Object.entries(config)) {
      const el = $(key); if (!el) continue;
      if (el.type === 'checkbox') el.checked = value; else el.value = String(value);
      if ($(`${key}-value`)) $(`${key}-value`).textContent = value;
    }
    refresh();
  }
  function persist() {
    clearTimeout(saveTimer);
    saveTimer = setTimeout(() => {
      const snapshot = { ...config, version: 2 };
      saveChain = saveChain.catch(() => {}).then(() => save(snapshot)).catch(e => { if (!disposed) status(`设置保存失败：${e.message || e}`); });
    }, 250);
  }
  function cancelCompute() { ++revision; clearTimeout(timer); worker?.terminate(); worker = null; computing = false; }
  function sizeCanvas(canvas, data) {
    canvas.width = Math.ceil(data.columns * cellWidth); canvas.height = data.rows * lineHeight;
  }
  function paint(canvas, data) {
    sizeCanvas(canvas, data);
    const ctx = canvas.getContext('2d');
    if (data.settings.background !== 'transparent') { ctx.fillStyle = data.settings.background; ctx.fillRect(0,0,canvas.width,canvas.height); }
    if (data.settings.style === 'block' || data.settings.style === 'dot') { paintShapes(ctx, data); return; }
    ctx.font = font; ctx.textBaseline = 'top';
    if (!data.settings.color && data.settings.background !== 'transparent') {
      ctx.fillStyle = data.settings.background === 'black' ? '#ffffff' : '#000000';
      const lines = data.text.split('\n');
      for (let row = 0; row < data.rows; row++) ctx.fillText(lines[row], 0, row * lineHeight);
    } else {
      for (let i = 0; i < data.chars.length; i++) {
        if (data.chars[i] === ' ') continue;
        ctx.fillStyle = data.settings.color ? `rgb(${data.colors[i*3]},${data.colors[i*3+1]},${data.colors[i*3+2]})` : '#ffffff';
        ctx.globalAlpha = data.alphas[i] / 255;
        ctx.fillText(data.chars[i], (i % data.columns) * cellWidth, Math.floor(i/data.columns) * lineHeight);
      }
    }
  }
  // 方块/波点：每格按亮度映射图形尺寸（方块边长/圆点直径），颜色模式沿用原图 RGB。
  // 长宽比按 sqrt 保面积拉伸：改变形状时视觉权重不跳变。
  function paintShapes(ctx, data) {
    const ink = data.settings.background === 'white' ? '#000000' : '#ffffff';
    const transparent = data.settings.background === 'transparent';
    const cell = Math.min(cellWidth, lineHeight) * (data.settings.shapeSize ?? 92) / 100;
    const stretch = Math.sqrt((data.settings.shapeRatio ?? 100) / 100);
    for (let i = 0; i < data.columns * data.rows; i++) {
      const alpha = data.alphas[i] / 255;
      if (transparent && alpha === 0) continue;
      const size = (data.lights[i] / 255) * cell;
      if (size <= 0) continue;
      const w = size * stretch, h = size / stretch;
      const x = (i % data.columns) * cellWidth, y = Math.floor(i / data.columns) * lineHeight;
      ctx.fillStyle = data.settings.color ? `rgb(${data.colors[i*3]},${data.colors[i*3+1]},${data.colors[i*3+2]})` : ink;
      ctx.globalAlpha = transparent ? alpha : 1;
      if (data.settings.style === 'block') ctx.fillRect(x + (cellWidth - w) / 2, y + (lineHeight - h) / 2, w, h);
      else { ctx.beginPath(); ctx.ellipse(x + cellWidth / 2, y + lineHeight / 2, w / 2, h / 2, 0, 0, Math.PI * 2); ctx.fill(); }
    }
    ctx.globalAlpha = 1;
  }
  function fit() {
    const canvas = $('canvas'); if (canvas.hidden) return;
    const stage = $('stage');
    const scale = Math.min((stage.clientWidth - 32) / canvas.width, (stage.clientHeight - 32) / canvas.height, 1) * zoom;
    canvas.style.width = `${Math.max(1, canvas.width * scale)}px`; canvas.style.height = `${Math.max(1, canvas.height * scale)}px`;
  }
  function draw() {
    root.querySelectorAll('[data-ascii-view]').forEach(el => { const selected = el.dataset.asciiView === view; el.classList.toggle('selected', selected); el.setAttribute('aria-pressed', String(selected)); });
    const viewButton = root.querySelector('[data-ascii-view="ascii"]');
    if (viewButton) viewButton.textContent = { ascii: 'ASCII', block: '方块', dot: '波点' }[config.style] || 'ASCII';
    const canvas = $('canvas'); canvas.hidden = view === 'source' ? !source : !result;
    $('empty').hidden = Boolean(source);
    canvas.classList.toggle('ascii-transparent', view === 'source' || result?.settings.background === 'transparent');
    if (canvas.hidden) return;
    if (view === 'source') { canvas.width = source.width; canvas.height = source.height; canvas.getContext('2d').drawImage(source,0,0); }
    else paint(canvas, result);
    fit();
  }
  function request() {
    cancelCompute(); result = null; $('size').textContent = ''; draw();
    if (!source || !active) { refresh(); return; }
    try { if (config.style === 'ascii') characterRamp(config); } catch (e) { status(e.message); refresh(); return; }
    computing = true; status('正在生成字符画…'); refresh();
    const current = revision, snapshot = { ...config };
    timer = setTimeout(() => {
      try {
        const { columns, rows } = gridSize(source.width, source.height, snapshot.columns, cellWidth, lineHeight);
        // Repeated area reduction avoids point-sampling fine image details.
        let scaled = source;
        while (scaled.width > columns * 2 || scaled.height > rows * 2) {
          const next = document.createElement('canvas'); next.width = Math.max(columns, Math.ceil(scaled.width / 2)); next.height = Math.max(rows, Math.ceil(scaled.height / 2));
          const ctx = next.getContext('2d'); if (snapshot.background !== 'transparent') { ctx.fillStyle = snapshot.background; ctx.fillRect(0,0,next.width,next.height); } ctx.drawImage(scaled,0,0,next.width,next.height); scaled = next;
        }
        const sample = document.createElement('canvas'); sample.width = columns; sample.height = rows;
        const ctx = sample.getContext('2d'); if (snapshot.background !== 'transparent') { ctx.fillStyle = snapshot.background; ctx.fillRect(0,0,columns,rows); } ctx.drawImage(scaled,0,0,columns,rows);
        const pixels = ctx.getImageData(0,0,columns,rows).data;
        worker = new Worker(new URL('./ascii-worker.js', import.meta.url), { type: 'module' });
        const fail = message => { if (current !== revision) return; computing = false; worker?.terminate(); worker = null; status(message); refresh(); };
        worker.onerror = () => fail('字符生成失败，请重新调整参数或导入图片。');
        worker.onmessage = ({data}) => {
          if (data.revision !== revision || !active || disposed) return;
          if (data.error) { fail(data.error); return; }
          worker.terminate(); worker = null; computing = false;
          result = { ...data.result, settings: snapshot };
          result.charSource = snapshot.style === 'ascii' ? 'ascii' : 'graphic';
          $('size').textContent = `${columns} 列 × ${rows} 行 · PNG ${Math.ceil(columns * cellWidth)} × ${rows * lineHeight}${columns !== snapshot.columns ? ' · 已按长图比例限制行数' : ''}`;
          status(alphaAutoSwitch ? '已检测到透明通道，背景自动切换为透明底。' : '预览已更新');
          alphaAutoSwitch = false;
          draw(); refresh();
        };
        worker.postMessage({ revision: current, columns, rows, pixels, settings: snapshot }, [pixels.buffer]);
      } catch (e) { computing = false; status(`生成失败：${e.message || e}`); refresh(); }
    }, 150);
  }
  async function loadFile(file) {
    if (exporting || importing) return;
    if (!/\.(png|jpe?g|webp)$/i.test(typeof file === 'string' ? file : file.name)) { status('请选择 PNG、JPG 或 WebP 图片。'); return; }
    const ticket = ++importRevision; importing = true; cancelCompute(); result = null; refresh(); status('正在读取图片…');
    try {
      const blob = typeof file === 'string' ? await (await fetch(convertFileSrc(file))).blob() : file;
      if (blob.size > 50 * 1024 * 1024) throw Error('图片文件超过 50 MB，请先缩小图片。');
      const bitmap = await createImageBitmap(blob);
      if (ticket !== importRevision || disposed || !active) { bitmap.close(); return; }
      const scale = Math.min(1, 4096 / Math.max(bitmap.width, bitmap.height));
      const decoded = document.createElement('canvas'); decoded.width = Math.max(1, Math.round(bitmap.width * scale)); decoded.height = Math.max(1, Math.round(bitmap.height * scale));
      decoded.getContext('2d').drawImage(bitmap,0,0,decoded.width,decoded.height); bitmap.close();
      source = decoded; sourceName = typeof file === 'string' ? file.split(/[\\/]/).pop() : file.name;
      // 带透明通道的素材（立绘/贴纸）不应被填充黑/白底：自动落到透明底，用户仍可手动改回。
      alphaAutoSwitch = false;
      if (detectTransparency(decoded.getContext('2d').getImageData(0, 0, decoded.width, decoded.height).data) && config.background !== 'transparent') {
        config.background = 'transparent'; $('background').value = 'transparent'; persist(); alphaAutoSwitch = true;
      }
      $('name').textContent = sourceName; $('import').textContent = '替换图片'; zoom = 1; $('zoom').value = 1;
      request();
    } catch (e) { status(`读取失败：${e.message || e}`); }
    finally { importing = false; refresh(); draw(); }
  }
  async function pick() {
    if (importing || exporting) return;
    if (!desktop) { $('file').click(); return; }
    try { const file = await open({ multiple: false, filters: [{name:'图片',extensions:['png','jpg','jpeg','webp']}] }); if (typeof file === 'string') await loadFile(file); }
    catch (e) { status(`选择失败：${e.message || e}`); }
  }
  $('import').onclick = pick; $('empty-import').onclick = pick;
  $('file').onchange = () => { const file = $('file').files[0]; $('file').value = ''; if (file) loadFile(file); };
  root.ondragover = e => { e.preventDefault(); };
  root.ondrop = e => { e.preventDefault(); if (desktop) return; if (e.dataTransfer.files.length !== 1) status('请一次拖入一张图片。'); else loadFile(e.dataTransfer.files[0]); };
  $('clear').onclick = () => { if (exporting || importing) return; cancelCompute(); source = result = null; sourceName = ''; $('name').textContent = 'PNG · JPG · WebP'; $('size').textContent = ''; $('import').textContent = '选择图片'; status(''); draw(); refresh(); };
  for (const key of Object.keys(config)) {
    const el = $(key);
    if (!el) continue; // style 由所在功能模式决定，不在检查器中
    el.addEventListener(el.tagName === 'SELECT' || el.type === 'checkbox' ? 'change' : 'input', () => {
      config[key] = el.type === 'checkbox' ? el.checked : key === 'color' ? el.value === 'true' : ['columns','brightness','contrast'].includes(key) ? Number(el.value) : el.value;
      if ($(`${key}-value`)) $(`${key}-value`).textContent = config[key];
      persist();
      // 图形大小/长宽比只影响绘制，直接重绘无需重新计算字符网格。
      if (key === 'shapeSize' || key === 'shapeRatio') { if (result) result.settings = { ...config }; draw(); refresh(); return; }
      if (key !== 'format') request(); refresh();
    });
  }
  $('reset').onclick = () => { config = { ...DEFAULTS }; sync(); persist(); request(); };
  root.querySelectorAll('[data-ascii-view]').forEach(el => el.onclick = () => { view = el.dataset.asciiView; draw(); });
  $('zoom').oninput = () => { zoom = Number($('zoom').value); fit(); };
  $('fit').onclick = () => { zoom = 1; $('zoom').value = 1; fit(); };
  $('copy').onclick = async () => {
    if (blocker()) return;
    try { await navigator.clipboard.writeText(result.text); status(config.color ? '字符已复制（纯文本不包含颜色）。' : '字符已复制。'); }
    catch { status('复制失败，请导出 TXT 文件。'); }
  };
  run.onclick = async () => {
    if (blocker() || busy()) return;
    const snapshot = result, format = config.format, name = `${sourceName.replace(/\.[^.]+$/, '')}_ascii.${format}`;
    exporting = true; setBusy(run, true); refresh();
    try {
      const path = desktop ? await saveDialog({ defaultPath: name, filters: [{ name: format.toUpperCase(), extensions: [format] }] }) : name;
      if (!path) { status('已取消导出。'); return; }
      let content, blob;
      if (format === 'txt') { content = snapshot.text; blob = new Blob([content], {type:'text/plain;charset=utf-8'}); }
      else { const canvas = document.createElement('canvas'); paint(canvas,snapshot); blob = await new Promise((resolve,reject) => canvas.toBlob(value => value ? resolve(value) : reject(Error('PNG 编码失败。')), 'image/png')); }
      if (desktop) {
        if (format === 'png') content = await new Promise((resolve,reject) => { const reader = new FileReader(); reader.onload = () => resolve(reader.result.split(',')[1]); reader.onerror = () => reject(Error('PNG 读取失败。')); reader.readAsDataURL(blob); });
        await invoke('ascii_export', { path, format, content });
      } else {
        const url = URL.createObjectURL(blob), a = document.createElement('a'); a.href = url; a.download = name; a.hidden = true; document.body.append(a); a.click(); a.remove(); setTimeout(() => URL.revokeObjectURL(url), 1000);
      }
      const outcome = desktop ? '已导出' : '已请求浏览器下载';
      status(`${outcome} ${format.toUpperCase()}${snapshot.settings.color && format === 'txt' ? '（纯文本不包含颜色）' : ''}`); notify(`${outcome} ${name}`, 'success');
    } catch (e) { status(`导出失败：${e.message || e}`); notify(`导出失败：${e.message || e}`, 'error'); }
    finally { exporting = false; setBusy(run,false); refresh(); }
  };
  const resize = new ResizeObserver(fit); resize.observe($('stage'));
  sync();
  return {
    blocker,
    // 风格化三个入口（ascii/ascii-block/ascii-dot）共用本模块与素材，切换只改风格。
    activate(mode) {
      active = mode === 'ascii' || mode === 'ascii-block' || mode === 'ascii-dot';
      controls.hidden = !active; exportActions.hidden = !active;
      if (!active) { ++importRevision; cancelCompute(); return; }
      const style = mode === 'ascii-block' ? 'block' : mode === 'ascii-dot' ? 'dot' : 'ascii';
      if (config.style !== style) {
        config.style = style;
        persist();
        // 图形风格锁定 PNG 导出；切回字符画时 TXT 选项恢复可用但保持当前选择。
        if (style !== 'ascii' && config.format === 'txt') config.format = 'png';
        if (result) result.settings = { ...config };
      }
      if (!source || !result) { request(); return; }
      // 图形↔图形切换无需重算（lights 与颜色与字符无关）；回到字符画需要真实字符。
      if (style === 'ascii' && result.charSource !== 'ascii') request();
      else { draw(); refresh(); }
    },
    addFiles(paths) { if (paths.length === 1) loadFile(paths[0]); else status('请一次拖入一张图片。'); },
    dispose() { disposed = true; ++importRevision; cancelCompute(); clearTimeout(saveTimer); resize.disconnect(); controls.remove(); exportActions.remove(); },
  };
}
