import * as THREE from 'three';
import { OrbitControls } from 'three/addons/controls/OrbitControls.js';
import './model-bake.css';
import { bakeDefaults as defaults, restoreBakeSettings } from './model-bake-state.mjs';

export function createModelBake({ root, desktop, invoke, open, openPath, convertFileSrc, listen, settings, save, busy, withLog, notify, syncSelect = () => {}, progress }) {
  const stored = restoreBakeSettings(settings);
  let model = null, geometry = null, active = false, running = false, loading = false, disposed = false, job = '', view = 'model';
  let materials = new Set(), channels = {}, focused = null, results = [];
  let renderer, controls, scene, camera, group, grid, axes, resizeObserver;
  let highlightedTriangle = null, inspecting = false, inspectionError = '', cancelling = false;
  let narrowPanel = 'settings';
  const aoTextures = new Map();
  let resultChannels = {};
  let capabilitiesRequested = false;
  let saveTimer, renderFrame, unlisten, outputDirectory = '', reportRevision = 0, orthographicHeight = 2, renderWidth = 0, renderHeight = 0;
  let reportTimer, reportPending = false, reportInFlight = false, inFlightSignature = '', lastReportSignature = '';
  const objectBounds = new Map();
  const scratchPoint = new THREE.Vector3(), scratchSize = new THREE.Vector3();

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
      <div id="bake-viewport-note" class="bake-viewport-note">Alt+左键 旋转 · 中键 平移 · 滚轮 缩放 · F 聚焦 · 1/3/7 前侧顶视图</div>
      <aside id="bake-outliner-panel" class="bake-float-panel bake-outliner-panel" aria-label="材质">
        <header class="bake-panel-heading"><h2>材质</h2><button data-bake-panel-toggle="outliner" class="bake-floating-button bake-panel-close" type="button" aria-label="收起材质面板">×</button></header>
        <div class="bake-panel-scroll">
          <section class="bake-outliner-section"><h3>输出材质 <span id="bake-object-count"></span></h3><div class="bake-list-actions"><button data-bake-select="all" type="button">全选</button><button data-bake-select="none" type="button">清空</button></div><small class="bake-list-note">勾选决定输出，点击名称检查 UV</small><div id="bake-materials"></div></section>
          <section class="bake-control-section"><p id="bake-focused">选择材质检查 UV</p>${select('channel', '当前材质 UV 通道', [[0, 'UV0']])}<button id="bake-check-uv" class="secondary-action" type="button">检查当前材质 UV</button><div id="bake-issues" hidden></div></section>
        </div>
      </aside>

      <aside id="bake-settings-panel" class="bake-float-panel bake-settings-panel" aria-label="烘焙参数">
        <header class="bake-panel-heading"><h2>烘焙参数</h2><button data-bake-panel-toggle="settings" class="bake-floating-button bake-panel-close" type="button" aria-label="收起烘焙参数面板">×</button></header>
        <div class="bake-panel-scroll bake-controls">
          ${section('01 / 输出贴图', `${check('ao', '环境遮蔽 AO')}${check('uv', 'UV 线框')}${check('id', '材质 ID')}<small id="bake-output-summary">按所选材质分别导出</small>`)}
          ${section('02 / 烘焙质量', `<div class="bake-presets" aria-label="质量预设"><button data-bake-preset="draft" type="button">快速</button><button data-bake-preset="standard" type="button">标准</button><button data-bake-preset="high" type="button">精细</button></div>${select('resolution', '贴图尺寸', [512, 1024, 2048, 4096].map(value => [value, `${value} × ${value}`]))}<small id="bake-quality-note"></small>`)}
          ${section('03 / 导出', `<button id="bake-export" class="secondary-action output-action" data-bake-export type="button"><i data-lucide="download" aria-hidden="true"></i>导出全部贴图</button><button id="bake-open-output" class="secondary-action output-action" type="button"><i data-lucide="folder-open" aria-hidden="true"></i>打开缓存目录</button><small>结果先缓存在应用数据目录，导出时选择目标文件夹</small>`)}
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
  // 大网格解析/构建前让状态文案先上屏：等两帧覆盖一次绘制；窗口最小化时 rAF
  // 不触发，用 200ms 定时器兜底，避免导入流程停住。
  const nextPaint = () => new Promise(resolve => {
    const timer = setTimeout(resolve, 200);
    requestAnimationFrame(() => requestAnimationFrame(() => { clearTimeout(timer); resolve(); }));
  });
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
    if (!materials.size) return '请选择输出材质。';
    if (!stored.ao && !stored.uv && !stored.id) return '请选择输出类型。';
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
    const box = selectionBox();
    if (!box) return;
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
    // Marmoset/Substance 键位：左键留给点选，Alt+左键旋转（keyboard 处理器
    // 动态切换 LEFT），中键/右键平移，滚轮缩放。
    controls.mouseButtons = { LEFT: null, MIDDLE: THREE.MOUSE.PAN, RIGHT: THREE.MOUSE.PAN };
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

  // 一次性为全部 (对象,材质) 组合建立网格，并按对象预计算包围盒：之后选择变化只切
  // mesh.visible，不再重扫三角形重建几何体（该函数只允许在导入/几何体变化时调用）。
  function buildMeshes() {
    init3d();
    if (!group || !geometry) return;
    clearMeshes();
    objectBounds.clear();
    const batches = new Map();
    for (const triangle of geometry.triangles) {
      let bounds = objectBounds.get(triangle.object);
      if (!bounds) objectBounds.set(triangle.object, bounds = new THREE.Box3());
      for (const position of triangle.positions) bounds.expandByPoint(scratchPoint.set(position[0], position[1], position[2]));
      const key = `${triangle.object}:${triangle.material}`;
      if (!batches.has(key)) batches.set(key, { positions: [], normals: [], uvs: [], material: triangle.material, object: triangle.object });
      const batch = batches.get(key);
      const [p0, p1, p2] = triangle.positions;
      batch.positions.push(p0[0], p0[1], p0[2], p1[0], p1[1], p1[2], p2[0], p2[1], p2[2]);
      const [n0, n1, n2] = triangle.normals;
      batch.normals.push(n0[0], n0[1], n0[2], n1[0], n1[1], n1[2], n2[0], n2[1], n2[2]);
      const uv = triangle.uvs[channels[triangle.material] ?? 0] || [[0, 0], [0, 0], [0, 0]];
      batch.uvs.push(uv[0][0], uv[0][1], uv[1][0], uv[1][1], uv[2][0], uv[2][1]);
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
    syncMeshVisibility();
    updateSceneHelpers();
    syncDisplayControls();
  }

  function syncMeshVisibility() {
    if (!group) return;
    for (const mesh of group.children) mesh.visible = materials.has(mesh.userData.material);
  }

  // 通道切换只更新该材质的 uv 属性，并让 AO 贴图随通道失效或恢复（等价于原整表重建）。
  function refreshMaterialUv(id) {
    if (!group || !geometry) return;
    const channel = channels[id] ?? 0;
    const values = new Map();
    for (const triangle of geometry.triangles) {
      if (triangle.material !== id) continue;
      if (!values.has(triangle.object)) values.set(triangle.object, []);
      const uv = triangle.uvs[channel] || [[0, 0], [0, 0], [0, 0]];
      values.get(triangle.object).push(uv[0][0], uv[0][1], uv[1][0], uv[1][1], uv[2][0], uv[2][1]);
    }
    const ao = aoTextures.get(`${id}:${channel}`);
    for (const mesh of group.children) {
      if (mesh.userData.material !== id) continue;
      const next = values.get(mesh.userData.object);
      if (!next) continue;
      const attribute = mesh.geometry.getAttribute('uv');
      if (attribute.array.length === next.length) {
        attribute.array.set(next);
        attribute.needsUpdate = true;
      } else {
        mesh.geometry.setAttribute('uv', new THREE.Float32BufferAttribute(next, 2));
      }
      mesh.material.aoMap?.dispose();
      mesh.material.aoMap = ao ? ao.clone() : null;
      if (mesh.material.aoMap) mesh.material.aoMap.needsUpdate = true;
      mesh.material.needsUpdate = true;
    }
  }

  // 取景与网格辅助覆盖整个模型（对象不再参与筛选）。
  function selectionBox() {
    const box = new THREE.Box3();
    for (const bounds of objectBounds.values()) box.union(bounds);
    return box.isEmpty() ? null : box;
  }

  function selectedBounds() {
    const box = selectionBox();
    return box ? box.getSize(scratchSize).length() : 0;
  }

  function frameSelection(resetDirection = false) {
    const box = selectionBox();
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

  // 标准视图（Blender/主流 DCC 惯例的 1/3/7）：保持距离与目标，只换方位角。
  function setStandardView(axis) {
    if (!camera || !controls) return;
    const directions = { front: [0, 0, 1], side: [1, 0, 0], top: [0, 1, 0] };
    const direction = directions[axis];
    if (!direction) return;
    const distance = camera.position.distanceTo(controls.target) || 1;
    camera.position.copy(controls.target).add(new THREE.Vector3(...direction).multiplyScalar(distance));
    controls.update();
  }

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
    const container = $('materials');
    container.replaceChildren();
    for (const item of model.materials) {
      const row = document.createElement('div');
      row.className = 'bake-select-row';
      const input = document.createElement('input');
      input.type = 'checkbox';
      input.checked = materials.has(item.id);
      input.setAttribute('aria-label', `材质 ${item.id} ${item.name}`);
      input.onchange = () => {
        if (input.checked) materials.add(item.id);
        else materials.delete(item.id);
        syncMeshVisibility();
        refresh();
      };
      const label = document.createElement('button');
      label.className = 'bake-item';
      label.type = 'button';
      label.textContent = item.name;
      label.title = item.name;
      label.dataset.material = item.id;
      label.onclick = () => focusMaterial(item.id);
      row.append(input, label);
      container.append(row);
    }
  }

  const reportSignature = () => JSON.stringify(
    // channels 的键序不稳定，排序后的键值对参与签名，避免同参数被判成新检查。
    Object.entries(channels).sort(([a], [b]) => a < b ? -1 : a > b ? 1 : 0),
  );

  function syncInspecting() {
    inspecting = reportInFlight || reportPending || reportTimer !== undefined;
    refresh();
  }

  function scheduleRefreshReports() {
    if (!model) return;
    clearTimeout(reportTimer);
    const signature = reportSignature();
    if (signature === lastReportSignature) {
      // 参数与上次成功检查一致：复用报告，不置 inspecting、不发请求。
      reportTimer = undefined;
      reportPending = false;
      if (reportInFlight && inFlightSignature !== signature) ++reportRevision;
      inspectionError = '';
      drawUv();
      syncInspecting();
      return;
    }
    if (reportInFlight && inFlightSignature === signature) { syncInspecting(); return; }
    inspectionError = '';
    // 300ms trailing：连续勾选/切换通道只在操作停止后检查一次。
    reportTimer = setTimeout(() => { reportTimer = undefined; refreshReports(); }, 300);
    syncInspecting();
  }

  async function refreshReports() {
    if (!model || disposed) return;
    const signature = reportSignature();
    if (signature === lastReportSignature) {
      reportPending = false;
      if (reportInFlight && inFlightSignature !== signature) ++reportRevision;
      inspectionError = '';
      drawUv();
      syncInspecting();
      return;
    }
    if (reportInFlight) {
      if (inFlightSignature === signature) return;
      // 后端任务锁一次只允许一个 worker：在途时不并发，只排队，结束后用最新参数补跑。
      reportPending = true;
      syncInspecting();
      return;
    }
    reportPending = false;
    const revision = ++reportRevision;
    inFlightSignature = signature;
    reportInFlight = true;
    inspectionError = '';
    syncInspecting();
    try {
      // 对象不再参与筛选：始终传全部对象 id（worker 按此过滤三角形）。
      const reports = await invoke('bake_inspect', { handle: model.handle, objects: model.objects.map(item => item.id), channels });
      if (revision !== reportRevision || disposed) return;
      for (const report of reports) {
        const material = model.materials.find(item => item.id === report.material);
        const index = material.channels.findIndex(channel => channel.channel === report.channel);
        if (index >= 0) material.channels[index] = report;
        else material.channels.push(report);
      }
      // 只有成功才记签名：失败的检查必须允许重试，不能被缓存跳过。
      lastReportSignature = signature;
      drawUv();
    } catch (error) {
      if (revision === reportRevision) { inspectionError = String(error); status(`UV 检查失败：${error}`); }
    } finally {
      reportInFlight = false;
      if (reportPending && !disposed) {
        reportPending = false;
        refreshReports();
      } else {
        reportPending = false;
        syncInspecting();
      }
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
      if (triangle.material !== focused) return;
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
    $('open-output').disabled = !desktop || !outputDirectory;
    root.querySelectorAll('[data-bake-export]').forEach(button => button.disabled = !desktop || !results.length || running);
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
      // 大模型 JSON 在主线程解析需数秒到十数秒：先让状态提示上屏，避免界面被误判为卡死。
      status('正在解析网格数据（大型模型可能需数秒）…');
      await nextPaint();
      const mesh = await response.json();
      if (!data.materials?.length || !data.objects?.length || !mesh.triangles?.length) throw Error('模型没有可用的网格或材质');
      if (model) await invoke('bake_release', { handle: model.handle });
      // 新模型已带全量检查报告：丢弃旧防抖/排队与成功签名，避免把新参数误判为已检查。
      clearTimeout(reportTimer); reportTimer = undefined; reportPending = false; lastReportSignature = '';
      ++reportRevision; inspecting = false; inspectionError = '';
      aoTextures.forEach(texture => texture.dispose()); aoTextures.clear();
      outputDirectory = '';
      model = data;
      pendingModel = null;
      geometry = mesh;
      highlightedTriangle = null;
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
      status('正在建立三维预览…');
      await nextPaint();
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

  async function exportResults() {
    if (!desktop || !results.length || running) return;
    try {
      const directory = await open({ directory: true });
      if (!directory) return;
      const data = await invoke('bake_export', { files: results.map(file => file.path), directory });
      status(`已导出 ${data.exported} 张贴图到 ${data.directory}`);
      notify(`已导出 ${data.exported} 张贴图`);
    } catch (error) {
      status(`导出失败：${error}`);
      notify(error);
    }
  }

  function showResults(data) {
    results = data.files || [];
    resultChannels = { ...channels };
    outputDirectory = data.directory;
    $('results').replaceChildren();
    const toolbar = document.createElement('div');
    toolbar.className = 'bake-results-toolbar';
    const exportButton = document.createElement('button');
    exportButton.className = 'secondary-action'; exportButton.type = 'button';
    exportButton.dataset.bakeExport = '';
    exportButton.textContent = '导出全部贴图';
    exportButton.onclick = () => exportResults();
    const openCache = document.createElement('button');
    openCache.className = 'secondary-action'; openCache.type = 'button';
    openCache.textContent = '打开缓存目录';
    openCache.onclick = () => openPath(data.directory).catch(notify);
    const hint = document.createElement('small');
    hint.textContent = '结果缓存在应用数据目录，导出时选择目标文件夹';
    toolbar.append(exportButton, openCache, hint);
    $('results').append(toolbar);
    for (const file of results) {
      const card = document.createElement('div');
      card.className = 'bake-result';
      const materialName = model?.materials.find(item => item.id === file.material)?.name;
      const label = materialName ? `${materialName} · ${file.kind.toUpperCase()}` : `材质 ${file.material} · ${file.kind.toUpperCase()}`;
      const title = document.createElement('p');
      title.textContent = label;
      const image = document.createElement('img');
      // 一次烘焙最多“材质数×3”张全分辨率贴图（4K 单张解码约 67MB）；
      // 懒加载把解码推迟到卡片接近视口，避免批量解码的内存尖峰。
      image.loading = 'lazy';
      image.decoding = 'async';
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
          options: { ...options, device: +stored.device, objects: model.objects.map(item => item.id), materials: [...materials], channels: { ...channels }, distance: +$('distance').value },
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
  $('open-output').onclick = () => { if (outputDirectory) openPath(outputDirectory).catch(notify); };
  root.querySelectorAll('[data-bake-export]').forEach(button => { button.onclick = () => exportResults(); });
  $('channel').onchange = () => {
    if (focused === null) return;
    channels[focused] = +$('channel').value;
    refreshMaterialUv(focused);
    scheduleRefreshReports();
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
    if (!element || key === 'workspace') continue;
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
  $('check-uv').onclick = () => { setView('uv'); scheduleRefreshReports(); };
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
      materials = new Set(button.dataset.bakeSelect === 'all' ? model.materials.map(item => item.id) : []);
      lists(); syncMeshVisibility(); scheduleRefreshReports(); refresh();
    };
  });
  const standardViews = { 1: 'front', 3: 'side', 7: 'top' };
  const keyboard = event => {
    if (!active) return;
    // Alt：按住时左键临时映射为旋转（Marmoset/Substance 习惯），松开归还点选。
    if (event.key === 'Alt') {
      if (controls && view === 'model' && !event.target.closest('input, select, textarea')) {
        controls.mouseButtons.LEFT = event.type === 'keydown' ? THREE.MOUSE.ROTATE : null;
        if (event.type === 'keydown') event.preventDefault();
      }
      return;
    }
    if (event.ctrlKey || event.metaKey || event.altKey || event.target.closest('input, select, textarea, button, [role="combobox"], dialog')) return;
    if (event.key.toLowerCase() === 'f' && model && view === 'model') { event.preventDefault(); frameSelection(); }
    const view_ = standardViews[event.key];
    if (view_ && model && view === 'model') { event.preventDefault(); setStandardView(view_); }
  };
  document.addEventListener('keydown', keyboard);
  document.addEventListener('keyup', keyboard);
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
      clearTimeout(reportTimer);
      cancelAnimationFrame(renderFrame);
      resizeObserver?.disconnect();
      document.removeEventListener('keydown', keyboard);
      document.removeEventListener('keyup', keyboard);
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
