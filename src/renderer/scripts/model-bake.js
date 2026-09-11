import * as THREE from 'three';
import { OrbitControls } from 'three/addons/controls/OrbitControls.js';
import './model-bake.css';
import { bakeDefaults as defaults, restoreBakeSettings } from './model-bake-state.mjs';

export function createModelBake({ root, inspector, runArea, desktop, invoke, open, openPath, convertFileSrc, listen, settings, save, busy, withLog, notify, syncSelect = () => {}, progress }) {
  const stored = restoreBakeSettings(settings);
  let model = null, geometry = null, active = false, running = false, loading = false, disposed = false, job = '', view = 'model';
  let objects = new Set(), materials = new Set(), channels = {}, focused = null, results = [], renderer, controls, scene, camera, group;
  let saveTimer, renderFrame, unlisten, outputDirectory = '', reportRevision = 0;
  root.innerHTML = `<div class="bake-toolbar"><button id="bake-import" class="secondary-action">导入模型</button><button id="bake-reset" class="secondary-action">复位视图</button><span id="bake-model-info">OBJ · GLB · glTF 静态三角网格</span></div>
    <div class="bake-layout"><aside class="bake-selection"><h3>对象</h3><div id="bake-objects"></div><h3>输出材质</h3><div id="bake-materials"></div></aside>
    <div class="bake-work"><div class="bake-tabs"><button data-bake-view="model" class="secondary-action">三维模型</button><button data-bake-view="uv" class="secondary-action">UV 检查</button><button data-bake-view="results" class="secondary-action">结果</button></div>
    <div id="bake-stage"><div id="bake-three"></div><canvas id="bake-uv-canvas" hidden aria-label="UV 检查，红色标记问题面"></canvas><div id="bake-results" hidden></div><p id="bake-empty">导入模型后检查 UV，并按材质生成贴图</p></div><p id="bake-status" role="status"></p><div id="bake-issues"></div></div></div>`;
  const panel = document.createElement('div'); panel.className = 'bake-controls'; panel.hidden = true;
  const section = (name, html) => `<section class="inspector-group" data-modes="model-bake"><button class="group-toggle" type="button" aria-expanded="true"><span>${name}</span><i data-lucide="chevron-down"></i></button><div class="group-content">${html}</div></section>`;
  const select = (id, title, entries) => `<label>${title}<select id="bake-${id}" aria-label="${title}">${entries.map(([v, t]) => `<option value="${v}">${t}</option>`).join('')}</select></label>`;
  const check = (id, title) => `<label class="toggle-label"><input id="bake-${id}" type="checkbox"><span class="toggle-track"><span class="toggle-thumb"></span></span>${title}</label>`;
  panel.innerHTML = section('计算设备', `${select('device', 'GPU', [[0, '检测设备中…']])}<small id="bake-device-note"></small>`)
    + section('UV 与输出', `<p id="bake-focused">选择材质检查 UV</p>${select('channel', '当前材质 UV 通道', [[0, 'UV0']])}${check('ao', 'GPU 环境遮蔽 AO')}${check('uv', '透明 UV 线框')}${check('id', '材质 ID 与颜色表')}`)
    + section('烘焙质量', `${select('resolution', '分辨率', [512, 1024, 2048, 4096].map(v => [v, `${v} × ${v}`]))}${select('samples', 'AO 采样', [32, 64, 128, 256].map(v => [v, `${v} 次`]))}${select('bits', 'AO 位深', [[8, '8 位线性灰度'], [16, '16 位线性灰度']])}<label>边缘扩展（px）<input id="bake-margin" type="number" min="0" max="128" value="16"></label>`)
    + section('遮蔽范围', `<label>遮蔽距离<input id="bake-distance" type="number" min="0.000001" step="any" value="1"></label><small id="bake-distance-note">默认包围盒对角线的 10%</small>${select('selfOnly', '遮挡对象', [['false', '所选对象相互遮挡'], ['true', '仅自身遮挡']])}<small>按不透明几何计算，不读取透明贴图。</small>`)
    + section('导出文件夹', `<div class="field-row"><input id="bake-output" aria-label="烘焙输出目录" readonly placeholder="请选择目录"><button id="bake-pick-output">选择</button></div><button id="bake-open-output" class="secondary-action output-action">打开结果目录</button>`);
  inspector.append(panel);
  const run = document.createElement('button'); run.id = 'bake-run'; run.className = 'run-button hidden'; run.textContent = '开始烘焙';
  const cancel = document.createElement('button'); cancel.id = 'bake-cancel'; cancel.className = 'secondary-action'; cancel.textContent = '取消烘焙'; cancel.hidden = true;
  runArea.insertBefore(run, runArea.querySelector('#task-progress')); runArea.insertBefore(cancel, runArea.querySelector('#task-progress'));
  const $ = id => document.getElementById(`bake-${id}`);
  for (const key of Object.keys(defaults)) { const el = $(key); if (!el) continue; if (el.type === 'checkbox') el.checked = Boolean(stored[key]); else el.value = String(stored[key]); }
  let devices = [];
  const status = text => { $('status').textContent = text; };
  const persist = () => { clearTimeout(saveTimer); saveTimer = setTimeout(() => save({ ...stored }).catch(notify), 250); };
  function blocker() {
    if (!desktop) return '请在桌面软件中导入模型并烘焙。';
    if (running || loading || busy()) return '任务正在运行。';
    if (!model) return '请导入模型。';
    if (!objects.size || !materials.size) return '请选择对象和输出材质。';
    if (!stored.ao && !stored.uv && !stored.id) return '请选择输出类型。';
    if (!stored.output) return '请选择输出目录。';
    if (stored.ao && !devices.find(d => d.index === +stored.device)?.supported) return '当前设备不支持 DXR 1.1 AO。';
    return null;
  }
  function refresh() {
    const locked = running || loading || busy();
    root.querySelectorAll('input,button').forEach(el => { el.disabled = locked && !el.dataset.bakeView && el.id !== 'bake-reset'; });
    panel.querySelectorAll('input,select,button').forEach(el => { el.disabled = locked; });
    const device = devices.find(d => d.index === +stored.device);
    $('ao').disabled = locked || !device?.supported;
    $('device-note').textContent = !desktop ? '桌面版可检测 DXR 显卡' : device ? `${device.supported ? 'DXR 1.1 可用' : device.reason} · 可用预算 ${(device.availableBytes / 1073741824).toFixed(1)} GiB${[4098, 32902].includes(device.vendor) ? '；按能力支持，待硬件实测' : ''}` : '无合格显卡；仍可导出 UV／ID';
    run.disabled = Boolean(blocker()); run.title = blocker() || '';
    cancel.hidden = !running || !active;
    panel.querySelectorAll('select').forEach(syncSelect);
  }
  async function capabilities() {
    if (!desktop) { $('device').replaceChildren(new Option('请使用桌面版', '0')); refresh(); return; }
    try { devices = (await invoke('bake_capabilities')).devices || []; const el = $('device'); el.replaceChildren();
      for (const d of devices) el.add(new Option(`${d.name}${d.supported ? '' : '（不支持 AO）'}`, d.index));
      if (!devices.some(d => d.index === +stored.device)) stored.device = devices.find(d => d.supported)?.index ?? devices[0]?.index ?? 0;
      el.value = String(stored.device); if (!devices.find(d => d.index === +stored.device)?.supported) { stored.ao = false; $('ao').checked = false; }
    } catch (e) { status(String(e)); $('device').replaceChildren(new Option('设备检测失败', '0')); stored.ao = false; $('ao').checked = false; }
    refresh();
  }
  function init3d() {
    if (renderer) return;
    try {
      renderer = new THREE.WebGLRenderer({ antialias: true, alpha: true }); renderer.setPixelRatio(Math.min(devicePixelRatio, 2)); $('three').append(renderer.domElement);
      scene = new THREE.Scene(); camera = new THREE.PerspectiveCamera(45, 1, 0.001, 1e7); controls = new OrbitControls(camera, renderer.domElement); controls.enableDamping = true;
      scene.add(new THREE.HemisphereLight(0xffffff, 0x545d70, 2)); const light = new THREE.DirectionalLight(0xffffff, 2.5); light.position.set(3, 5, 4); scene.add(light); group = new THREE.Group(); scene.add(group);
      const animate = () => { if (disposed) return; renderFrame = requestAnimationFrame(animate); if (!active || view !== 'model') return; const w = $('three').clientWidth, h = $('three').clientHeight; if (!w || !h) return; renderer.setSize(w, h, false); camera.aspect = w / h; camera.updateProjectionMatrix(); controls.update(); renderer.render(scene, camera); }; animate();
    } catch (e) { status(`三维预览不可用：${e}。UV 检查与烘焙仍可使用。`); }
  }
  function clearMeshes() { if (!group) return; for (const mesh of [...group.children]) { mesh.geometry.dispose(); mesh.material.aoMap?.dispose(); mesh.material.dispose(); group.remove(mesh); } }
  function buildMeshes() {
    init3d(); if (!group) return; clearMeshes();
    const batches = new Map();
    for (const t of geometry.triangles) { if (!objects.has(t.object)) continue; const key = `${t.object}:${t.material}`; if (!batches.has(key)) batches.set(key, { positions: [], normals: [], uvs: [], material: t.material }); const b = batches.get(key); b.positions.push(...t.positions.flat()); b.normals.push(...t.normals.flat()); b.uvs.push(...(t.uvs[channels[t.material] ?? 0] || [[0, 0], [0, 0], [0, 0]]).flat()); }
    for (const b of batches.values()) { const g = new THREE.BufferGeometry(); g.setAttribute('position', new THREE.Float32BufferAttribute(b.positions, 3)); g.setAttribute('normal', new THREE.Float32BufferAttribute(b.normals, 3)); g.setAttribute('uv', new THREE.Float32BufferAttribute(b.uvs, 2)); const mat = new THREE.MeshStandardMaterial({ color: 0xaeb6c3, roughness: 0.85, side: THREE.DoubleSide }); const mesh = new THREE.Mesh(g, mat); mesh.userData.material = b.material; group.add(mesh); }
  }
  function reset() { if (!model || !camera) return; const box = new THREE.Box3().setFromObject(group); const center = box.getCenter(new THREE.Vector3()); const size = box.getSize(new THREE.Vector3()).length() || 1; camera.near = Math.max(size / 10000, 0.000001); camera.far = size * 100; camera.position.copy(center).add(new THREE.Vector3(0.8, 0.6, 1).multiplyScalar(size)); controls.target.copy(center); controls.update(); }
  function selectedBounds() { const bounds = [new THREE.Vector3(Infinity, Infinity, Infinity), new THREE.Vector3(-Infinity, -Infinity, -Infinity)]; for (const t of geometry.triangles) if (objects.has(t.object)) for (const p of t.positions) { const v = new THREE.Vector3(...p); bounds[0].min(v); bounds[1].max(v); } return bounds[0].distanceTo(bounds[1]); }
  function focusMaterial(id) {
    focused = id; const m = model.materials.find(m => m.id === id); $('focused').textContent = `材质 ${id} · ${m.name}`;
    $('channel').replaceChildren(...m.channels.map(c => new Option(`UV${c.channel}`, c.channel))); $('channel').value = String(channels[id] ?? 0); syncSelect($('channel')); drawUv();
    $('materials').querySelectorAll('button').forEach(b => b.classList.toggle('active', +b.dataset.material === id));
  }
  function lists() {
    for (const [name, items, selected] of [['objects', model.objects, objects], ['materials', model.materials, materials]]) { $(name).replaceChildren();
      for (const item of items) { const row = document.createElement('div'); row.className = 'bake-select-row'; const input = document.createElement('input'); input.type = 'checkbox'; input.checked = selected.has(item.id); input.setAttribute('aria-label', `${name === 'objects' ? '对象' : '材质'} ${item.id} ${item.name}`);
        input.onchange = () => { if (input.checked) selected.add(item.id); else selected.delete(item.id); if (name === 'objects') { buildMeshes(); $('distance').value = String(Math.max(selectedBounds() * stored.distanceRatio, 0.000001)); refreshReports(); } refresh(); };
        const label = document.createElement('button'); label.className = 'bake-item'; label.textContent = `${item.id} · ${item.name}`; label.dataset.material = item.id; label.onclick = () => { if (name === 'materials') focusMaterial(item.id); }; row.append(input, label); $(name).append(row);
      }
    }
  }
  async function refreshReports() {
    const revision = ++reportRevision;
    try { const reports = await invoke('bake_inspect', { handle: model.handle, objects: [...objects], channels }); if (revision !== reportRevision || disposed) return;
      for (const report of reports) { const m = model.materials.find(m => m.id === report.material); const i = m.channels.findIndex(c => c.channel === report.channel); if (i >= 0) m.channels[i] = report; else m.channels.push(report); } drawUv();
    } catch (e) { status(String(e)); }
  }
  function drawUv(highlight = null) {
    if (!model || focused === null) return; const canvas = $('uv-canvas'); const n = Math.max(256, Math.min(900, $('stage').clientWidth - 24)); canvas.width = canvas.height = n; const ctx = canvas.getContext('2d'); ctx.fillStyle = '#171c24'; ctx.fillRect(0, 0, n, n);
    const report = model.materials.find(m => m.id === focused)?.channels.find(c => c.channel === (channels[focused] ?? 0)); const bad = new Set((report?.issues || []).flatMap(i => [i.triangle, i.otherTriangle]).filter(i => i != null));
    geometry.triangles.forEach((t, index) => { if (t.material !== focused || !objects.has(t.object)) return; const uv = t.uvs[channels[focused] ?? 0]; if (!uv) return; ctx.beginPath(); uv.forEach((p, i) => { const x = 12 + p[0] * (n - 24), y = 12 + (1 - p[1]) * (n - 24); if (i) ctx.lineTo(x, y); else ctx.moveTo(x, y); }); ctx.closePath(); ctx.strokeStyle = highlight === index ? '#ffd166' : bad.has(index) ? '#ff6278' : '#8296ae'; ctx.lineWidth = highlight === index ? 3 : 1; if (bad.has(index)) { ctx.fillStyle = '#ff627833'; ctx.fill(); } ctx.stroke(); });
    $('issues').replaceChildren(); if (report?.issues?.length) { const note = document.createElement('p'); note.textContent = `${report.issueCount} 处 UV 问题；该材质的 AO／ID 会被阻止，UV 线框仍可导出。`; $('issues').append(note); for (const issue of report.issues.slice(0, 30)) { const button = document.createElement('button'); button.className = 'bake-issue'; button.textContent = `对象 ${issue.object} · 源面 ${issue.face}：${issue.kind}${issue.otherTriangle != null ? `（与三角形 ${issue.otherTriangle}）` : ''}`; button.onclick = () => { setView('uv'); drawUv(issue.triangle); }; $('issues').append(button); } } else { $('issues').textContent = report ? '当前材质 UV 检查通过。' : 'UV 检查中…'; }
  }
  function setView(next) { view = next; $('three').hidden = next !== 'model'; $('uv-canvas').hidden = next !== 'uv'; $('results').hidden = next !== 'results'; $('issues').hidden = next !== 'uv'; root.querySelectorAll('[data-bake-view]').forEach(b => b.classList.toggle('active', b.dataset.bakeView === next)); if (next === 'uv') drawUv(); }
  async function importModel(path) {
    if (!desktop || running || loading || busy()) return; loading = true; refresh(); status('正在导入并检查模型…');
    try { const data = await invoke('bake_import', { path }); const response = await fetch(convertFileSrc(data.meshPath)); if (!response.ok) throw Error('无法读取规范化网格'); const mesh = await response.json();
      if (model) await invoke('bake_release', { handle: model.handle }); model = data; geometry = mesh; objects = new Set(model.objects.map(o => o.id)); materials = new Set(model.materials.map(m => m.id)); channels = Object.fromEntries(model.materials.map(m => [m.id, 0])); results = []; $('results').replaceChildren();
      $('model-info').textContent = `${model.name} · ${model.objects.length} 对象 · ${model.triangleCount.toLocaleString()} 三角形`; $('empty').hidden = true; lists(); buildMeshes(); reset(); $('distance').value = String(selectedBounds() * stored.distanceRatio); $('distance-note').textContent = `单位：${model.units}；所选包围盒对角线的 ${(stored.distanceRatio * 100).toFixed(1)}%`; focusMaterial(model.materials[0].id); setView('model'); status('模型已导入。检查材质 UV 后开始烘焙。');
    } catch (e) { status(`导入失败：${e}`); notify(e); } finally { loading = false; refresh(); }
  }
  function showResults(data) {
    results = data.files || []; outputDirectory = data.directory; $('results').replaceChildren();
    for (const file of results) { const card = document.createElement('div'); card.className = 'bake-result'; const title = document.createElement('p'); title.textContent = `材质 ${file.material} · ${file.kind.toUpperCase()}`; const image = document.createElement('img'); image.src = convertFileSrc(file.path); image.alt = title.textContent; card.append(image, title);
      if (file.kind === 'ao') { const button = document.createElement('button'); button.className = 'secondary-action'; button.textContent = '应用 AO 到模型'; button.onclick = () => { if (!group) return; new THREE.TextureLoader().load(convertFileSrc(file.path), texture => { texture.colorSpace = THREE.NoColorSpace; for (const mesh of group.children.filter(m => m.userData.material === file.material)) { mesh.material.aoMap?.dispose(); mesh.material.aoMap = texture.clone(); mesh.material.aoMap.needsUpdate = true; mesh.material.aoMapIntensity = 1; mesh.material.needsUpdate = true; } texture.dispose(); setView('model'); }); }; card.append(button); } $('results').append(card);
    }
    setView('results'); status(`${data.cancelled ? '已取消' : data.failures?.length ? '部分完成' : '烘焙完成'} · ${results.length} 张贴图${data.elapsedMs != null ? ` · ${(data.elapsedMs / 1000).toFixed(1)} 秒` : ''}${data.failures?.length ? `。${data.failures.join('；')}` : ''}`);
  }
  run.onclick = async () => {
    if (blocker()) return; job = crypto.randomUUID(); running = true; refresh();
    try { await withLog('model-bake-log', run, async () => { const types = ['ao', 'uv', 'id'].filter(k => stored[k]); const data = await invoke('bake_start', { handle: model.handle, jobId: job, options: { ...stored, device: +stored.device, objects: [...objects], materials: [...materials], channels: { ...channels }, distance: +$('distance').value } }); showResults(data); return { completed: data.files?.length || 0, total: materials.size * types.length, logs: [...(data.files || []).map(f => f.path), ...(data.failures || []), ...(data.cancelled ? ['任务已取消，已完成文件保留。'] : [])] }; }, '模型烘焙'); }
    finally { running = false; job = ''; refresh(); }
  };
  cancel.onclick = async () => { cancel.disabled = true; try { await invoke('bake_cancel', { jobId: job }); status('正在取消，保留已完整写入的结果…'); } catch (e) { notify(e); } finally { cancel.disabled = false; } };
  $('import').onclick = async () => { if (!desktop) return; const path = await open({ multiple: false, filters: [{ name: '静态模型', extensions: ['obj', 'glb', 'gltf'] }] }); if (path) await importModel(path); };
  $('reset').onclick = reset;
  $('pick-output').onclick = async () => { const path = await open({ directory: true }); if (path) { stored.output = path; $('output').value = path; persist(); refresh(); } };
  $('open-output').onclick = () => { if (outputDirectory || stored.output) openPath(outputDirectory || stored.output).catch(notify); };
  $('channel').onchange = () => { if (focused === null) return; channels[focused] = +$('channel').value; buildMeshes(); refreshReports(); };
  $('distance').onchange = () => { const value = +$('distance').value; if (geometry && value > 0 && Number.isFinite(value)) { stored.distanceRatio = value / selectedBounds(); $('distance-note').textContent = `单位：${model.units}；所选包围盒对角线的 ${(stored.distanceRatio * 100).toFixed(1)}%`; persist(); } };
  for (const key of Object.keys(defaults)) { const el = $(key); if (!el || key === 'output') continue; el.addEventListener('change', () => { stored[key] = el.type === 'checkbox' ? el.checked : key === 'selfOnly' ? el.value === 'true' : +el.value; if (key === 'device' && !devices.find(d => d.index === +stored.device)?.supported) { stored.ao = false; $('ao').checked = false; } persist(); refresh(); }); }
  root.querySelectorAll('[data-bake-view]').forEach(b => b.onclick = () => setView(b.dataset.bakeView));
  if (desktop) listen('bake-progress', event => { if (event.payload.jobId !== job) return; const data = event.payload.data; status(data.phase); progress?.(data.progress, data.phase); }).then(fn => { if (disposed) fn(); else unlisten = fn; });
  setView('model'); capabilities();
  return { blocker, importModel, activate(mode) { active = mode === 'model-bake'; panel.hidden = !active; if (active) { init3d(); refresh(); } cancel.hidden = !active || !running; }, dispose() { disposed = true; clearTimeout(saveTimer); cancelAnimationFrame(renderFrame); unlisten?.(); controls?.dispose(); clearMeshes(); renderer?.dispose(); if (model && !running) invoke('bake_release', { handle: model.handle }).catch(() => {}); } };
}
