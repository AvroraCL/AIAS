import * as THREE from 'three';
import { OrbitControls } from 'three/addons/controls/OrbitControls.js';
import './model-bake.css';
import { bakeDefaults as defaults, restoreBakeSettings } from './model-bake-state.mjs';

export function createModelBake({ root, desktop, invoke, open, openPath, convertFileSrc, listen, settings, save, busy, withLog, notify, syncSelect = () => {}, progress }) {
  const stored = restoreBakeSettings(settings);
  let model = null, geometry = null, active = false, running = false, loading = false, exporting = false, disposed = false, job = '', view = 'model';
  let materials = new Set(), channels = {}, focused = null, results = [];
  let renderer, controls, scene, camera, group, resizeObserver;
  let highlightedTriangle = null, inspecting = false, inspectionError = '', cancelling = false, oidnReady = false, oidnSource = '', oidnStatusResolved = !desktop;
  let failedMaterials = new Set();
  let narrowPanel = 'settings';
  const resultTextures = new Map();
  let previewMaterialRevision = 0, resultTextureEpoch = 0, lastBakeProgress = 0, pendingResultPreview = '';
  let resultChannels = {};
  let capabilitiesRequested = false;
  let saveTimer, renderFrame, unlisten, outputDirectory = '', resultHandle = '', reportRevision = 0, orthographicHeight = 2, renderWidth = 0, renderHeight = 0;
  let importRevision = 0, previewRequest = 0, uvDrawRevision = 0;
  let endUvPan = () => {};
  // UV 视图变换：zoom=1 时 pan 恒为基准偏移；issue 定位自动居中放大
  let uvZoom = 1, uvPanX = 0, uvPanY = 0;
  const previewPending = new Map();
  const previewWorker = typeof Worker === 'function' ? new Worker(new URL('./model-bake-preview-worker.js', import.meta.url), { type: 'module' }) : null;
  if (previewWorker) {
    previewWorker.onmessage = event => {
      const pending = previewPending.get(event.data.id);
      if (!pending) return;
      previewPending.delete(event.data.id);
      if (event.data.error) pending.reject(Error(event.data.error)); else pending.resolve(event.data.buffer);
    };
    // worker 脚本加载失败/被终止时若不应答，导入会永久挂在 loadPreview 且
    // UI 全锁；统一拒绝并清空在途请求，让导入以明确错误收场。
    const rejectAllPending = () => {
      for (const pending of previewPending.values()) pending.reject(Error('预览 worker 已崩溃，请重试导入'));
      previewPending.clear();
    };
    previewWorker.onerror = rejectAllPending;
    previewWorker.onmessageerror = rejectAllPending;
  }
  let reportTimer, reportPending = false, reportInFlight = false, inFlightSignature = '', lastReportSignature = '';
  const objectBounds = new Map();
  const scratchSize = new THREE.Vector3();

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
        <p>生成智能材质识别所需的 Mesh Maps，并直接在模型上检查结果。</p>
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

      <div id="bake-live-progress" class="bake-live-progress" hidden role="status" aria-live="polite">
        <div class="bake-live-heading"><span class="bake-live-spinner" aria-hidden="true"></span><div><small>实时烘焙阶段</small><strong id="bake-live-phase">准备烘焙任务</strong></div><b id="bake-live-percent">0%</b></div>
        <div class="bake-live-track" aria-hidden="true"><span id="bake-live-fill"></span></div>
        <div class="bake-live-meta"><span id="bake-live-material">正在准备材质</span><span id="bake-live-map">等待 Worker</span></div>
        <div class="bake-live-stages" aria-label="烘焙阶段"><span data-bake-stage="prepare">准备</span><span data-bake-stage="mesh_map">几何图</span><span data-bake-stage="ao">AO</span><span data-bake-stage="denoise">降噪</span><span data-bake-stage="thickness">厚度</span><span data-bake-stage="finalize">整理</span></div>
      </div>

      <div class="bake-display-tools" aria-label="视图显示工具">
        <label class="bake-map-preview-control">贴图预览<select id="bake-map-preview" aria-label="模型贴图预览"><option value="ao">环境遮蔽</option><option value="curvature">曲率</option><option value="world_normal">世界空间法线</option><option value="position">位置</option><option value="thickness">厚度</option><option value="normal">切线法线</option><option value="id">材质 ID</option><option value="uv">UV 线框</option><option value="material">着色 + AO</option></select></label>
        <button id="bake-focus" data-bake-display="focus" class="bake-floating-button" type="button" title="聚焦所选对象 · F" aria-label="聚焦所选对象"><i data-lucide="scan" aria-hidden="true"></i></button>
        <button id="bake-reset" data-bake-display="reset" class="bake-floating-button" type="button" title="复位视图" aria-label="复位视图"><i data-lucide="rotate-ccw" aria-hidden="true"></i></button>
        <button id="bake-projection" data-bake-display="projection" class="bake-floating-button" type="button" title="切换透视 / 正交">透视</button>
        <details class="bake-display-menu"><summary title="显示选项"><i data-lucide="eye" aria-hidden="true"></i><span>显示</span></summary><div>
          <button id="bake-wireframe" data-bake-display="wireframe" type="button">线框</button>
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
          ${section('01 / 智能材质 Mesh Maps', `${check('ao', '环境遮蔽 AO')}${check('curvature', '曲率 Curvature')}${check('worldNormal', '世界空间法线')}${check('position', '位置 Position')}${check('thickness', '厚度 Thickness')}${check('normal', '切线空间法线（同模平面）')}${check('id', '材质 ID')}${check('uv', 'UV 线框（检查用）')}<small id="bake-output-summary">按所选材质分别导出</small>`)}
          ${section('02 / 烘焙质量', `<div class="bake-presets" aria-label="质量预设"><button data-bake-preset="draft" type="button">快速</button><button data-bake-preset="standard" type="button">标准</button><button data-bake-preset="high" type="button">精细</button></div>${select('resolution', '贴图尺寸', [512, 1024, 2048, 4096].map(value => [value, `${value} × ${value}`]))}<small id="bake-quality-note"></small>`)}
          ${section('03 / 导出', `<button id="bake-export" class="secondary-action output-action" data-bake-export type="button"><i data-lucide="download" aria-hidden="true"></i>导出全部贴图</button><button id="bake-open-output" class="secondary-action output-action" type="button"><i data-lucide="folder-open" aria-hidden="true"></i>打开缓存目录</button><small>结果先缓存在应用数据目录，导出时选择目标文件夹</small>`)}
          <details class="bake-advanced"><summary>高级设置</summary>
          ${section('UV 工作流', `${select('uvMode', '导入时 UV 处理', [['preserveValid', '智能保留'], ['regenerateAll', '全部重新展开'], ['strictSource', '严格使用源 UV']])}<small>智能保留会优先使用合格源 UV，仅修复缺失或越界的材质；UV 重叠（镜像/分层堆叠）与零面积退化面视为无害设计，保留原样。烘焙贴图供智能材质制作使用。</small>`)}
          ${section('计算设备', `${select('device', 'GPU', [[0, '检测设备中…']])}<small id="bake-device-note"></small>`)}
          ${section('光线追踪与边缘', `${select('samples', 'AO / 厚度采样', [32, 64, 128, 256].map(value => [value, `${value} 次`]))}${select('bits', '输出位深', [[8, '8 位'], [16, '16 位（AO/厚度/曲率/位置/世界法线）']])}<label>边缘扩展（px）<input id="bake-margin" type="number" min="0" max="128" value="16"></label><label>射线距离<input id="bake-distance" type="number" min="0.000001" step="any" value="1"></label><small id="bake-distance-note">默认包围盒对角线的 10%</small>${check('denoise', 'AI 降噪（仅用于 AO）')}<button id="bake-oidn-download" class="secondary-action" type="button" hidden>下载降噪组件</button><small id="bake-oidn-note"></small>${select('selfOnly', '遮挡范围', [['false', '全部对象互相影响'], ['true', '仅同一对象']])}<small>AO 与厚度使用 GPU；其余 Mesh Map 由模型几何直接生成。</small>`)}
          </details>
        </div>
      </aside>

      <footer class="bake-footer"><div class="bake-status-panel"><span id="bake-readiness"></span><span id="bake-status" role="status"></span><progress id="bake-progress" max="1" value="0" hidden aria-label="烘焙进度"></progress></div>
      <div class="bake-run-actions"><button id="bake-cancel" class="secondary-action" type="button" hidden><span>取消烘焙</span></button><button id="bake-run" class="run-button hidden" type="button"><span>开始烘焙</span></button></div></footer>
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
  const meshMapKeys = ['ao', 'curvature', 'worldNormal', 'position', 'thickness', 'normal', 'id', 'uv'];
  const status = text => { $('status').textContent = text; };
  const progressMapNames = { ao: '环境遮蔽', curvature: '曲率', world_normal: '世界空间法线', position: '位置', thickness: '厚度', normal: '切线空间法线', id: '材质 ID', uv: 'UV 线框' };
  function updateBakeProgress(data = {}) {
    const raw = Math.max(0, Math.min(1, Number(data.progress) || 0));
    const value = running ? Math.max(lastBakeProgress, raw) : raw;
    if (running) lastBakeProgress = value;
    // 取消请求期间 worker 仍会在文件边界间继续发进度：阶段文案保持"正在
    // 取消"，避免提示一闪即被后续 progress 事件覆盖。
    const phase = cancelling ? '正在取消，保留已完整写入的结果…' : (data.phase || (running ? '准备烘焙任务' : '正在处理…'));
    status(phase);
    $('progress').value = value;
    progress?.(value, phase);
    if (!running) return;
    const live = $('live-progress');
    live.hidden = false;
    $('live-phase').textContent = phase;
    $('live-percent').textContent = `${Math.round(value * 100)}%`;
    $('live-fill').style.width = `${(value * 100).toFixed(2)}%`;
    $('live-material').textContent = data.materialPosition
      ? `材质 ${data.materialPosition} / ${data.materialTotal}${Number.isInteger(data.material) ? ` · ID ${data.material}` : ''}`
      : '正在准备全部材质';
    $('live-map').textContent = data.mapPosition
      ? `贴图 ${data.mapPosition} / ${data.mapTotal}${data.map ? ` · ${progressMapNames[data.map] || data.map}` : ''}`
      : '准备输出贴图';
    let activeStage = data.stage || 'prepare';
    if (activeStage === 'raster' || activeStage === 'prepare_gpu') activeStage = 'prepare';
    if (activeStage === 'save') activeStage = data.map === 'ao' ? 'ao' : data.map === 'thickness' ? 'thickness' : 'mesh_map';
    root.querySelectorAll('[data-bake-stage]').forEach(element => element.classList.toggle('active', element.dataset.bakeStage === activeStage));
  }
  const setActionBusy = (button, value) => {
    if (!button) return;
    button.classList.toggle('busy', value);
    button.dataset.busy = String(value);
    button.setAttribute('aria-busy', String(value));
    button.disabled = value;
  };
  const loadPreview = preview => {
    const url = convertFileSrc(preview.bufferPath);
    if (!previewWorker) return fetch(url).then(response => { if (!response.ok) throw Error('无法读取预览数据'); return response.arrayBuffer(); });
    const id = ++previewRequest;
    return new Promise((resolve, reject) => {
      previewPending.set(id, { resolve, reject });
      previewWorker.postMessage({ id, url, expectedBytes: preview.byteLength });
    });
  };
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
    if (running || loading || exporting || busy()) return '任务正在运行。';
    if (inspecting) return '正在检查 UV…';
    if (inspectionError) return 'UV 检查失败，请重新检查后烘焙。';
    if (!model) return '请导入模型。';
    if (!materials.size) return '请选择输出材质。';
    if (!meshMapKeys.some(key => stored[key])) return '请选择输出类型。';
    if (!Number.isFinite(+$('distance').value) || +$('distance').value <= 0) return '遮蔽距离必须大于 0。';
    if (!Number.isInteger(+$('margin').value) || +$('margin').value < 0 || +$('margin').value > 128) return '边缘扩展应为 0–128 的整数。';
    if ((stored.ao || stored.thickness) && !devices.find(device => device.index === +stored.device)?.supported) return '当前设备不支持 DXR 1.1 光线追踪。';
    if (stored.ao && stored.denoise && desktop && !oidnReady) {
      // 首次状态查询未返回前不判定"未下载"：内置组件的查询是毫秒级，
      // 误报会直接挡住烘焙并诱导用户手动下载。
      return oidnStatusResolved ? 'AI 降噪组件未下载，请先点击「下载降噪组件」。' : '正在检查内置 OIDN 组件…';
    }
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

  function syncDisplayControls() {
    const workspace = stored.workspace;
    const projection = $('projection');
    projection.textContent = workspace.projection === 'perspective' ? '透视' : '正交';
    projection.setAttribute('aria-pressed', String(workspace.projection === 'orthographic'));
    const button = $('wireframe');
    button.classList.toggle('active', workspace.wireframe);
    button.setAttribute('aria-pressed', String(workspace.wireframe));
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
      scene.add(group);
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
      mesh.material.dispose();
      group.remove(mesh);
    }
  }

  function disposePreparedMeshes(meshes) {
    for (const mesh of meshes || []) {
      mesh.geometry.dispose();
      mesh.material.dispose();
    }
  }

  function createBaseMaterial(aoMap = null) {
    const material = new THREE.MeshStandardMaterial({ color: 0xb8b8b8, roughness: 0.85, side: THREE.DoubleSide, wireframe: stored.workspace.wireframe });
    if (aoMap) {
      material.aoMap = aoMap;
      material.aoMapIntensity = 1.35;
    }
    return material;
  }

  function resetPreviewMaterials(predicate = () => true) {
    for (const mesh of group?.children || []) {
      if (!predicate(mesh)) continue;
      const previous = mesh.material;
      mesh.material = createBaseMaterial();
      previous.dispose();
    }
  }

  function clearResultTextures() {
    ++resultTextureEpoch;
    ++previewMaterialRevision;
    resultTextures.forEach(texture => {
      texture.image?.close?.();
      texture.dispose();
    });
    resultTextures.clear();
  }

  function retainResultTextures(keep) {
    for (const [key, texture] of resultTextures) {
      if (keep.has(texture)) continue;
      texture.image?.close?.();
      texture.dispose();
      resultTextures.delete(key);
    }
  }

  function loadResultTexture(file, usage) {
    const key = `${usage}:${file.path}`;
    const cached = resultTextures.get(key);
    if (cached) return Promise.resolve(cached);
    const epoch = resultTextureEpoch;
    const commit = texture => {
        if (disposed || epoch !== resultTextureEpoch) {
          texture.image?.close?.();
          texture.dispose();
          throw Error('贴图预览已过期');
        }
        texture.colorSpace = usage === 'ao' ? THREE.NoColorSpace : THREE.SRGBColorSpace;
        texture.needsUpdate = true;
        resultTextures.set(key, texture);
        return texture;
    };
    if (typeof createImageBitmap === 'function') {
      return fetch(convertFileSrc(file.path))
        .then(response => { if (!response.ok) throw Error(`无法读取贴图（${response.status}）`); return response.blob(); })
        // 1024：预览是用户判断 AO 边缘/破面的依据，512 会抹平边缘细节；
        // 4K 原图解码后降到 1024 的内存与耗时仍远低于原尺寸。
        .then(blob => createImageBitmap(blob, { resizeWidth: 1024, resizeHeight: 1024, resizeQuality: 'high', imageOrientation: 'flipY' }))
        .then(bitmap => commit(new THREE.Texture(bitmap)));
    }
    return new Promise((resolve, reject) => {
      new THREE.TextureLoader().load(convertFileSrc(file.path), texture => {
        try { resolve(commit(texture)); } catch (error) { reject(error); }
      }, undefined, reject);
    });
  }

  async function applyMapPreview(kind = stored.workspace.mapPreview, announce = true, switchView = true) {
    // 显式应用结果时消费掉“切回模型后应用默认结果”的待办，避免 setView('model')
    // 再启动一次默认预览并覆盖用户刚选择的贴图。
    pendingResultPreview = '';
    if (!group || !model || !results.length) return;
    const mismatched = new Set([...materials].filter(id => (resultChannels[id] ?? 0) !== (channels[id] ?? 0)));
    if (mismatched.size) {
      resetPreviewMaterials(mesh => mismatched.has(mesh.userData.material));
      status('烘焙结果使用了不同的 UV 通道，请切回烘焙通道后预览。');
      return;
    }
    const revision = ++previewMaterialRevision;
    const sourceKind = kind === 'material' ? 'ao' : kind;
    const files = results.filter(file => file.kind === sourceKind);
    if (!files.length) {
      $('map-preview').value = stored.workspace.mapPreview;
      status(kind === 'material' ? '本次结果没有 AO，无法显示着色 + AO。' : '本次结果没有生成该贴图。');
      return;
    }
    $('map-preview').value = kind;
    if (announce) status(`正在模型上载入${$('map-preview').selectedOptions[0]?.textContent || '贴图'}…`);
    const textures = new Map();
    for (const file of files) {
      try {
        textures.set(file.material, await loadResultTexture(file, kind === 'material' ? 'ao' : 'display'));
      } catch (error) {
        if (revision === previewMaterialRevision) notify(`贴图预览加载失败：${error}`);
      }
      if (revision !== previewMaterialRevision || disposed) return;
      await nextPaint();
    }
    if (revision !== previewMaterialRevision || disposed) return;
    for (const mesh of group.children) {
      const texture = textures.get(mesh.userData.material) || null;
      const previous = mesh.material;
      mesh.material = kind === 'material'
        ? createBaseMaterial(texture)
        : new THREE.MeshBasicMaterial({ map: texture, color: texture ? 0xffffff : 0x2c2d30, side: THREE.DoubleSide, wireframe: stored.workspace.wireframe });
      previous.dispose();
    }
    // 一种 1024 预览 × 25 材质约占 100 MB；若把浏览过的全部 Mesh Map
    // 永久缓存，完整切换一轮可超过 700 MB。材质已解除旧引用后只保留当前组。
    retainResultTextures(new Set(textures.values()));
    stored.workspace.mapPreview = kind;
    if (switchView) setView('model');
    syncDisplayControls();
    persist();
    if (announce) status(kind === 'material' ? '已在模型上显示着色 + AO 预览。' : `正在模型上预览${$('map-preview').selectedOptions[0]?.textContent || '贴图'}。`);
  }

  // 一次性为全部 (对象,材质) 组合建立网格，并按对象预计算包围盒：之后选择变化只切
  // mesh.visible，不再重扫三角形重建几何体（该函数只允许在导入/几何体变化时调用）。
  async function buildMeshes(revision, nextModel, nextGeometry, nextChannels) {
    init3d();
    if (!group || !nextGeometry) return { meshes: [], bounds: new Map() };
    const meshes = [];
    const bounds = new Map();
    const modelBounds = new THREE.Box3(new THREE.Vector3(...nextModel.bounds[0]), new THREE.Vector3(...nextModel.bounds[1]));
    for (const object of nextModel.objects) bounds.set(object.id, modelBounds);
    let sliceStarted = performance.now();
    for (const [batchIndex, batch] of nextGeometry.batches.entries()) {
      if (revision !== importRevision || disposed) {
        disposePreparedMeshes(meshes);
        throw Error('导入已被新的模型替代');
      }
      const meshGeometry = new THREE.BufferGeometry();
      meshGeometry.setAttribute('position', new THREE.BufferAttribute(new Float32Array(nextGeometry.buffer, batch.positionOffset, batch.vertexCount * 3), 3));
      // 预览必须保留导入模型的逐角法线和硬边，才能与法线/曲率烘焙结果一致。
      // 缺失或无效法线已由工作进程回退为几何面法线，前端不再按位置二次焊合。
      meshGeometry.setAttribute('normal', new THREE.BufferAttribute(new Float32Array(nextGeometry.buffer, batch.normalOffset, batch.vertexCount * 3), 3));
      const uvOffset = batch.uvOffsets[String(nextChannels[batch.material] ?? 0)];
      const uv = uvOffset == null ? new Float32Array(batch.vertexCount * 2) : new Float32Array(nextGeometry.buffer, uvOffset, batch.vertexCount * 2);
      meshGeometry.setAttribute('uv', new THREE.BufferAttribute(uv, 2));
      const material = createBaseMaterial();
      const mesh = new THREE.Mesh(meshGeometry, material);
      mesh.userData.material = batch.material;
      mesh.userData.object = batch.object;
      mesh.userData.batch = batch;
      meshes.push(mesh);
      if (performance.now() - sliceStarted >= 9 || batchIndex === nextGeometry.batches.length - 1) {
        await nextPaint();
        sliceStarted = performance.now();
      }
    }
    return { meshes, bounds };
  }

  function installMeshes(prepared) {
    clearMeshes();
    objectBounds.clear();
    for (const [id, bounds] of prepared.bounds) objectBounds.set(id, bounds);
    for (const mesh of prepared.meshes) group?.add(mesh);
    syncMeshVisibility();
    syncDisplayControls();
  }

  function syncMeshVisibility() {
    if (!group) return;
    for (const mesh of group.children) mesh.visible = materials.has(mesh.userData.material);
  }

  // 通道切换只更新该材质的 uv 属性；已有结果仍绑定烘焙时的通道。
  function refreshMaterialUv(id) {
    if (!group || !geometry) return;
    const channel = channels[id] ?? 0;
    for (const mesh of group.children) {
      if (mesh.userData.material !== id) continue;
      const batch = mesh.userData.batch;
      const offset = batch.uvOffsets[String(channel)];
      const next = offset == null ? new Float32Array(batch.vertexCount * 2) : new Float32Array(geometry.buffer, offset, batch.vertexCount * 2);
      mesh.geometry.setAttribute('uv', new THREE.BufferAttribute(next, 2));
    }
    if (results.length) applyMapPreview(stored.workspace.mapPreview, false, false).catch(error => notify(`贴图预览加载失败：${error}`));
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
    $('channel').replaceChildren(...material.channels.map(channel => new Option(channel.generated ? `自动 UV · UV${channel.channel}` : `源 UV${channel.channel}`, channel.channel)));
    $('channel').value = String(channels[id] ?? 0);
    syncSelect($('channel'));
    drawUv();
    $('materials').querySelectorAll('button').forEach(button => button.classList.toggle('active', +button.dataset.material === id));
  }

  // 材质勾选支持按住左键滑动批量勾选：在起始复选框上按下时确定目标状态，
  // 拖动经过的复选框统一应用该状态，松开/取消指针结束会话。鼠标无隐式指针
  // 捕获，pointerenter 在按住拖动时照常触发，正好用作扫描目标。
  let checkboxDrag = null;
  function setMaterialChecked(input, id, checked) {
    if (input.checked === checked) return;
    input.checked = checked;
    if (checked) materials.add(id);
    else materials.delete(id);
    syncMeshVisibility();
    refresh();
  }

  function lists() {
    const container = $('materials');
    container.replaceChildren();
    $('object-count').textContent = `(${model.materials.length})`;
    for (const item of model.materials) {
      const row = document.createElement('div');
      row.className = 'bake-select-row';
      const input = document.createElement('input');
      input.type = 'checkbox';
      input.checked = materials.has(item.id);
      input.setAttribute('aria-label', `材质 ${item.id} ${item.name}`);
      input.onchange = () => setMaterialChecked(input, item.id, input.checked);
      input.onpointerdown = event => {
        if (event.button !== 0 || checkboxDrag !== null) return;
        event.preventDefault();
        setMaterialChecked(input, item.id, !input.checked);
        checkboxDrag = input.checked;
      };
      input.onpointerenter = () => {
        if (checkboxDrag === null) return;
        setMaterialChecked(input, item.id, checkboxDrag);
      };
      // 按下时已手动切换过状态，pointer 起源的 click 一律拦截默认翻转：
      // pointerup 先于 click 清掉会话标志，不能拿它判断是否拦截；键盘
      // 空格合成的 click detail 为 0，保持原生切换与 change 行为不受影响。
      input.onclick = event => {
        if (event.detail > 0) event.preventDefault();
      };
      const label = document.createElement('button');
      label.className = 'bake-item' + (failedMaterials.has(item.id) ? ' bake-failed' : '');
      label.type = 'button';
      label.textContent = item.name + (failedMaterials.has(item.id) ? ' ⚠' : '');
      label.title = failedMaterials.has(item.id) ? `${item.name}（上次烘焙失败）` : item.name;
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

  function drawUv(highlight = highlightedTriangle, focusTriangle = null) {
    highlightedTriangle = highlight;
    if (!model || focused === null) return;
    if (view !== 'uv') { ++uvDrawRevision; return; }
    const applyFocus = (valuesFor, finder) => {
      if (focusTriangle == null) return;
      // 以目标三角形 UV 包围盒中心居中放大（zoom 至少 4），便于定位小缺陷面
      const found = finder(focusTriangle);
      if (!found) return;
      const { minU, maxU, minV, maxV } = found;
      uvZoom = Math.max(uvZoom, 4);
      uvPanX = size / 2 - 12 - ((minU + maxU) / 2) * (size - 24) * uvZoom;
      uvPanY = size / 2 - 12 - (1 - (minV + maxV) / 2) * (size - 24) * uvZoom;
    };
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
    const drawRevision = ++uvDrawRevision;
    const batches = geometry.batches.filter(batch => batch.material === focused && batch.uvOffsets[String(channels[focused] ?? 0)] != null)
      .map(batch => ({ batch, values: new Float32Array(geometry.buffer, batch.uvOffsets[String(channels[focused] ?? 0)], batch.vertexCount * 2), triangleIds: new Uint32Array(geometry.buffer, batch.triangleOffset, batch.triangleCount) }));
    if (focusTriangle != null) {
      applyFocus(valuesFor => {
        // 在批内查目标三角形的 UV 包围盒
        for (const { values, triangleIds } of batches) {
          const local = triangleIds.indexOf(focusTriangle);
          if (local >= 0) {
            const base = local * 6;
            return {
              minU: Math.min(values[base], values[base + 2], values[base + 4]),
              maxU: Math.max(values[base], values[base + 2], values[base + 4]),
              minV: Math.min(values[base + 1], values[base + 3], values[base + 5]),
              maxV: Math.max(values[base + 1], values[base + 3], values[base + 5]),
            };
          }
        }
        return null;
      });
    }
    let batchIndex = 0, triangle = 0;
    // 按样式分桶合并 path：同色三角形合成一条路径一次 stroke，绘制调用
    // 降一个数量级（大网格 4K 线框的 Canvas 成本大头就在逐三角形 stroke）。
    const buckets = {
      normal: { style: '#a8a8a8', width: 1, path: new Path2D() },
      bad: { style: '#ff6278', width: 1, path: new Path2D() },
      highlight: { style: '#ffd166', width: 3, path: new Path2D() },
    };
    const drawChunk = () => {
      if (drawRevision !== uvDrawRevision || view !== 'uv') return;
      const deadline = performance.now() + 9;
      while (batchIndex < batches.length && performance.now() < deadline) {
        const { batch, values, triangleIds } = batches[batchIndex];
        if (triangle >= batch.triangleCount) { batchIndex++; triangle = 0; continue; }
        const index = triangleIds[triangle];
        const bucket = highlight === index ? buckets.highlight : bad.has(index) ? buckets.bad : buckets.normal;
        const path = bucket.path;
        for (let point = 0; point < 3; point++) {
          const base = triangle * 6 + point * 2;
          const x = uvPanX + 12 + values[base] * (size - 24) * uvZoom;
          const y = uvPanY + 12 + (1 - values[base + 1]) * (size - 24) * uvZoom;
          if (point) path.lineTo(x, y); else path.moveTo(x, y);
        }
        path.closePath();
        triangle++;
      }
      if (batchIndex < batches.length) requestAnimationFrame(drawChunk);
      else {
        // bad 填充统一在收尾做一次：逐三角形对累计路径重复 fill 会让先画
        // 的问题面 alpha 越叠越深，把缺陷严重度渲染成“越靠前越红”。
        context.fillStyle = '#ff627833';
        context.fill(buckets.bad.path);
        for (const bucket of Object.values(buckets)) {
          context.strokeStyle = bucket.style;
          context.lineWidth = bucket.width;
          context.stroke(bucket.path);
        }
      }
    };
    drawChunk();
    const issues = $('issues');
    issues.replaceChildren();
    if (report?.issues?.length) {
      const note = document.createElement('p');
      const overlap = Math.max((report.issueCount ?? 0) - (report.defectCount ?? 0), 0);
      note.textContent = `缺陷 ${report.defectCount ?? 0} 处（缺失/越界），无害记录 ${overlap} 处（重叠/零面积退化，镜像分层设计常见）。烘焙按源 UV 原样进行。`;
      issues.append(note);
      for (const issue of report.issues.slice(0, 30)) {
        const button = document.createElement('button');
        button.className = 'bake-issue';
        button.type = 'button';
        button.textContent = `对象 ${issue.object} · 源面 ${issue.face}：${issue.kind}${issue.otherTriangle != null ? `（与三角形 ${issue.otherTriangle}）` : ''}`;
        button.onclick = () => { setView('uv'); drawUv(issue.triangle, issue.triangle); };
        issues.append(button);
      }
      if (report.issueCount > report.issues.length) {
        const truncation = document.createElement('small');
        truncation.textContent = `明细仅显示前 ${report.issues.length} 条，共 ${report.issueCount} 处。`;
        issues.append(truncation);
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
    if (next === 'uv' && model) { stored.workspace.outlinerOpen = true; narrowPanel = 'outliner'; persist(); }
    updatePanelState('outliner'); updatePanelState('settings');
    $('viewport-note').hidden = !model || next !== 'model' || running || loading;
    root.querySelector('.bake-display-tools').hidden = !model || next !== 'model';
    root.querySelectorAll('[data-bake-view]').forEach(button => {
      const selected = button.dataset.bakeView === next;
      button.classList.toggle('active', selected);
      button.setAttribute('aria-selected', String(selected));
    });
    if (next === 'uv') drawUv();
    refresh();
    if (next === 'model' && pendingResultPreview) {
      const kind = pendingResultPreview;
      pendingResultPreview = '';
      applyMapPreview(kind, false, false).catch(error => notify(`贴图预览加载失败：${error}`));
    }
  }

  function refresh() {
    const locked = running || loading || exporting || busy();
    root.querySelector('.bake-workspace').classList.toggle('has-model', Boolean(model));
    root.querySelectorAll('input, select, button').forEach(element => {
      const staysInteractive = element.dataset.bakeView || element.dataset.bakeDisplay || element.dataset.bakePanelToggle || element.dataset.bakeResultAction !== undefined || element.id === 'bake-cancel';
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
    $('progress').hidden = !(running || loading);
    $('live-progress').hidden = !running;
    $('viewport-note').hidden = !model || view !== 'model' || running || loading;
    const count = materials.size * meshMapKeys.filter(key => stored[key]).length;
    $('output-summary').textContent = `${materials.size} 个材质 · 预计 ${count} 张贴图`;
    const rayMaps = stored.ao || stored.thickness;
    $('quality-note').textContent = rayMaps ? `${stored.samples} 次光线采样 · ${stored.bits} 位灰度` : '几何 Mesh Map 不使用光线采样';
    for (const key of ['samples', 'bits', 'device', 'distance', 'selfOnly']) $(key).disabled = locked || !rayMaps;
    root.querySelectorAll('[data-bake-preset]').forEach(button => {
      const [resolution, samples] = presets[button.dataset.bakePreset];
      button.setAttribute('aria-pressed', String(stored.resolution === resolution && stored.samples === samples));
    });
    const device = devices.find(item => item.index === +stored.device);
    $('ao').disabled = locked || !device?.supported;
    $('thickness').disabled = locked || !device?.supported;
    $('ao').closest('label').title = device?.supported ? '使用 GPU 生成环境遮蔽' : '当前设备不支持 AO';
    $('thickness').closest('label').title = device?.supported ? '使用 GPU 生成厚度图' : '当前设备不支持厚度图';
    $('device-note').textContent = !desktop ? '桌面版可检测 DXR 显卡' : device ? `${device.supported ? 'DXR 1.1 可用' : device.reason} · 可用预算 ${(device.availableBytes / 1073741824).toFixed(1)} GiB${[4098, 32902].includes(device.vendor) ? '；按能力支持，待硬件实测' : ''}` : '无合格显卡；仍可导出几何 Mesh Maps';
    const reason = blocker();
    const oidnDownload = $('oidn-download');
    if (oidnDownload) {
      // 状态未决时不亮出下载按钮：内置组件存在的情况下闪现下载按钮是误导。
      oidnDownload.hidden = !desktop || !stored.ao || !stored.denoise || oidnReady || !oidnStatusResolved || locked;
      oidnDownload.disabled = locked;
    }
    $('oidn-note').textContent = !stored.ao || !stored.denoise ? '' : oidnReady ? (oidnSource === 'bundled' ? '已使用软件内置 OIDN 2.2.2。' : 'OIDN 2.2.2 已就绪。') : !desktop ? '桌面版已内置 OIDN 2.2.2。' : oidnStatusResolved ? '需要下载 OIDN 2.2.2。' : '正在检查内置 OIDN 组件…';
    refreshOidnStatus();
    $('run').disabled = Boolean(reason);
    $('run').title = reason || '';
    $('readiness').textContent = running ? (cancelling ? '正在取消…' : '正在烘焙') : loading ? '正在导入模型…' : exporting ? '正在导出结果…' : reason || `已就绪 · ${count} 张贴图`;
    $('open-output').disabled = exporting || !desktop || !outputDirectory;
    root.querySelectorAll('[data-bake-export]').forEach(button => button.disabled = exporting || !desktop || !results.length);
    // 新任务运行时，旧结果仍应能切换并应用到模型；这里只在导出切换句柄的
    // 短窗口内锁定，避免用户误以为旧结果已消失。
    $('map-preview').disabled = exporting || !results.length;
    const resultHint = root.querySelector('.bake-results-toolbar small');
    if (resultHint?.dataset.baseText) resultHint.textContent = running ? `上次结果 · ${resultHint.dataset.baseText}` : resultHint.dataset.baseText;
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
      for (const item of devices) device.add(new Option(`${item.displayName || item.name}${item.supported ? '' : '（不支持 AO）'}`, item.index));
      const saved = stored.deviceLuid && devices.find(item => item.luid === stored.deviceLuid);
      if (saved) stored.device = saved.index;
      if (!devices.some(item => item.index === +stored.device)) stored.device = devices.find(item => item.supported)?.index ?? devices[0]?.index ?? 0;
      stored.deviceLuid = devices.find(item => item.index === +stored.device)?.luid || '';
      device.value = String(stored.device);
      if (!devices.find(item => item.index === +stored.device)?.supported) {
        stored.ao = false;
        stored.thickness = false;
        $('ao').checked = false;
        $('thickness').checked = false;
      }
    } catch (error) {
      capabilitiesRequested = false;
      status(String(error));
      $('device').replaceChildren(new Option('设备检测失败', '0'));
      stored.ao = false;
      stored.thickness = false;
      $('ao').checked = false;
      $('thickness').checked = false;
    }
    refresh();
  }

  async function importModel(path) {
    if (!desktop || running || loading || exporting || busy()) return;
    loading = true; $('progress').value = 0;
    const revision = ++importRevision;
    let pendingModel = null, prepared = null;
    refresh();
    status('正在导入并检查模型…');
    try {
      const data = await invoke('bake_import', { path, uvMode: stored.uvMode });
      pendingModel = data;
      if (revision !== importRevision || disposed) throw Error('导入已被新的模型替代');
      status('正在载入紧凑三维预览…');
      const buffer = await loadPreview(data.preview);
      if (revision !== importRevision || disposed) throw Error('导入已被新的模型替代');
      if (!data.materials?.length || !data.objects?.length || !data.preview?.batches?.length || !buffer.byteLength) throw Error('模型没有可用的网格或材质');
      const nextGeometry = { buffer, batches: data.preview.batches };
      const nextChannels = Object.fromEntries(data.materials.map(item => [item.id, item.selectedChannel ?? 0]));
      status('正在建立三维预览…');
      await nextPaint();
      prepared = await buildMeshes(revision, data, nextGeometry, nextChannels);
      if (revision !== importRevision || disposed) throw Error('导入已被新的模型替代');

      const previousModel = model;
      const previousResultHandle = resultHandle;
      // 新模型已带全量检查报告：丢弃旧防抖/排队与成功签名，避免把新参数误判为已检查。
      clearTimeout(reportTimer); reportTimer = undefined; reportPending = false; lastReportSignature = '';
      ++reportRevision; inspecting = false; inspectionError = '';
      clearResultTextures();
      outputDirectory = '';
      model = data;
      geometry = nextGeometry;
      highlightedTriangle = null;
      materials = new Set(model.materials.map(item => item.id));
      channels = nextChannels;
      results = [];
      resultHandle = '';
      pendingResultPreview = '';
      $('results').replaceChildren();
      installMeshes(prepared); prepared = null;
      pendingModel = null;
      if (previousModel) invoke('bake_release', { handle: previousModel.handle }).catch(() => {});
      if (previousResultHandle) invoke('bake_result_release', { resultHandle: previousResultHandle }).catch(() => {});
      failedMaterials = new Set();
      $('model-info').textContent = `${model.name} · ${model.objects.length} 对象 · ${model.triangleCount.toLocaleString()} 三角形`;
      if (model.degenerateFaces > 0) {
        const examples = (model.degenerateExamples || []).join('、');
        notify(`已跳过 ${model.degenerateFaces} 个零面积退化面${examples ? `（${examples}${model.degenerateFaces > (model.degenerateExamples || []).length ? ' 等' : ''}）` : ''}，这些面对烘焙无影响。`);
      }
      for (const warning of model.warnings || []) notify(warning);
      lists();
      updatePanelState('outliner'); updatePanelState('settings');
      resizeRenderer(); reset();
      $('distance').value = String(selectedBounds() * stored.distanceRatio);
      $('distance-note').textContent = `单位：${model.units === '模型单位' ? '相对单位' : model.units}；所选包围盒对角线的 ${(stored.distanceRatio * 100).toFixed(1)}%`;
      focusMaterial(model.materials[0].id);
      setView('model');
      const generated = Object.keys(model.generatedChannels || {}).length;
      status(generated ? `模型已导入，${generated} 个材质已生成自动 UV。` : '模型已导入，源 UV 检查通过。');
    } catch (error) {
      if (prepared) disposePreparedMeshes(prepared.meshes);
      if (pendingModel) invoke('bake_release', { handle: pendingModel.handle }).catch(() => {});
      status(`导入失败：${error}`);
      notify(error);
    } finally {
      loading = false;
      refresh();
    }
  }

  async function exportResults(button = $('export')) {
    if (!desktop || !results.length || exporting) return;
    exporting = true; setActionBusy(button, true); refresh();
    try {
      const directory = await open({ directory: true });
      if (!directory) return;
      const data = await invoke('bake_export', resultHandle ? { resultHandle, directory } : { files: results.map(file => file.path), directory });
      status(`已导出 ${data.exported} 个文件到 ${data.directory}`);
      notify(`已导出 ${data.exported} 个文件`);
    } catch (error) {
      status(`导出失败：${error}`);
      notify(error);
    } finally {
      exporting = false; setActionBusy(button, false); refresh();
    }
  }

  async function showResults(data) {
    // 模块在烘焙期间被销毁（HMR/关窗）：新结果句柄立即释放，否则后端
    // RESULTS 表与缓存目录要留到重启才被清理。
    if (disposed) {
      if (data.resultHandle) invoke('bake_result_release', { resultHandle: data.resultHandle }).catch(() => {});
      return;
    }
    // worker 正常返回 failedMaterials；进程崩溃/强制取消的恢复路径返回
    // unfinished 明细。两者都要映射到材质徽标，否则部分结果看起来像全量成功。
    failedMaterials = new Set([
      ...(data.failedMaterials || []),
      ...(data.unfinished || []).map(item => Number(item.material)),
    ].filter(Number.isFinite));
    const previousHandle = resultHandle;
    clearResultTextures();
    // 贴图已 dispose 并 close() 位图，但网格材质可能仍引用它们（本函数只在
    // 模型视图下重建贴图材质）：先统一还原为无贴图基础材质，避免切回模型
    // 视图时渲染已释放的位图。模型视图下随后的 applyMapPreview 会重新应用。
    stored.workspace.mapPreview = 'material';
    $('map-preview').value = 'material';
    resetPreviewMaterials();
    results = data.files || [];
    resultHandle = data.resultHandle || '';
    // 以 worker 返回的实际通道为准。任务运行期间 UI 仍可切换视图，后续版本也
    // 可能允许调整通道；用当前 UI 状态会让不匹配的贴图被错误套到模型上。
    resultChannels = { ...(data.selectedChannels || channels) };
    outputDirectory = data.directory;
    $('results').replaceChildren();
    const toolbar = document.createElement('div');
    toolbar.className = 'bake-results-toolbar';
    const exportButton = document.createElement('button');
    exportButton.className = 'secondary-action'; exportButton.type = 'button';
    exportButton.dataset.bakeExport = '';
    exportButton.textContent = '导出全部贴图';
    exportButton.onclick = () => exportResults(exportButton);
    const openCache = document.createElement('button');
    openCache.className = 'secondary-action'; openCache.type = 'button';
    openCache.dataset.bakeResultAction = '';
    openCache.textContent = '打开缓存目录';
    openCache.onclick = () => openPath(data.directory).catch(notify);
    const hint = document.createElement('small');
    hint.dataset.baseText = '烘焙贴图与清单缓存在应用数据目录，导出时选择目标文件夹；贴图供智能材质制作使用';
    hint.textContent = hint.dataset.baseText;
    toolbar.append(exportButton, openCache, hint);
    $('results').append(toolbar);
    for (const file of results) {
      const card = document.createElement('div');
      card.className = 'bake-result';
      const materialName = model?.materials.find(item => item.id === file.material)?.name;
      const kindNames = { ao: '环境遮蔽', normal: '切线法线', world_normal: '世界空间法线', curvature: '曲率', position: '位置', thickness: '厚度', id: '材质 ID', uv: 'UV 线框' };
      const kindName = kindNames[file.kind] || file.kind.toUpperCase();
      const label = materialName ? `${materialName} · ${kindName}` : `材质 ${file.material} · ${kindName}`;
      const title = document.createElement('p');
      title.textContent = label;
      const image = document.createElement('img');
      // 一次烘焙可能生成“材质数×8”张全分辨率贴图（4K 单张解码约 67MB）；
      // 懒加载把解码推迟到卡片接近视口，避免批量解码的内存尖峰。
      image.loading = 'lazy';
      image.decoding = 'async';
      image.src = convertFileSrc(file.path);
      image.alt = title.textContent;
      const preview = document.createElement('button');
      preview.className = 'bake-result-preview';
      preview.dataset.bakeResultAction = '';
      preview.type = 'button'; preview.title = '放大查看贴图';
      preview.setAttribute('aria-label', `放大 ${title.textContent}`);
      preview.onclick = () => {
        const dialog = document.createElement('dialog'); dialog.className = 'bake-image-dialog';
        const close = document.createElement('button'); close.textContent = '关闭'; close.dataset.bakeResultAction = ''; close.onclick = () => dialog.close();
        const full = image.cloneNode();
        dialog.append(close, full); root.append(dialog);
        dialog.addEventListener('close', () => dialog.remove(), {once:true}); dialog.showModal();
      };
      preview.append(image);
      card.append(preview, title);
      const reveal = document.createElement('button'); reveal.className = 'secondary-action'; reveal.textContent = '打开文件';
      reveal.dataset.bakeResultAction = '';
      reveal.onclick = () => openPath(file.path).catch(notify); card.append(reveal);
      const apply = document.createElement('button');
      apply.className = 'secondary-action'; apply.type = 'button';
      apply.dataset.bakeResultAction = '';
      apply.textContent = '在模型上预览';
      apply.onclick = () => applyMapPreview(file.kind).catch(error => notify(`贴图预览加载失败：${error}`));
      card.append(apply);
      $('results').append(card);
    }
    if (previousHandle && previousHandle !== resultHandle) invoke('bake_result_release', { resultHandle: previousHandle }).catch(() => {});
    lists(); // 失败材质在左侧列表同步打 ⚠ 标记
    const defaultPreview = results.some(file => file.kind === 'ao') ? 'material' : (results.find(file => file.kind !== 'uv')?.kind || 'uv');
    if (view === 'model') {
      pendingResultPreview = '';
      await applyMapPreview(defaultPreview, false, false);
    } else {
      // 不改变用户所在的 UV/结果页；切回模型时再解码贴图，避免隐藏视口一次性
      // 占用大量内存，也确保模型不会继续显示上一次烘焙或无贴图材质。
      pendingResultPreview = defaultPreview;
    }
    status(`${data.cancelled ? '已取消' : data.failures?.length ? '部分完成' : '烘焙完成'} · ${results.length} 张贴图${view === 'model' ? ' · 已更新模型预览' : ' · 当前视图保持不变，切回模型后显示新结果'}${data.elapsedMs != null ? ` · ${(data.elapsedMs / 1000).toFixed(1)} 秒` : ''}${data.warnings?.length ? `。${data.warnings.join('；')}` : ''}${data.failures?.length ? `。${data.failures.join('；')}` : ''}`);
  }

  $('run').onclick = async () => {
    if (blocker()) return;
    job = crypto.randomUUID();
    running = true; cancelling = false; lastBakeProgress = 0; $('progress').value = 0;
    refresh();
    updateBakeProgress({ phase: '准备烘焙任务', stage: 'prepare', progress: 0, materialTotal: materials.size, mapTotal: meshMapKeys.filter(key => stored[key]).length });
    try {
      await withLog('model-bake-log', $('run'), async () => {
        const types = meshMapKeys.filter(key => stored[key]);
        const { workspace, uvMode, deviceLuid, ...options } = stored;
        const data = await invoke('bake_start', {
          handle: model.handle,
          jobId: job,
          options: { ...options, device: +stored.device, objects: model.objects.map(item => item.id), materials: [...materials], channels: { ...channels }, distance: +$('distance').value },
        });
        if (data.files?.length) await showResults(data);
        else {
          if (data.resultHandle) invoke('bake_result_release', { resultHandle: data.resultHandle }).catch(() => {});
          status(`${data.cancelled ? '烘焙已取消' : '烘焙失败'}，上次结果仍可使用。${data.failures?.length ? ` ${data.failures.join('；')}` : ''}`);
        }
        return {
          completed: data.files?.length || 0,
          total: materials.size * types.length,
          cancelled: Boolean(data.cancelled),
          logs: [...(data.files || []).map(file => file.path), ...(data.artifacts || []).map(file => file.path), ...(data.warnings || []), ...(data.failures || []), ...(data.cancelled ? ['任务已取消，已完成文件保留。'] : [])],
        };
      }, '模型烘焙');
    } finally {
      running = false; cancelling = false;
      job = '';
      // dispose 发生在烘焙中时不能立即删除 worker 正在读取的模型缓存；任务
      // 收尾后补释放，避免切换功能/HMR 后模型 TempDir 一直留到进程退出。
      if (disposed && model) {
        const released = model;
        model = null;
        invoke('bake_release', { handle: released.handle }).catch(() => {});
      }
      refresh();
    }
  };

  $('cancel').onclick = async () => {
    if (!running || cancelling) return;
    cancelling = true; setActionBusy($('cancel'), true); refresh();
    try {
      await invoke('bake_cancel', { jobId: job });
      status('正在取消，保留已完整写入的结果…');
      $('live-phase').textContent = '正在安全取消，已完成文件会保留';
    } catch (error) {
      cancelling = false; notify(error); refresh();
    } finally {
      setActionBusy($('cancel'), false); refresh();
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
  $('map-preview').value = stored.workspace.mapPreview;
  $('map-preview').onchange = () => applyMapPreview($('map-preview').value).catch(error => notify(`贴图预览加载失败：${error}`));
  $('wireframe').onclick = () => {
    stored.workspace.wireframe = !stored.workspace.wireframe;
    syncDisplayControls();
    persist();
  };
  root.querySelectorAll('[data-bake-panel-toggle]').forEach(button => {
    button.onclick = () => {
      const panel = button.dataset.bakePanelToggle;
      setPanelOpen(panel, $(`${panel}-panel`).hidden);
    };
  });
  $('open-output').onclick = () => { if (outputDirectory) openPath(outputDirectory).catch(notify); };
  root.querySelectorAll('[data-bake-export]').forEach(button => { button.onclick = () => exportResults(button); });
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
      $('distance-note').textContent = `单位：${model.units === '模型单位' ? '相对单位' : model.units}；所选包围盒对角线的 ${(stored.distanceRatio * 100).toFixed(1)}%`;
      persist();
    }
    refresh();
  };
  {
    // UV 视图交互：滚轮以光标为锚缩放，拖拽平移，缩到 1 即复位
    const canvas = $('uv-canvas');
    const clampPan = () => {
      if (uvZoom <= 1) { uvZoom = 1; uvPanX = 0; uvPanY = 0; }
    };
    canvas.addEventListener('wheel', event => {
      if (view !== 'uv') return;
      event.preventDefault();
      const rect = canvas.getBoundingClientRect();
      const cx = event.clientX - rect.left;
      const cy = event.clientY - rect.top;
      const factor = event.deltaY < 0 ? 1.2 : 1 / 1.2;
      const next = Math.min(16, Math.max(1, uvZoom * factor));
      // 保持光标下的内容点不动：锚点含 12px 基准偏移，pan' = (c-12) - (c-12-pan)·r
      uvPanX = cx - 12 - (cx - 12 - uvPanX) * (next / uvZoom);
      uvPanY = cy - 12 - (cy - 12 - uvPanY) * (next / uvZoom);
      uvZoom = next;
      clampPan();
      drawUv();
    }, { passive: false });
    let panning = null;
    endUvPan = () => { panning = null; };
    canvas.addEventListener('pointerdown', event => {
      if (view !== 'uv' || event.button !== 0) return;
      panning = { x: event.clientX, y: event.clientY, panX: uvPanX, panY: uvPanY };
      canvas.setPointerCapture(event.pointerId);
    });
    canvas.addEventListener('pointermove', event => {
      if (!panning) return;
      uvPanX = panning.panX + event.clientX - panning.x;
      uvPanY = panning.panY + event.clientY - panning.y;
      clampPan();
      drawUv();
    });
    canvas.addEventListener('pointerup', endUvPan);
    canvas.addEventListener('pointercancel', endUvPan);
    canvas.addEventListener('lostpointercapture', endUvPan);
    canvas.addEventListener('dblclick', () => {
      uvZoom = 1; uvPanX = 0; uvPanY = 0;
      drawUv();
    });
  }
  for (const key of Object.keys(defaults)) {
    const element = $(key);
    if (!element || key === 'workspace') continue;
    element.addEventListener('change', () => {
      stored[key] = element.type === 'checkbox' ? element.checked : key === 'selfOnly' ? element.value === 'true' : key === 'uvMode' ? element.value : +element.value;
      if (key === 'uvMode' && model) status('UV 处理方式将在下次导入或重新导入模型时生效。');
      if (key === 'device') stored.deviceLuid = devices.find(item => item.index === +stored.device)?.luid || '';
      if (key === 'device' && !devices.find(item => item.index === +stored.device)?.supported) {
        stored.ao = false;
        stored.thickness = false;
        $('ao').checked = false;
        $('thickness').checked = false;
      }
      persist();
      refresh();
    });
  }
  root.querySelectorAll('[data-bake-view]').forEach(button => { button.onclick = () => setView(button.dataset.bakeView); });
  if (desktop) {
    listen('bake-progress', event => {
      if (running && event.payload.jobId !== job) return;
      if (!running && !loading) return;
      updateBakeProgress(event.payload.data);
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
  const resetAltRotate = () => {
    if (controls) controls.mouseButtons.LEFT = null;
  };
  const keyboard = event => {
    if (!active) return;
    // Alt：按住时左键临时映射为旋转（Marmoset/Substance 习惯），松开归还点选。
    // keyup 无条件恢复：焦点已移进输入框（或 Alt+Tab 切窗收不到 keyup，由
    // window blur 兜底）时若沿用 target 守卫，LEFT=ROTATE 会永久粘滞。
    if (event.key === 'Alt') {
      if (event.type === 'keyup') {
        resetAltRotate();
      } else if (controls && view === 'model' && !event.target.closest('input, select, textarea')) {
        controls.mouseButtons.LEFT = THREE.MOUSE.ROTATE;
        event.preventDefault();
      }
      return;
    }
    // 按钮不排除：点击材质名后焦点留在 <button> 上，F/1/3/7 仍需可用；
    // 这些键与按钮的 Enter/Space 激活键不冲突。
    if (event.ctrlKey || event.metaKey || event.altKey || event.target.closest('input, select, textarea, [role="combobox"], dialog')) return;
    if (event.key.toLowerCase() === 'f' && model && view === 'model') { event.preventDefault(); frameSelection(); }
    const view_ = standardViews[event.key];
    if (view_ && model && view === 'model') { event.preventDefault(); setStandardView(view_); }
  };
  document.addEventListener('keydown', keyboard);
  document.addEventListener('keyup', keyboard);
  window.addEventListener('blur', resetAltRotate);
  window.addEventListener('blur', endUvPan);
  const endCheckboxDrag = () => { checkboxDrag = null; };
  window.addEventListener('pointerup', endCheckboxDrag);
  window.addEventListener('pointercancel', endCheckboxDrag);
  updatePanelState('outliner');
  updatePanelState('settings');
  syncDisplayControls();
  setView('model');
  refreshOidnStatus();
  // 下载按钮的点击处理器只在构造时绑定一次；漏掉这一步按钮就是死的。
  bindOidnDownload();

  // OIDN 状态查询的单一入口：内置组件存在时一次即成功。查询失败保持"未决"
  //（不显示下载按钮、不判未下载），refresh() 每次自动重试自愈——一次 IPC
  // 抖动不该把用户卡在"请手动下载"上。
  let oidnStatusInFlight = false;
  async function refreshOidnStatus() {
    if (!desktop || oidnReady || oidnStatusResolved || oidnStatusInFlight) return;
    oidnStatusInFlight = true;
    try {
      const value = await invoke('oidn_status');
      oidnReady = Boolean(value.installed);
      oidnSource = value.source || '';
      oidnStatusResolved = true;
      refresh();
    } catch (error) {
      console.warn('OIDN 状态查询失败，将在下次刷新时重试', error);
    } finally {
      oidnStatusInFlight = false;
    }
  }
  function bindOidnDownload() {
    const button = $('oidn-download');
    if (!button || button.dataset.bound === 'true') return;
    button.dataset.bound = 'true';
    button.onclick = async () => {
      if (button.disabled) return;
      button.disabled = true; button.textContent = '正在下载…';
      try {
        const value = await invoke('oidn_install');
        oidnReady = true; oidnSource = value.source || 'downloaded';
        status('AI 降噪组件已就绪。');
        notify('AI 降噪组件已下载');
      } catch (error) {
        status(`降噪组件下载失败：${error}`);
        notify(`降噪组件下载失败：${error}`);
      } finally {
        button.disabled = false; button.textContent = '下载降噪组件';
        refresh();
      }
    };
  }

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
        refreshOidnStatus();
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
      window.removeEventListener('blur', resetAltRotate);
      window.removeEventListener('blur', endUvPan);
      window.removeEventListener('pointerup', endCheckboxDrag);
      window.removeEventListener('pointercancel', endCheckboxDrag);
      clearResultTextures();
      unlisten?.();
      controls?.dispose();
      clearMeshes();
      renderer?.dispose();
      if (model && !running) invoke('bake_release', { handle: model.handle }).catch(() => {});
      if (resultHandle) invoke('bake_result_release', { resultHandle }).catch(() => {});
      previewWorker?.terminate();
      previewPending.forEach(({ reject }) => reject(Error('预览已关闭'))); previewPending.clear();
    },
  };
}
