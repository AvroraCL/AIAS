import * as THREE from 'three';
import { OrbitControls } from 'three/addons/controls/OrbitControls.js';
import './model-bake.css';
import { bakeDefaults as defaults, restoreBakeSettings } from './model-bake-state.mjs';

export function createModelBake({ root, desktop, invoke, open, openPath, convertFileSrc, listen, settings, save, busy, withLog, notify, syncSelect = () => {}, progress }) {
  const stored = restoreBakeSettings(settings);
  let model = null, geometry = null, active = false, running = false, loading = false, disposed = false, job = '', view = 'model';
  let objects = new Set(), materials = new Set(), channels = {}, focused = null, results = [];
  let renderer, controls, scene, camera, group, grid, axes, resizeObserver;
  let highlightedTriangle = null, inspecting = false, inspectionError = '', cancelling = false;
  let narrowPanel = 'settings';
  const aoTextures = new Map();
  let resultChannels = {};
  let capabilitiesRequested = false;
  let saveTimer, renderFrame, unlisten, outputDirectory = '', reportRevision = 0, orthographicHeight = 2, renderWidth = 0, renderHeight = 0;

  const section = (name, html) => `<section class="bake-control-section"><h3>${name}</h3>${html}</section>`;
  const select = (id, title, entries) => `<label>${title}<select id="bake-${id}" aria-label="${title}">${entries.map(([value, text]) => `<option value="${value}">${text}</option>`).join('')}</select></label>`;
  const check = (id, title) => `<label class="toggle-label"><input id="bake-${id}" type="checkbox"><span class="toggle-track"><span class="toggle-thumb"></span></span>${title}</label>`;

  root.innerHTML = `<section class="bake-workspace" aria-label="模型烘焙三维工作区">
    <div id="bake-stage" class="bake-viewport">
      <div id="bake-three"></div>
      <canvas id="bake-uv-canvas" hidden aria-label="UV 检查，红色标记问题面"></canvas>
      <div id="bake-results" hidden></div>
      <div id="bake-empty">
        <span class="bake-empty-glyph"><i data-lucide="box" aria-hidden="true"></i></span>
        <span class="bake-eyebrow">MODEL BAKING</span><h2>从一个模型开始</h2>
        <p>检查 UV，为每个材质生成 AO、UV 布局与材质 ID。</p>
        <button id="bake-empty-import" class="primary-compact" type="button">选择模型</button>
        <small id="bake-entry-note">支持 OBJ、GLB、glTF · 静态三角网格</small>
        <div class="bake-entry-steps"><span>01　导入模型</span><span>02　检查 UV</span><span>03　烘焙贴图</span></div>
      </div>

      <header class="bake-overlay bake-viewport-header">
        <div class="bake-header-actions">
          <button id="bake-import" class="bake-floating-button bake-import-button" type="button">导入模型</button>
          <span id="bake-model-info" class="bake-model-info">OBJ · GLB · glTF 静态三角网格</span>
        </div>
        <div class="bake-view-tabs" role="tablist" aria-label="烘焙视图">
          <button data-bake-view="model" type="button" role="tab">模型</button>
          <button data-bake-view="uv" type="button" role="tab">UV</button>
          <button data-bake-view="results" type="button" role="tab">结果</button>
        </div>
        <div class="bake-panel-switches">
          <button data-bake-panel-toggle="outliner" class="bake-floating-button" type="button" aria-controls="bake-outliner-panel"><i data-lucide="list-tree" aria-hidden="true"></i>对象 / 材质</button>
          <button data-bake-panel-toggle="settings" class="bake-floating-button" type="button" aria-controls="bake-settings-panel"><i data-lucide="sliders-horizontal" aria-hidden="true"></i>烘焙设置</button>
        </div>
      </header>

      <div class="bake-display-tools" aria-label="视图显示工具">
        <button id="bake-focus" data-bake-display="focus" class="bake-floating-button" type="button" title="聚焦所选对象 · F" aria-label="聚焦所选对象"><i data-lucide="scan" aria-hidden="true"></i></button>
        <button id="bake-reset" data-bake-display="reset" class="bake-floating-button" type="button" title="复位视图" aria-label="复位视图"><i data-lucide="rotate-ccw" aria-hidden="true"></i></button>
        <button id="bake-projection" data-bake-display="projection" class="bake-floating-button" type="button" title="切换透视 / 正交">透视</button>
        <details class="bake-display-menu"><summary title="显示选项"><i data-lucide="eye" aria-hidden="true"></i><span>显示</span></summary><div>
          <button id="bake-wireframe" data-bake-display="wireframe" type="button">线框</button>
          <button id="bake-grid" data-bake-display="grid" type="button">网格</button>
          <button id="bake-axes" data-bake-display="axes" type="button">坐标轴</button>
        </div></details>
      </div>
      <div id="bake-viewport-note" class="bake-viewport-note">拖动旋转 · 右键平移 · 滚轮缩放 · F 聚焦</div>
      <aside id="bake-outliner-panel" class="bake-float-panel bake-outliner-panel" aria-label="对象与材质">
        <header class="bake-panel-heading"><h2>对象与材质</h2><button data-bake-panel-toggle="outliner" class="bake-floating-button bake-panel-close" type="button" aria-label="收起对象与材质面板">×</button></header>
        <div class="bake-panel-scroll">
          <section class="bake-outliner-section"><h3>参与烘焙的对象 <span id="bake-object-count"></span></h3><div class="bake-list-actions"><button data-bake-select="all" type="button">全选</button><button data-bake-select="none" type="button">清空</button></div><div id="bake-objects"></div></section>
          <section class="bake-outliner-section"><h3>输出材质</h3><small class="bake-list-note">勾选决定输出，点击名称检查 UV</small><div id="bake-materials"></div></section>
          <section class="bake-control-section"><p id="bake-focused">选择材质检查 UV</p>${select('channel', '当前材质 UV 通道', [[0, 'UV0']])}<button id="bake-check-uv" class="secondary-action" type="button">检查当前材质 UV</button><div id="bake-issues" hidden></div></section>
        </div>
      </aside>

      <aside id="bake-settings-panel" class="bake-float-panel bake-settings-panel" aria-label="烘焙参数">
        <header class="bake-panel-heading"><h2>烘焙参数</h2><button data-bake-panel-toggle="settings" class="bake-floating-button bake-panel-close" type="button" aria-label="收起烘焙参数面板">×</button></header>
        <div class="bake-panel-scroll bake-controls">
          ${section('01 / 输出贴图', `${check('ao', '环境遮蔽 AO')}${check('uv', 'UV 线框')}${check('id', '材质 ID')}<small id="bake-output-summary">按所选材质分别导出</small>`)}
          ${section('02 / 烘焙质量', `<div class="bake-presets" aria-label="质量预设"><button data-bake-preset="draft" type="button">快速</button><button data-bake-preset="standard" type="button">标准</button><button data-bake-preset="high" type="button">精细</button></div>${select('resolution', '贴图尺寸', [512, 1024, 2048, 4096].map(value => [value, `${value} × ${value}`]))}<small id="bake-quality-note"></small>`)}
          ${section('03 / 保存位置', `<div class="field-row"><input id="bake-output" aria-label="烘焙输出目录" readonly placeholder="选择输出文件夹"><button id="bake-pick-output" type="button">浏览</button></div><button id="bake-open-output" class="secondary-action output-action" type="button"><i data-lucide="folder-open" aria-hidden="true"></i>打开结果目录</button>`)}
          <details class="bake-advanced"><summary>高级设置</summary>
          ${section('计算设备', `${select('device', 'GPU', [[0, '检测设备中…']])}<small id="bake-device-note"></small>`)}
          ${section('AO 与边缘', `${select('samples', 'AO 采样', [32, 64, 128, 256].map(value => [value, `${value} 次`]))}${select('bits', 'AO 位深', [[8, '8 位线性灰度'], [16, '16 位线性灰度']])}<label>边缘扩展（px）<input id="bake-margin" type="number" min="0" max="128" value="16"></label><label>遮蔽距离<input id="bake-distance" type="number" min="0.000001" step="any" value="1"></label><small id="bake-distance-note">默认包围盒对角线的 10%</small>${select('selfOnly', '遮挡对象', [['false', '所选对象相互遮挡'], ['true', '仅自身遮挡']])}<small>按不透明几何计算，不读取透明贴图。</small>`)}
          </details>
        </div>
      </aside>

      <footer class="bake-footer"><div class="bake-status-panel"><span id="bake-readiness"></span><span id="bake-status" role="status"></span><progress id="bake-progress" max="1" value="0" hidden aria-label="烘焙进度"></progress></div>
      <div class="bake-run-actions"><button id="bake-cancel" class="secondary-action" type="button" hidden>取消烘焙</button><button id="bake-run" class="run-button hidden" type="button">开始烘焙</button></div></footer>
    </div>
  </section>`;

  const $ = id => root.querySelector(`#bake-${id}`);
  for (const key of Object.keys(defaults)) {
    const element = $(key);
    if (!element) continue;
    if (element.type === 'checkbox') element.checked = Boolean(stored[key]);
    else element.value = String(stored[key]);
  }

  let devices = [];
  const presets = { draft: [512, 32], standard: [2048, 128], high: [4096, 256] };
  const status = text => { $('status').textContent = text; };
  const persist = () => {
    clearTimeout(saveTimer);
    saveTimer = setTimeout(() => save({ ...stored, workspace: { ...stored.workspace } }).catch(notify), 250);
  };
  const workspaceKey = panel => panel === 'outliner' ? 'outlinerOpen' : 'settingsOpen';

  function blocker() {
    if (!desktop) return '请在桌面软件中导入模型并烘焙。';
    if (running || loading || busy()) return '任务正在运行。';
    if (inspecting) return '正在检查 UV…';
    if (inspectionError) return 'UV 检查失败，请重新检查后烘焙。';
    if (!model) return '请导入模型。';
    if (!objects.size || !materials.size) return '请选择对象和输出材质。';
    if (!stored.ao && !stored.uv && !stored.id) return '请选择输出类型。';
    if (!stored.output) return '请选择输出目录。';
    if (!Number.isFinite(+$('distance').value) || +$('distance').value <= 0) return '遮蔽距离必须大于 0。';
    if (!Number.isInteger(+$('margin').value) || +$('margin').value < 0 || +$('margin').value > 128) return '边缘扩展应为 0–128 的整数。';
    if (stored.ao && !devices.find(device => device.index === +stored.device)?.supported) return '当前设备不支持 DXR 1.1 AO。';
    return null;
  }

  function updatePanelState(panel) {
    const narrow = root.clientWidth < 900;
    const open = Boolean(model) && view !== 'results' && stored.workspace[workspaceKey(panel)] && (!narrow || narrowPanel === panel);
    const element = $(`${panel}-panel`);
    if (element) element.hidden = !open;
    root.querySelector('.bake-workspace').classList.toggle(`${panel}-open`, open);
    root.querySelectorAll(`[data-bake-panel-toggle="${panel}"]`).forEach(button => {
      button.classList.toggle('active', open);
      button.setAttribute('aria-expanded', String(open));
    });
  }

  function setPanelOpen(panel, open) {
    stored.workspace[workspaceKey(panel)] = open;
    narrowPanel = panel;
    if (open && view === 'results') setView('model');
    updatePanelState('outliner');
    updatePanelState('settings');
    resizeRenderer();
    persist();
  }

  function applyWireframe() {
    if (!group) return;
    group.children.forEach(mesh => {
      mesh.material.wireframe = stored.workspace.wireframe;
      mesh.material.needsUpdate = true;
    });
  }

  function updateSceneHelpers() {
    if (!group || !grid || !axes) return;
    const box = new THREE.Box3().setFromObject(group);
    if (box.isEmpty()) return;
    const size = box.getSize(new THREE.Vector3());
    const center = box.getCenter(new THREE.Vector3());
    const span = Math.max(size.x, size.y, size.z, 0.1);
    grid.position.set(center.x, box.min.y - span * 0.002, center.z);
    grid.scale.set(span * 2, 1, span * 2);
    axes.position.set(center.x, box.min.y, center.z);
    axes.scale.setScalar(span * 0.16);
  }

  function syncDisplayControls() {
    const workspace = stored.workspace;
    const projection = $('projection');
    projection.textContent = workspace.projection === 'perspective' ? '透视' : '正交';
    projection.setAttribute('aria-pressed', String(workspace.projection === 'orthographic'));
    for (const key of ['wireframe', 'grid', 'axes']) {
      const button = $(key);
      button.classList.toggle('active', workspace[key]);
      button.setAttribute('aria-pressed', String(workspace[key]));
    }
    if (grid) grid.visible = Boolean(model) && workspace.grid;
    if (axes) axes.visible = Boolean(model) && workspace.axes;
    applyWireframe();
  }

  function updateCameraViewport() {
    if (!camera) return;
    const width = $('three').clientWidth;
    const height = $('three').clientHeight;
    if (!width || !height) return;
    const aspect = width / height;
    if (camera.isPerspectiveCamera) camera.aspect = aspect;
    else {
      camera.left = -orthographicHeight * aspect / 2;
      camera.right = orthographicHeight * aspect / 2;
      camera.top = orthographicHeight / 2;
      camera.bottom = -orthographicHeight / 2;
    }
    camera.updateProjectionMatrix();
  }

  function resizeRenderer() {
    if (view === 'uv') drawUv(highlightedTriangle);
    if (!renderer) return;
    const width = $('three').clientWidth;
    const height = $('three').clientHeight;
    if (!width || !height) return;
    if (width !== renderWidth || height !== renderHeight) {
      renderWidth = width;
      renderHeight = height;
      renderer.setSize(width, height, false);
      updateCameraViewport();
      if (view === 'uv') drawUv();
    }
  }

  function createControls(target = new THREE.Vector3()) {
    controls?.dispose();
    controls = new OrbitControls(camera, renderer.domElement);
    controls.enableDamping = true;
    controls.dampingFactor = 0.08;
    controls.target.copy(target);
    controls.update();
  }

  function init3d() {
    if (renderer) return;
    try {
      renderer = new THREE.WebGLRenderer({ antialias: true, alpha: true });
      renderer.setPixelRatio(Math.min(devicePixelRatio, 2));
      $('three').append(renderer.domElement);
      scene = new THREE.Scene();
      camera = new THREE.PerspectiveCamera(45, 1, 0.001, 1e7);
      camera.position.set(2, 1.5, 2.5);
      createControls();
      scene.add(new THREE.HemisphereLight(0xffffff, 0x404040, 2.2));
      const light = new THREE.DirectionalLight(0xffffff, 2.6);
      light.position.set(3, 5, 4);
      scene.add(light);
      group = new THREE.Group();
      grid = new THREE.GridHelper(1, 10, 0x5a5a5a, 0x303030);
      grid.material.transparent = true;
      grid.material.opacity = 0.56;
      axes = new THREE.AxesHelper(1);
      axes.setColors(0x999999, 0xcccccc, 0x666666);
      scene.add(grid, axes, group);
      setProjection(stored.workspace.projection, false);
      resizeObserver = typeof ResizeObserver === 'function' ? new ResizeObserver(() => { updatePanelState('outliner'); updatePanelState('settings'); resizeRenderer(); }) : null;
      resizeObserver?.observe($('stage'));
      resizeObserver?.observe($('three'));
      const animate = () => {
        if (disposed) return;
        renderFrame = requestAnimationFrame(animate);
        if (!active || view !== 'model') return;
        resizeRenderer();
        controls.update();
        renderer.render(scene, camera);
      };
      animate();
    } catch (error) {
      status(`三维预览不可用：${error}。UV 检查与烘焙仍可使用。`);
    }
  }

  function clearMeshes() {
    if (!group) return;
    for (const mesh of [...group.children]) {
      mesh.geometry.dispose();
      mesh.material.aoMap?.dispose();
      mesh.material.dispose();
      group.remove(mesh);
    }
  }

  function buildMeshes() {
    init3d();
    if (!group || !geometry) return;
    clearMeshes();
    const batches = new Map();
    for (const triangle of geometry.triangles) {
      if (!objects.has(triangle.object)) continue;
      const key = `${triangle.object}:${triangle.material}`;
      if (!batches.has(key)) batches.set(key, { positions: [], normals: [], uvs: [], material: triangle.material, object: triangle.object });
      const batch = batches.get(key);
      batch.positions.push(...triangle.positions.flat());
      batch.normals.push(...triangle.normals.flat());
      batch.uvs.push(...(triangle.uvs[channels[triangle.material] ?? 0] || [[0, 0], [0, 0], [0, 0]]).flat());
    }
    for (const batch of batches.values()) {
      const meshGeometry = new THREE.BufferGeometry();
      meshGeometry.setAttribute('position', new THREE.Float32BufferAttribute(batch.positions, 3));
      meshGeometry.setAttribute('normal', new THREE.Float32BufferAttribute(batch.normals, 3));
      meshGeometry.setAttribute('uv', new THREE.Float32BufferAttribute(batch.uvs, 2));
      const material = new THREE.MeshStandardMaterial({ color: 0xb8b8b8, roughness: 0.85, side: THREE.DoubleSide, wireframe: stored.workspace.wireframe });
      const mesh = new THREE.Mesh(meshGeometry, material);
      mesh.userData.material = batch.material;
      mesh.userData.object = batch.object;
      const ao = aoTextures.get(`${batch.material}:${channels[batch.material] ?? 0}`);
      if (ao) { mesh.material.aoMap = ao.clone(); mesh.material.aoMap.needsUpdate = true; }
      group.add(mesh);
    }
    updateSceneHelpers();
    syncDisplayControls();
  }

  function meshBounds() {
    if (!group?.children.length) return null;
    const box = new THREE.Box3().setFromObject(group);
    return box.isEmpty() ? null : box;
  }

  function selectedBounds() {
    const box = new THREE.Box3();
    for (const triangle of geometry?.triangles || []) if (objects.has(triangle.object)) {
      for (const position of triangle.positions) box.expandByPoint(new THREE.Vector3(...position));
    }
    return box.isEmpty() ? 0 : box.getSize(new THREE.Vector3()).length();
  }

  function frameSelection(resetDirection = false) {
    const box = meshBounds();
    if (!box || !camera || !controls) return;
    controls.update();
    const center = box.getCenter(new THREE.Vector3());
    const radius = Math.max(box.getSize(new THREE.Vector3()).length() / 2, 0.00001);
    const aspect = Math.max($('three').clientWidth / Math.max($('three').clientHeight, 1), 0.1);
    const angle = Math.atan(Math.tan(THREE.MathUtils.degToRad(22.5)) * Math.min(aspect, 1));
    const distance = radius / Math.sin(angle) * 1.15;
    const direction = resetDirection ? new THREE.Vector3(0.8, 0.6, 1) : camera.position.clone().sub(controls.target);
    if (!direction.lengthSq()) direction.set(0.8, 0.6, 1);
    camera.near = Math.max(radius / 10000, 0.0000001);
    camera.far = distance + radius * 100;
    camera.position.copy(center).add(direction.normalize().multiplyScalar(distance));
    camera.zoom = 1;
    controls.target.copy(center);
    orthographicHeight = radius * 2.3 / Math.min(aspect, 1);
    updateCameraViewport();
    controls.update();
  }

  function reset() { frameSelection(true); }

  function setProjection(next, shouldPersist = true) {
    if (next !== 'perspective' && next !== 'orthographic') return;
    stored.workspace.projection = next;
    if (!camera || !renderer) {
      syncDisplayControls();
      if (shouldPersist) persist();
      return;
    }
    if ((next === 'perspective') === camera.isPerspectiveCamera) {
      syncDisplayControls();
      if (shouldPersist) persist();
      return;
    }
    const target = controls.target.clone();
    const position = camera.position.clone();
    const near = camera.near;
    const far = camera.far;
    if (next === 'orthographic') {
      const distance = Math.max(position.distanceTo(target), 0.1);
      orthographicHeight = Math.max(0.001, distance * 2 * Math.tan(THREE.MathUtils.degToRad(22.5)));
      camera = new THREE.OrthographicCamera(-1, 1, 1, -1, near, far);
    } else {
      const distance = orthographicHeight / camera.zoom / (2 * Math.tan(THREE.MathUtils.degToRad(22.5)));
      position.sub(target).normalize().multiplyScalar(distance).add(target);
      camera = new THREE.PerspectiveCamera(45, 1, near, far);
    }
    camera.position.copy(position);
    updateCameraViewport();
    createControls(target);
    syncDisplayControls();
    if (shouldPersist) persist();
  }

  function focusMaterial(id) {
    highlightedTriangle = null;
    focused = id;
    const material = model.materials.find(item => item.id === id);
    $('focused').textContent = `材质 ${id} · ${material.name}`;
    $('channel').replaceChildren(...material.channels.map(channel => new Option(`UV${channel.channel}`, channel.channel)));
    $('channel').value = String(channels[id] ?? 0);
    syncSelect($('channel'));
    drawUv();
    $('materials').querySelectorAll('button').forEach(button => button.classList.toggle('active', +button.dataset.material === id));
  }

  function lists() {
    for (const [name, items, selected] of [['objects', model.objects, objects], ['materials', model.materials, materials]]) {
      $(name).replaceChildren();
      for (const item of items) {
        const row = document.createElement('div');
        row.className = 'bake-select-row';
        const input = document.createElement('input');
        input.type = 'checkbox';
        input.checked = selected.has(item.id);
        input.setAttribute('aria-label', `${name === 'objects' ? '对象' : '材质'} ${item.id} ${item.name}`);
        input.onchange = () => {
          if (input.checked) selected.add(item.id);
          else selected.delete(item.id);
          if (name === 'objects') {
            buildMeshes();
            $('distance').value = String(Math.max(selectedBounds() * stored.distanceRatio, 0.000001));
            refreshReports();
          }
          refresh();
        };
        const label = document.createElement('button');
        label.className = 'bake-item';
        label.type = 'button';
        label.textContent = item.name;
        label.title = item.name;
        label.dataset.material = item.id;
        label.onclick = () => {
          if (name === 'materials') focusMaterial(item.id);
          else { input.checked = !input.checked; input.onchange(); }
        };
        row.append(input, label);
        $(name).append(row);
      }
    }
  }

  async function refreshReports() {
    if (!model) return;
    const revision = ++reportRevision;
    inspecting = true; inspectionError = ''; refresh();
    try {
      const reports = await invoke('bake_inspect', { handle: model.handle, objects: [...objects], channels });
      if (revision !== reportRevision || disposed) return;
      for (const report of reports) {
        const material = model.materials.find(item => item.id === report.material);
        const index = material.channels.findIndex(channel => channel.channel === report.channel);
        if (index >= 0) material.channels[index] = report;
        else material.channels.push(report);
      }
      drawUv();
    } catch (error) {
      if (revision === reportRevision) { inspectionError = String(error); status(`UV 检查失败：${error}`); }
    } finally {
      if (revision === reportRevision) { inspecting = false; refresh(); }
    }
  }

  function drawUv(highlight = highlightedTriangle) {
    highlightedTriangle = highlight;
    if (!model || focused === null) return;
    const canvas = $('uv-canvas');
    const stage = $('stage');
    const safe = getComputedStyle(root.querySelector('.bake-workspace'));
    const width = stage.clientWidth - parseFloat(safe.getPropertyValue('--bake-left')) - parseFloat(safe.getPropertyValue('--bake-right'));
    const available = Math.min(width - 48, stage.clientHeight - 212);
    const size = Math.max(64, Math.min(1400, available || 256));
    canvas.width = canvas.height = size;
    const context = canvas.getContext('2d');
    context.fillStyle = '#181818';
    context.fillRect(0, 0, size, size);
    const report = model.materials.find(material => material.id === focused)?.channels.find(channel => channel.channel === (channels[focused] ?? 0));
    const bad = new Set((report?.issues || []).flatMap(issue => [issue.triangle, issue.otherTriangle]).filter(index => index != null));
    geometry.triangles.forEach((triangle, index) => {
      if (triangle.material !== focused || !objects.has(triangle.object)) return;
      const uv = triangle.uvs[channels[focused] ?? 0];
      if (!uv) return;
      context.beginPath();
      uv.forEach((point, pointIndex) => {
        const x = 12 + point[0] * (size - 24);
        const y = 12 + (1 - point[1]) * (size - 24);
        if (pointIndex) context.lineTo(x, y);
        else context.moveTo(x, y);
      });
      context.closePath();
      context.strokeStyle = highlight === index ? '#ffd166' : bad.has(index) ? '#ff6278' : '#a8a8a8';
      context.lineWidth = highlight === index ? 3 : 1;
      if (bad.has(index)) {
        context.fillStyle = '#ff627833';
        context.fill();
      }
      context.stroke();
    });
    const issues = $('issues');
    issues.replaceChildren();
    if (report?.issues?.length) {
      const note = document.createElement('p');
      note.textContent = `${report.issueCount} 处 UV 问题；该材质的 AO／ID 会被阻止，UV 线框仍可导出。`;
      issues.append(note);
      for (const issue of report.issues.slice(0, 30)) {
        const button = document.createElement('button');
        button.className = 'bake-issue';
        button.type = 'button';
        button.textContent = `对象 ${issue.object} · 源面 ${issue.face}：${issue.kind}${issue.otherTriangle != null ? `（与三角形 ${issue.otherTriangle}）` : ''}`;
        button.onclick = () => { setView('uv'); drawUv(issue.triangle); };
        issues.append(button);
      }
    } else {
      issues.textContent = report ? '当前材质 UV 检查通过。' : 'UV 检查中…';
    }
  }

  function setView(next) {
    view = next;
    $('three').hidden = next !== 'model';
    $('uv-canvas').hidden = next !== 'uv';
    $('results').hidden = next !== 'results';
    if (next === 'results' && !results.length && !$('results').children.length) {
      const empty = document.createElement('p');
      empty.className = 'bake-results-empty';
      empty.textContent = '尚无烘焙结果。完成任务后将在这里预览贴图。';
      $('results').append(empty);
    }
    $('empty').hidden = Boolean(model) || next !== 'model';
    $('issues').hidden = next !== 'uv';
    root.querySelector('.bake-workspace').dataset.view = next;
    if (next === 'uv' && model) { stored.workspace.outlinerOpen = true; narrowPanel = 'outliner'; }
    updatePanelState('outliner'); updatePanelState('settings');
    $('viewport-note').hidden = !model || next !== 'model';
    root.querySelector('.bake-display-tools').hidden = !model || next !== 'model';
    root.querySelectorAll('[data-bake-view]').forEach(button => {
      const selected = button.dataset.bakeView === next;
      button.classList.toggle('active', selected);
      button.setAttribute('aria-selected', String(selected));
    });
    if (next === 'uv') drawUv();
    refresh();
  }

  function refresh() {
    const locked = running || loading || busy();
    root.querySelector('.bake-workspace').classList.toggle('has-model', Boolean(model));
    root.querySelectorAll('input, select, button').forEach(element => {
      const staysInteractive = element.dataset.bakeView || element.dataset.bakeDisplay || element.dataset.bakePanelToggle || element.id === 'bake-cancel';
      element.disabled = locked && !staysInteractive;
    });
    $('import').disabled = locked || !desktop;
    $('empty-import').disabled = locked || !desktop;
    $('empty-import').textContent = loading ? '正在导入…' : '选择模型';
    $('entry-note').textContent = desktop ? '支持 OBJ、GLB、glTF · 静态三角网格' : '浏览器可查看界面；导入与烘焙请在桌面软件中使用。';
    $('import').textContent = model ? '更换模型' : '导入模型';
    root.querySelectorAll('[data-bake-panel-toggle], [data-bake-display]').forEach(el => el.disabled = !model);
    root.querySelectorAll('[data-bake-view]').forEach(el => el.disabled = el.dataset.bakeView === 'uv' ? !model : el.dataset.bakeView === 'results' ? !results.length : false);
    $('check-uv').disabled = !model || locked || inspecting;
    $('cancel').disabled = cancelling;
    $('progress').hidden = !running;
    $('object-count').textContent = model ? `${objects.size} / ${model.objects.length}` : '';
    const count = materials.size * ['ao', 'uv', 'id'].filter(key => stored[key]).length;
    $('output-summary').textContent = `${materials.size} 个材质 · 预计 ${count} 张贴图`;
    $('quality-note').textContent = stored.ao ? `${stored.samples} 次 AO 采样 · ${stored.bits} 位灰度` : 'UV 与 ID 导出不使用 AO 采样';
    for (const key of ['samples', 'bits', 'device', 'distance', 'selfOnly']) $(key).disabled = locked || !stored.ao;
    root.querySelectorAll('[data-bake-preset]').forEach(button => {
      const [resolution, samples] = presets[button.dataset.bakePreset];
      button.setAttribute('aria-pressed', String(stored.resolution === resolution && stored.samples === samples));
    });
    const device = devices.find(item => item.index === +stored.device);
    $('ao').disabled = locked || !device?.supported;
    $('ao').closest('label').title = device?.supported ? '使用 GPU 生成环境遮蔽' : '当前设备不支持 AO，仍可导出 UV 与材质 ID';
    $('device-note').textContent = !desktop ? '桌面版可检测 DXR 显卡' : device ? `${device.supported ? 'DXR 1.1 可用' : device.reason} · 可用预算 ${(device.availableBytes / 1073741824).toFixed(1)} GiB${[4098, 32902].includes(device.vendor) ? '；按能力支持，待硬件实测' : ''}` : '无合格显卡；仍可导出 UV／ID';
    const reason = blocker();
    $('run').disabled = Boolean(reason);
    $('run').title = reason || '';
    $('readiness').textContent = running ? (cancelling ? '正在取消…' : '正在烘焙') : loading ? '正在导入模型…' : reason || `已就绪 · ${count} 张贴图`;
    $('open-output').disabled = !desktop || !(outputDirectory || stored.output);
    $('pick-output').disabled = !desktop || locked;
    $('cancel').hidden = !running || !active;
    root.querySelectorAll('select').forEach(syncSelect);
  }

  async function capabilities() {
    if (!desktop) {
      $('device').replaceChildren(new Option('请使用桌面版', '0'));
      refresh();
      return;
    }
    try {
      devices = (await invoke('bake_capabilities')).devices || [];
      const device = $('device');
      device.replaceChildren();
      for (const item of devices) device.add(new Option(`${item.name}${item.supported ? '' : '（不支持 AO）'}`, item.index));
      if (!devices.some(item => item.index === +stored.device)) stored.device = devices.find(item => item.supported)?.index ?? devices[0]?.index ?? 0;
      device.value = String(stored.device);
      if (!devices.find(item => item.index === +stored.device)?.supported) {
        stored.ao = false;
        $('ao').checked = false;
      }
    } catch (error) {
      status(String(error));
      $('device').replaceChildren(new Option('设备检测失败', '0'));
      stored.ao = false;
      $('ao').checked = false;
    }
    refresh();
  }

  async function importModel(path) {
    if (!desktop || running || loading || busy()) return;
    loading = true;
    let pendingModel = null;
    refresh();
    status('正在导入并检查模型…');
    try {
      const data = await invoke('bake_import', { path });
      pendingModel = data;
      const response = await fetch(convertFileSrc(data.meshPath));
      if (!response.ok) throw Error('无法读取规范化网格');
      const mesh = await response.json();
      if (!data.materials?.length || !data.objects?.length || !mesh.triangles?.length) throw Error('模型没有可用的网格或材质');
      if (model) await invoke('bake_release', { handle: model.handle });
      ++reportRevision; inspecting = false; inspectionError = '';
      aoTextures.forEach(texture => texture.dispose()); aoTextures.clear();
      outputDirectory = '';
      model = data;
      pendingModel = null;
      geometry = mesh;
      highlightedTriangle = null;
      objects = new Set(model.objects.map(item => item.id));
      materials = new Set(model.materials.map(item => item.id));
      channels = Object.fromEntries(model.materials.map(item => [item.id, 0]));
      results = [];
      $('results').replaceChildren();
      $('model-info').textContent = `${model.name} · ${model.objects.length} 对象 · ${model.triangleCount.toLocaleString()} 三角形`;
      if (model.degenerateFaces > 0) {
        const examples = (model.degenerateExamples || []).join('、');
        notify(`已跳过 ${model.degenerateFaces} 个零面积退化面${examples ? `（${examples}${model.degenerateFaces > (model.degenerateExamples || []).length ? ' 等' : ''}）` : ''}，这些面对烘焙无影响。`);
      }
      lists();
      updatePanelState('outliner'); updatePanelState('settings');
      buildMeshes();
      resizeRenderer(); reset();
      $('distance').value = String(selectedBounds() * stored.distanceRatio);
      $('distance-note').textContent = `单位：${model.units}；所选包围盒对角线的 ${(stored.distanceRatio * 100).toFixed(1)}%`;
      focusMaterial(model.materials[0].id);
      setView('model');
      status('模型已导入。检查材质 UV 后开始烘焙。');
    } catch (error) {
      if (pendingModel) invoke('bake_release', { handle: pendingModel.handle }).catch(() => {});
      status(`导入失败：${error}`);
      notify(error);
    } finally {
      loading = false;
      refresh();
    }
  }

  function showResults(data) {
    results = data.files || [];
    resultChannels = { ...channels };
    outputDirectory = data.directory;
    $('results').replaceChildren();
    for (const file of results) {
      const card = document.createElement('div');
      card.className = 'bake-result';
      const title = document.createElement('p');
      title.textContent = `材质 ${file.material} · ${file.kind.toUpperCase()}`;
      const image = document.createElement('img');
      image.src = convertFileSrc(file.path);
      image.alt = title.textContent;
      const preview = document.createElement('button');
      preview.className = 'bake-result-preview';
      preview.type = 'button'; preview.title = '放大查看贴图';
      preview.setAttribute('aria-label', `放大 ${title.textContent}`);
      preview.onclick = () => {
        const dialog = document.createElement('dialog'); dialog.className = 'bake-image-dialog';
        const close = document.createElement('button'); close.textContent = '关闭'; close.onclick = () => dialog.close();
        const full = image.cloneNode();
        dialog.append(close, full); root.append(dialog);
        dialog.addEventListener('close', () => dialog.remove(), {once:true}); dialog.showModal();
      };
      preview.append(image);
      card.append(preview, title);
      const reveal = document.createElement('button'); reveal.className = 'secondary-action'; reveal.textContent = '打开文件';
      reveal.onclick = () => openPath(file.path).catch(notify); card.append(reveal);
      if (file.kind === 'ao') {
        const apply = document.createElement('button');
        apply.className = 'secondary-action';
        apply.type = 'button';
        apply.textContent = '应用 AO 到模型';
        apply.onclick = () => {
          if (!group) return;
          const appliedModel = model;
          const uvChannel = resultChannels[file.material] ?? 0;
          if (uvChannel !== (channels[file.material] ?? 0)) { status('请切回烘焙时使用的 UV 通道，再应用该 AO 结果。'); return; }
          new THREE.TextureLoader().load(convertFileSrc(file.path), texture => {
            if (disposed || appliedModel !== model || uvChannel !== (channels[file.material] ?? 0)) { texture.dispose(); return; }
            const key = `${file.material}:${uvChannel}`;
            aoTextures.get(key)?.dispose(); aoTextures.set(key, texture.clone());
            texture.colorSpace = THREE.NoColorSpace;
            for (const mesh of group.children.filter(item => item.userData.material === file.material)) {
              mesh.material.aoMap?.dispose();
              mesh.material.aoMap = texture.clone();
              mesh.material.aoMap.needsUpdate = true;
              mesh.material.aoMapIntensity = 1;
              mesh.material.needsUpdate = true;
            }
            texture.dispose();
            setView('model');
            status('AO 已应用到预览，可切换线框检查。');
          }, undefined, error => { status('AO 贴图加载失败'); notify(error); });
        };
        card.append(apply);
      }
      $('results').append(card);
    }
    setView('results');
    status(`${data.cancelled ? '已取消' : data.failures?.length ? '部分完成' : '烘焙完成'} · ${results.length} 张贴图${data.elapsedMs != null ? ` · ${(data.elapsedMs / 1000).toFixed(1)} 秒` : ''}${data.failures?.length ? `。${data.failures.join('；')}` : ''}`);
  }

  $('run').onclick = async () => {
    if (blocker()) return;
    job = crypto.randomUUID();
    running = true; cancelling = false; $('progress').value = 0;
    refresh();
    try {
      await withLog('model-bake-log', $('run'), async () => {
        const types = ['ao', 'uv', 'id'].filter(key => stored[key]);
        const { workspace, ...options } = stored;
        const data = await invoke('bake_start', {
          handle: model.handle,
          jobId: job,
          options: { ...options, device: +stored.device, objects: [...objects], materials: [...materials], channels: { ...channels }, distance: +$('distance').value },
        });
        showResults(data);
        return {
          completed: data.files?.length || 0,
          total: materials.size * types.length,
          logs: [...(data.files || []).map(file => file.path), ...(data.failures || []), ...(data.cancelled ? ['任务已取消，已完成文件保留。'] : [])],
        };
      }, '模型烘焙');
    } finally {
      running = false; cancelling = false;
      job = '';
      refresh();
    }
  };

  $('cancel').onclick = async () => {
    if (!running || cancelling) return;
    cancelling = true; $('cancel').disabled = true; refresh();
    try {
      await invoke('bake_cancel', { jobId: job });
      status('正在取消，保留已完整写入的结果…');
    } catch (error) {
      cancelling = false; notify(error); refresh();
    }
  };
  $('import').onclick = async () => {
    if (!desktop) return;
    const path = await open({ multiple: false, filters: [{ name: '静态模型', extensions: ['obj', 'glb', 'gltf'] }] });
    if (path) await importModel(path);
  };
  $('reset').onclick = reset;
  $('focus').onclick = () => frameSelection(false);
  $('projection').onclick = () => setProjection(stored.workspace.projection === 'perspective' ? 'orthographic' : 'perspective');
  for (const key of ['wireframe', 'grid', 'axes']) {
    $(key).onclick = () => {
      stored.workspace[key] = !stored.workspace[key];
      syncDisplayControls();
      persist();
    };
  }
  root.querySelectorAll('[data-bake-panel-toggle]').forEach(button => {
    button.onclick = () => {
      const panel = button.dataset.bakePanelToggle;
      setPanelOpen(panel, $(`${panel}-panel`).hidden);
    };
  });
  $('pick-output').onclick = async () => {
    const path = await open({ directory: true });
    if (path) {
      stored.output = path;
      $('output').value = path;
      persist();
      refresh();
    }
  };
  $('open-output').onclick = () => { if (outputDirectory || stored.output) openPath(outputDirectory || stored.output).catch(notify); };
  $('channel').onchange = () => {
    if (focused === null) return;
    channels[focused] = +$('channel').value;
    buildMeshes();
    refreshReports();
  };
  $('distance').onchange = () => {
    const value = +$('distance').value;
    if (geometry && value > 0 && Number.isFinite(value)) {
      const diagonal = selectedBounds();
      if (!diagonal) return;
      stored.distanceRatio = value / diagonal;
      $('distance-note').textContent = `单位：${model.units}；所选包围盒对角线的 ${(stored.distanceRatio * 100).toFixed(1)}%`;
      persist();
    }
    refresh();
  };
  for (const key of Object.keys(defaults)) {
    const element = $(key);
    if (!element || key === 'output' || key === 'workspace') continue;
    element.addEventListener('change', () => {
      stored[key] = element.type === 'checkbox' ? element.checked : key === 'selfOnly' ? element.value === 'true' : +element.value;
      if (key === 'device' && !devices.find(item => item.index === +stored.device)?.supported) {
        stored.ao = false;
        $('ao').checked = false;
      }
      persist();
      refresh();
    });
  }
  root.querySelectorAll('[data-bake-view]').forEach(button => { button.onclick = () => setView(button.dataset.bakeView); });
  if (desktop) {
    listen('bake-progress', event => {
      if (event.payload.jobId !== job) return;
      const data = event.payload.data;
      status(data.phase);
      $('progress').value = Math.max(0, Math.min(1, Number(data.progress) || 0));
      progress?.(data.progress, data.phase);
    }).then(unsubscribe => { if (disposed) unsubscribe(); else unlisten = unsubscribe; });
  }

  $('empty-import').onclick = () => $('import').click();
  $('check-uv').onclick = () => { setView('uv'); refreshReports(); };
  root.querySelectorAll('[data-bake-preset]').forEach(button => {
    button.onclick = () => {
      [stored.resolution, stored.samples] = presets[button.dataset.bakePreset];
      $('resolution').value = String(stored.resolution); $('samples').value = String(stored.samples);
      persist(); refresh();
    };
  });
  root.querySelectorAll('[data-bake-select]').forEach(button => {
    button.onclick = () => {
      if (!model || running || loading) return;
      objects = new Set(button.dataset.bakeSelect === 'all' ? model.objects.map(item => item.id) : []);
      lists(); buildMeshes(); $('distance').value = String(Math.max(selectedBounds() * stored.distanceRatio, 0.000001));
      refreshReports(); refresh();
    };
  });
  const keyboard = event => {
    if (!active || event.ctrlKey || event.metaKey || event.altKey || event.target.closest('input, select, textarea, button, [role="combobox"], dialog')) return;
    if (event.key.toLowerCase() === 'f' && model && view === 'model') { event.preventDefault(); frameSelection(); }
  };
  document.addEventListener('keydown', keyboard);
  updatePanelState('outliner');
  updatePanelState('settings');
  syncDisplayControls();
  setView('model');

  // 构造时不再无条件探测 DXR 能力（会拉起 worker 子进程拖慢启动）；
  // 首次激活烘焙模式时由 app.js 调用，已拉取过或正在拉取则跳过。
  function refreshCapabilities() {
    if (capabilitiesRequested) return;
    capabilitiesRequested = true;
    capabilities();
  }

  return {
    blocker,
    importModel,
    refreshCapabilities,
    activate(mode) {
      active = mode === 'model-bake';
      if (active) {
        init3d();
        requestAnimationFrame(() => resizeRenderer());
      }
      refresh();
    },
    dispose() {
      disposed = true;
      clearTimeout(saveTimer);
      cancelAnimationFrame(renderFrame);
      resizeObserver?.disconnect();
      document.removeEventListener('keydown', keyboard);
      aoTextures.forEach(texture => texture.dispose()); aoTextures.clear();
      unlisten?.();
      controls?.dispose();
      clearMeshes();
      [grid, axes].forEach(helper => {
        helper?.geometry?.dispose();
        const materials = Array.isArray(helper?.material) ? helper.material : [helper?.material];
        materials.forEach(material => material?.dispose());
      });
      renderer?.dispose();
      if (model && !running) invoke('bake_release', { handle: model.handle }).catch(() => {});
    },
  };
}
