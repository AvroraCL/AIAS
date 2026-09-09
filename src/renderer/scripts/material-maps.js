import { createPreviewQueue, restoreMapSettings } from './material-map-state.mjs';
import './material-maps.css';

function lightRenderer(canvas) {
  const gl = canvas.getContext('webgl', { alpha: false, preserveDrawingBuffer: true });
  if (!gl) return null;
  const compile = (type, code) => {
    const shader = gl.createShader(type); gl.shaderSource(shader, code); gl.compileShader(shader);
    if (!gl.getShaderParameter(shader, gl.COMPILE_STATUS)) { gl.deleteShader(shader); throw new Error('光照着色器初始化失败'); }
    return shader;
  };
  const vs = compile(gl.VERTEX_SHADER, 'attribute vec2 p; varying vec2 uv; void main(){ uv=vec2((p.x+1.0)*0.5,(1.0-p.y)*0.5); gl_Position=vec4(p,0,1); }');
  const fs = compile(gl.FRAGMENT_SHADER, 'precision mediump float; varying vec2 uv; uniform sampler2D normals; uniform vec3 light; uniform float flipY; void main(){vec3 n=texture2D(normals,uv).rgb*2.0-1.0;n.y*=flipY;float d=max(dot(normalize(n),normalize(light)),0.0);vec3 c=vec3(0.65,0.69,0.74)*(0.20+0.80*d);gl_FragColor=vec4(c,1.0);}');
  const program = gl.createProgram(); gl.attachShader(program, vs); gl.attachShader(program, fs); gl.linkProgram(program);
  gl.deleteShader(vs); gl.deleteShader(fs);
  if (!gl.getProgramParameter(program, gl.LINK_STATUS)) { gl.deleteProgram(program); throw new Error('光照初始化失败'); }
  gl.useProgram(program);
  const buffer = gl.createBuffer(); gl.bindBuffer(gl.ARRAY_BUFFER, buffer);
  gl.bufferData(gl.ARRAY_BUFFER, new Float32Array([-1,-1,1,-1,-1,1,-1,1,1,-1,1,1]), gl.STATIC_DRAW);
  const position = gl.getAttribLocation(program, 'p'); gl.enableVertexAttribArray(position); gl.vertexAttribPointer(position, 2, gl.FLOAT, false, 0, 0);
  const texture = gl.createTexture(); gl.bindTexture(gl.TEXTURE_2D, texture);
  gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.LINEAR); gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.LINEAR);
  gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE); gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE);
  return {
    upload(image) { gl.bindTexture(gl.TEXTURE_2D, texture); gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGB, gl.RGB, gl.UNSIGNED_BYTE, image); },
    draw(light, directx) { gl.viewport(0,0,canvas.width,canvas.height); gl.useProgram(program);
      gl.uniform3fv(gl.getUniformLocation(program, 'light'), light);
      gl.uniform1f(gl.getUniformLocation(program, 'flipY'), directx ? -1 : 1); gl.drawArrays(gl.TRIANGLES,0,6); },
    dispose() { gl.deleteTexture(texture); gl.deleteBuffer(buffer); gl.deleteProgram(program); },
  };
}

export function createMaterialMaps({ root, inspector, runArea, syncSelect = () => {}, desktop, invoke, open, openPath, settings, save, busy, withLog, notify }) {
  root.innerHTML = `
    <div class="stage-heading"><div><span>输入资源</span><strong id="map-title">材质贴图</strong></div><small id="map-runtime"></small></div>
    <p class="map-notice">明暗会被解释成表面凹凸，不等同于真实几何高度。光照预览使用固定材质，仅供检查强度和方向。</p>
    <div class="map-layout"><div class="map-main">
      <div class="map-toolbar"><button id="map-add" type="button">添加素材</button><button id="map-clear" type="button">清空</button><span id="map-count">0 张素材</span></div>
      <div id="map-files" class="map-files" aria-label="素材列表"></div>
      <div class="map-toolbar map-tabs" role="group" aria-label="预览类型"><button type="button" data-map-view="source">原图</button><button type="button" data-map-view="generated">生成贴图</button><button type="button" data-map-view="light">光照效果</button><button id="map-retry" type="button">刷新预览</button></div>
      <div id="map-stage" class="map-stage"><p id="map-empty">添加素材后查看贴图与光照预览</p><canvas id="map-2d" hidden aria-label="素材贴图预览"></canvas><canvas id="map-light" hidden aria-label="拖动调整光源"></canvas></div>
      <div class="map-toolbar"><label>缩放 <input id="map-zoom" type="range" min="0.25" max="3" step="0.05" value="1"></label><button id="map-reset" type="button">复位视图与光源</button></div>
      <p id="map-preview-status" role="status"></p>
    </div><aside class="map-controls">
      <label>高度来源<select id="map-channel"><option value="luminance">亮度／灰度数据</option><option value="r">R 通道</option><option value="g">G 通道</option><option value="b">B 通道</option><option value="alpha">Alpha 通道</option></select></label>
      <label>平滑半径 <output id="map-smoothing-value">0</output><input id="map-smoothing" type="range" min="0" max="20" step="1" value="0"></label>
      <label>对比度 <output id="map-contrast-value">1</output><input id="map-contrast" type="range" min="0" max="4" step="0.05" value="1"></label>
      <label class="map-check"><input id="map-invert" type="checkbox">反相（默认白色凸起）</label>
      <label>边界模式<select id="map-boundary"><option value="clamp">普通素材 · 边缘钳制</option><option value="wrap">平铺纹理 · 循环采样</option></select></label>
      <label>法线强度 <output id="map-strength-value">1</output><input id="map-strength" type="range" min="0" max="10" step="0.1" value="1"></label>
      <label>法线方向<select id="map-convention"><option value="opengl">OpenGL（+Y）</option><option value="directx">DirectX（−Y）</option></select></label>
      <label>高度位深<select id="map-bits"><option value="16">16 位灰度 PNG</option><option value="8">8 位灰度 PNG</option></select></label>
      <label id="map-also-label" class="map-check"><input id="map-alsoHeight" type="checkbox">同时输出高度图</label>
      <label>输出目录<input id="map-output" readonly placeholder="请选择输出目录"></label><button id="map-pick-output" type="button">选择目录</button>
      <button id="map-run" type="button" class="run-button">生成 PNG</button><button id="map-open-output" type="button">打开输出目录</button>
      <p id="map-export-status" role="status"></p><small>透明区域输出中性高度及平坦法线。数据贴图不附加源图 ICC。缩略预览供参考，导出使用原始尺寸。</small>
    </aside></div>`;
  // Keep the existing application inspector and run area as the single layout.
  const controls = root.querySelector('.map-controls');
  const elements = new Map([...root.querySelectorAll('[id]')].map(element => [element.id, element]));
  const $ = id => elements.get(id);
  const groups = [
    ['高度调整', ['channel','smoothing','contrast','invert','boundary']],
    ['法线设置', ['strength','convention']],
    ['输出设置', ['bits','alsoHeight']],
  ];
  for (const [title, keys] of groups) {
    const section = document.createElement('section'); section.className = 'inspector-group'; section.dataset.modes = 'normal-map height-map';
    section.innerHTML = `<button class="group-toggle" type="button" aria-expanded="true"><span>${title}</span><i data-lucide="chevron-down" aria-hidden="true"></i></button><div class="group-content"></div>`;
    for (const key of keys) section.lastElementChild.append($(`map-${key}`).closest('label'));
    controls.append(section);
  }
  const output = document.createElement('section'); output.className = 'inspector-group'; output.dataset.modes = 'normal-map height-map';
  output.innerHTML = '<button class="group-toggle" type="button" aria-expanded="true"><span>导出文件夹</span><i data-lucide="chevron-down" aria-hidden="true"></i></button><div class="group-content"></div>';
  output.lastElementChild.append($('map-output').closest('label'));
  $('map-output').closest('label').htmlFor = 'map-output';
  $('map-output').setAttribute('aria-label', '材质输出文件夹');
  const row = document.createElement('div'); row.className = 'field-row'; row.append($('map-output'), $('map-pick-output'));
  $('map-pick-output').textContent = '选择'; output.lastElementChild.append(row, $('map-open-output'), $('map-export-status'), controls.querySelector('small'));
  controls.append(output);
  if (inspector) inspector.append(controls);
  if (runArea) runArea.insertBefore($('map-run'), runArea.querySelector('#task-progress'));
  for (const label of controls.querySelectorAll('.map-check')) {
    label.className = 'toggle-label';
    const input = label.querySelector('input');
    input.insertAdjacentHTML('afterend', '<span class="toggle-track" aria-hidden="true"><span class="toggle-thumb"></span></span>');
  }
  for (const select of controls.querySelectorAll('select')) select.setAttribute('aria-label', select.closest('label').firstChild.textContent.trim());
  for (const range of controls.querySelectorAll('input[type="range"]')) {
    const label = range.closest('label'); label.classList.add('map-range');
    range.setAttribute('aria-label', label.firstChild.textContent.trim());
  }
  for (const button of root.querySelectorAll('.map-toolbar button')) button.classList.add('secondary-action');
  $('map-add').innerHTML = '<i data-lucide="image-plus" aria-hidden="true"></i>添加素材';
  $('map-clear').innerHTML = '<i data-lucide="trash-2" aria-hidden="true"></i>清空';
  $('map-open-output').className = 'secondary-action output-action';
  $('map-open-output').innerHTML = '<i data-lucide="folder-open" aria-hidden="true"></i>打开输出目录';
  let stored = restoreMapSettings(settings), kind = 'normal', files = [], selected = '', preview = null;
  let view = 'generated', light = [0.4, 0.3, 1], exporting = false, active = false, bitmapRevision = 0, disposed = false;
  let renderer;
  try { renderer = lightRenderer($('map-light')); } catch { renderer = null; }
  $('map-runtime').textContent = desktop ? '本地处理' : '网页演示 · 请在桌面软件中导入并生成';
  if (!renderer) { root.querySelector('[data-map-view="light"]').disabled = true; $('map-preview-status').textContent = 'WebGL 不可用，已使用二维贴图预览。'; }
  function parameters() { return stored[kind].parameters; }
  function message(error) { return error?.message || String(error); }
  let saveTimer, saveChain = Promise.resolve();
  function persist() {
    clearTimeout(saveTimer);
    saveTimer = setTimeout(() => {
      const snapshot = structuredClone(stored);
      saveChain = saveChain.catch(() => {}).then(() => save(snapshot)).catch(error => { if (!disposed) $('map-export-status').textContent = `设置保存失败：${message(error)}`; });
    }, 200);
  }
  function updateRun() {
    $('map-run').disabled = !desktop || !files.length || !stored[kind].outputPath || busy() || exporting;
    for (const control of [...controls.querySelectorAll('input,select'), $('map-add'), $('map-clear'), $('map-pick-output')]) control.disabled = exporting;
    controls.querySelectorAll('select').forEach(syncSelect);
  }
  function renderFiles() {
    $('map-count').textContent = `${files.length} 张素材`;
    $('map-files').replaceChildren();
    for (const file of files) {
      const item = document.createElement('div'); item.className = 'map-file';
      const button = document.createElement('button'); button.className = 'secondary-action'; button.type = 'button'; button.textContent = file.split(/[\\/]/).pop(); button.title = file;
      button.classList.toggle('selected', file === selected); button.setAttribute('aria-pressed', String(file === selected));
      button.onclick = () => { if (!exporting) { selected = file; renderFiles(); requestPreview(); } };
      const remove = document.createElement('button'); remove.className = 'icon-button'; remove.type = 'button'; remove.textContent = '×'; remove.setAttribute('aria-label', `移除 ${button.textContent}`);
      remove.onclick = () => { if (exporting) return; files = files.filter(value => value !== file); if (selected === file) selected = files[0] || ''; renderFiles(); requestPreview(); };
      item.append(button,remove); $('map-files').append(item);
    }
    updateRun();
  }
  function clearPreview() {
    ++bitmapRevision;
    if (preview) for (const image of Object.values(preview.images)) image.close?.();
    preview = null; $('map-2d').hidden = true; $('map-light').hidden = true; $('map-empty').hidden = false;
  }
  function draw() {
    root.querySelectorAll('[data-map-view]').forEach(button => { button.classList.toggle('selected', button.dataset.mapView === view); button.setAttribute('aria-pressed', String(button.dataset.mapView === view)); });
    if (!preview) return;
    $('map-empty').hidden = true;
    const showLight = view === 'light' && renderer;
    $('map-light').hidden = !showLight; $('map-2d').hidden = Boolean(showLight);
    const canvas = showLight ? $('map-light') : $('map-2d');
    const stage = $('map-stage');
    const fit = Math.min(1, (stage.clientWidth - 32) / preview.width, (stage.clientHeight - 32) / preview.height);
    const displayWidth = Math.max(1, preview.width * fit) * Number($('map-zoom').value);
    canvas.style.width = `${displayWidth}px`; canvas.style.height = 'auto';
    if (showLight) renderer.draw(light, preview.convention === 'directx');
    else { const ctx = canvas.getContext('2d'); ctx.clearRect(0,0,canvas.width,canvas.height); ctx.drawImage(preview.images[view === 'source' ? 'source' : kind],0,0); }
  }
  const queue = createPreviewQueue({
    run: async input => ({ data: await invoke('material_maps_preview', input), convention: input.parameters.convention }),
    ready: async ({ data, convention }) => {
      const revision = bitmapRevision;
      const images = {};
      try {
        for (const key of ['source','normal','height']) images[key] = await createImageBitmap(await (await fetch(data[key])).blob());
      } catch (error) { Object.values(images).forEach(image => image.close?.()); throw error; }
      if (disposed || revision !== bitmapRevision || !active) { Object.values(images).forEach(image => image.close?.()); return; }
      clearPreview(); preview = { images, width: data.width, height: data.heightPixels, convention };
      for (const canvas of [$('map-2d'),$('map-light')]) { canvas.width = preview.width; canvas.height = preview.height; }
      renderer?.upload(images.normal);
      $('map-preview-status').textContent = `${data.width} × ${data.heightPixels} 预览 · 导出保持原始尺寸${renderer ? ' · 光照视图可拖动光源' : ' · WebGL 不可用'}`;
      draw();
    },
    failed: error => { clearPreview(); $('map-preview-status').textContent = `预览失败：${message(error)}`; },
  });
  function requestPreview() {
    queue.invalidate(); clearPreview();
    if (!selected || !active) { $('map-preview-status').textContent = ''; return; }
    if (!desktop) { $('map-preview-status').textContent = '网页演示不执行本地贴图转换，请打开桌面软件。'; return; }
    if (busy() || exporting) { $('map-preview-status').textContent = '任务运行中，结束后可刷新预览。'; return; }
    $('map-preview-status').textContent = '正在生成预览…';
    queue.request({ input: selected, parameters: structuredClone(parameters()) });
  }
  function syncControls() {
    for (const [key,value] of Object.entries(parameters())) {
      const control = $(`map-${key}`); if (!control) continue;
      if (control.type === 'checkbox') control.checked = value; else control.value = value;
      if (control.tagName === 'SELECT') syncSelect(control);
      if ($(`map-${key}-value`)) $(`map-${key}-value`).textContent = value;
    }
    $('map-output').value = stored[kind].outputPath;
    $('map-title').textContent = '材质贴图';
    $('map-also-label').hidden = kind !== 'normal';
    $('map-run').innerHTML = `<svg viewBox="0 0 24 24" width="16" height="16" fill="none" stroke="currentColor" stroke-width="2" aria-hidden="true"><path d="m8 5 11 7-11 7z"/></svg><span>${kind === 'normal' ? '生成法线图' : '生成高度图'}</span>`; updateRun();
  }
  for (const key of Object.keys(parameters())) {
    const control = $(`map-${key}`);
    control.addEventListener(control.tagName === 'SELECT' ? 'change' : 'input', () => {
      parameters()[key] = control.type === 'checkbox' ? control.checked : ['smoothing','contrast','strength','bits'].includes(key) ? Number(control.value) : control.value;
      if ($(`map-${key}-value`)) $(`map-${key}-value`).textContent = parameters()[key];
      persist(); requestPreview();
    });
  }
  function addFiles(paths) {
    if (exporting || !desktop) return;
    const accepted = paths.filter(path => /\.(png|jpe?g|webp|tga)$/i.test(path));
    files = [...new Set([...files, ...accepted])]; selected ||= files[0] || '';
    renderFiles(); requestPreview();
  }
  $('map-add').onclick = async () => {
    if (!desktop) { $('map-preview-status').textContent = '请在桌面软件中添加本地素材，网页演示不会生成文件。'; return; }
    try { const result = await open({ multiple: true, filters: [{ name: '图片素材', extensions: ['png','jpg','jpeg','webp','tga'] }] }); if (result) addFiles(Array.isArray(result) ? result : [result]); }
    catch (error) { $('map-preview-status').textContent = message(error); }
  };
  $('map-clear').onclick = () => { files = []; selected = ''; renderFiles(); requestPreview(); };
  $('map-pick-output').onclick = async () => {
    if (!desktop) { $('map-export-status').textContent = '网页演示不能选择本地输出目录。'; return; }
    const currentKind = kind;
    try { const path = await open({ directory: true, multiple: false }); if (typeof path === 'string') { stored[currentKind].outputPath = path; syncControls(); persist(); } }
    catch (error) { $('map-export-status').textContent = message(error); }
  };
  $('map-open-output').onclick = async () => {
    if (!desktop || !stored[kind].outputPath) return;
    try { await openPath(stored[kind].outputPath); } catch (error) { $('map-export-status').textContent = message(error); }
  };
  $('map-run').onclick = async () => {
    if (!desktop || exporting || busy() || !files.length || !stored[kind].outputPath) return;
    const options = { files: [...files], kind, outputPath: stored[kind].outputPath, parameters: structuredClone(parameters()) };
    exporting = true; updateRun();
    try {
      await queue.settle();
      const result = await withLog('material-maps-log', $('map-run'), () => invoke('material_maps_generate', { options }), kind === 'normal' ? '生成法线图' : '生成高度图');
      $('map-export-status').textContent = result ? `成功 ${result.completed}/${result.total} 张素材 · 输出 ${result.outputs?.length || 0} 个文件` : '生成失败或任务忙，请查看活动日志。';
    } catch (error) { $('map-export-status').textContent = message(error); notify(message(error)); }
    finally { exporting = false; syncControls(); requestPreview(); }
  };
  root.querySelectorAll('[data-map-view]').forEach(button => button.onclick = () => { view = button.dataset.mapView; draw(); });
  $('map-retry').onclick = requestPreview;
  $('map-zoom').oninput = draw;
  $('map-reset').onclick = () => { light = [0.4,0.3,1]; $('map-zoom').value = '1'; draw(); };
  const canvas = $('map-light');
  const moveLight = event => { const rect = canvas.getBoundingClientRect(); light = [(event.clientX-rect.left)/rect.width*2-1, 1-(event.clientY-rect.top)/rect.height*2, 0.8]; draw(); };
  canvas.onpointerdown = event => { canvas.setPointerCapture(event.pointerId); moveLight(event); };
  canvas.onpointermove = event => { if (canvas.hasPointerCapture(event.pointerId)) moveLight(event); };
  canvas.onpointerup = event => { if (canvas.hasPointerCapture(event.pointerId)) canvas.releasePointerCapture(event.pointerId); };
  canvas.addEventListener('webglcontextlost', event => { event.preventDefault(); renderer = null; view = 'generated'; root.querySelector('[data-map-view="light"]').disabled = true; $('map-preview-status').textContent = '光照上下文丢失，已退回二维贴图预览。'; draw(); });
  const resize = new ResizeObserver(draw); resize.observe($('map-stage'));
  const interval = setInterval(updateRun, 500);
  syncControls();
  return {
    blocker() { return !desktop ? '请在桌面软件中生成贴图。' : exporting || busy() ? '任务正在运行。' : !files.length ? '请添加素材。' : !stored[kind].outputPath ? '请选择输出目录。' : null; },
    activate(mode) { active = mode === 'normal-map' || mode === 'height-map'; controls.hidden = !active; if (active) { kind = mode === 'normal-map' ? 'normal' : 'height'; syncControls(); } requestPreview(); },
    addFiles,
    dispose() { disposed = true; queue.dispose(); clearPreview(); clearTimeout(saveTimer); clearInterval(interval); resize.disconnect(); renderer?.dispose(); controls.remove(); $('map-run').remove(); },
  };
}
