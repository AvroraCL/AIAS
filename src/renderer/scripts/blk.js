import { blkFileGroup, createBlkRules, defaultBlkName, diffBlk, renderBlk, validateBlk } from './blk-core.mjs';
import { createBlkThumbnails } from './blk-thumbnails.js';

const svgNS = 'http://www.w3.org/2000/svg';
const groups = [
  ['c', 'C 文件', '_c.dds · 可统一设置，也可单独修改'],
  ['n', 'N 文件', '_n.dds · 固定 replace_tex'],
  ['other', '其他 DDS', '逐条选择规则指令']
];

export function createBlk({ desktop, scan, exportFile, pickDirectory, saveDirectory, confirm, notify, changed, thumbnail, convertFileSrc }) {
  const $ = id => document.getElementById(id);
  const directory = $('blk-directory');
  const fileName = $('blk-file-name');
  const browserInput = $('blk-browser-directory');
  const rows = $('blk-rules');
  const preview = $('blk-preview');
  const thumbs = createBlkThumbnails({ desktop, thumbnail, convertFileSrc });
  if (!desktop) $('blk-output-hint').textContent = '浏览器预览会下载 BLK；桌面版直接写入 DDS 目录。';

  let rules = [];
  let files = [];
  let browserFiles = [];
  let existingContent = null;
  let scanVersion = 0;
  let thumbnailVersion = 0;
  let nextRuleId = 0;
  let selectedId = null;
  let active = true;
  let drawQueued = false;
  let links = [];
  const resizeObserver = new ResizeObserver(queueDraw);
  const thumbObserver = new IntersectionObserver(entries => {
    for (const entry of entries) {
      if (!entry.isIntersecting) continue;
      thumbObserver.unobserve(entry.target);
      loadThumb(entry.target, entry.target.dataset.dds);
    }
  }, { rootMargin: '160px' });

  function selectedRule() { return rules.find(rule => rule.id === selectedId) || null; }
  function browserDds(name) {
    return browserFiles.find(file => file.webkitRelativePath.split('/').length === 2 && file.name.toLowerCase() === name.toLowerCase());
  }

  async function inspect() {
    if (desktop) return scan(directory.value, fileName.value);
    const direct = browserFiles.filter(file => file.webkitRelativePath.split('/').length === 2);
    const match = direct.find(file => file.name.toLowerCase() === `${fileName.value.replace(/\.blk$/i, '')}.blk`.toLowerCase());
    return { files: direct.map(file => file.name).filter(name => /\.dds$/i.test(name)), existingContent: match ? await match.text() : null };
  }

  function resetThumbs() {
    thumbnailVersion++;
    thumbObserver.disconnect();
    thumbs.reset();
    delete $('blk-selected-thumb').dataset.dds;
  }

  async function loadThumb(node, name) {
    if (!node || !name || !active) return;
    const version = thumbnailVersion;
    const result = await thumbs.get(name, directory.value, browserDds(name));
    if (version !== thumbnailVersion || !node.isConnected || node.dataset.dds !== name || !active) return;
    const image = node.querySelector('img');
    const status = node.querySelector('span');
    if (result.url) {
      image.src = result.url;
      image.hidden = false;
      status.hidden = true;
      node.classList.remove('unavailable');
      node.title = `${name} · 点击放大`;
    } else {
      image.hidden = true;
      status.hidden = false;
      status.textContent = result.error || '无法预览 DDS';
      node.title = `${name} · ${status.textContent}`;
      node.classList.add('unavailable');
    }
  }

  function watchThumb(node, name, eager = false) {
    node.dataset.dds = name;
    node.classList.remove('unavailable');
    const image = document.createElement('img');
    image.alt = '';
    image.hidden = true;
    const status = document.createElement('span');
    status.textContent = '加载 DDS 缩略图';
    node.replaceChildren(image, status);
    if (eager) loadThumb(node, name);
    else thumbObserver.observe(node);
  }

  function openLargePreview(name) {
    document.querySelector('.blk-image-modal')?.remove();
    const backdrop = document.createElement('div');
    backdrop.className = 'blk-image-modal';
    const panel = document.createElement('div');
    panel.className = 'blk-image-panel';
    panel.setAttribute('role', 'dialog');
    panel.setAttribute('aria-modal', 'true');
    panel.setAttribute('aria-label', `${name} DDS 预览`);
    const heading = document.createElement('div');
    heading.className = 'blk-image-heading';
    const title = document.createElement('strong');
    title.textContent = name;
    const close = document.createElement('button');
    close.type = 'button';
    close.textContent = '关闭';
    close.addEventListener('click', () => backdrop.remove());
    heading.append(title, close);
    const image = document.createElement('button');
    image.type = 'button';
    image.className = 'blk-image-large';
    image.setAttribute('aria-label', `${name} 的 DDS 图像`);
    watchThumb(image, name, true);
    panel.append(heading, image);
    backdrop.append(panel);
    backdrop.addEventListener('click', event => { if (event.target === backdrop) backdrop.remove(); });
    backdrop.addEventListener('keydown', event => { if (event.key === 'Escape') backdrop.remove(); });
    document.body.append(backdrop);
    close.focus();
  }

  function queueDraw() {
    if (drawQueued) return;
    drawQueued = true;
    requestAnimationFrame(() => { drawQueued = false; drawConnections(); });
  }

  function drawConnections() {
    for (const { bundle, source, target, path, hit, label } of links) {
      if (!bundle.isConnected) continue;
      const base = bundle.getBoundingClientRect();
      const from = source.getBoundingClientRect();
      const to = target.getBoundingClientRect();
      const horizontal = to.left - from.right >= 20;
      const x1 = (horizontal ? from.right : from.left + from.width / 2) - base.left;
      const y1 = (horizontal ? from.top + from.height / 2 : from.bottom) - base.top;
      const x2 = (horizontal ? to.left : to.left + to.width / 2) - base.left;
      const y2 = (horizontal ? to.top + to.height / 2 : to.top) - base.top;
      const d = horizontal
        ? `M ${x1} ${y1} C ${x1 + (x2 - x1) * .48} ${y1}, ${x1 + (x2 - x1) * .52} ${y2}, ${x2} ${y2}`
        : `M ${x1} ${y1} C ${x1} ${y1 + (y2 - y1) * .48}, ${x2} ${y1 + (y2 - y1) * .52}, ${x2} ${y2}`;
      path.setAttribute('d', d);
      hit.setAttribute('d', d);
      label.setAttribute('x', String((x1 + x2) / 2));
      label.setAttribute('y', String((y1 + y2) / 2 - (horizontal ? 9 : 0)));
    }
  }

  function updateSelection() {
    for (const { source, link, rule } of links) {
      const selected = rule.id === selectedId;
      source.classList.toggle('selected', selected);
      link.classList.toggle('selected', selected);
      link.classList.toggle('disabled', !rule.enabled);
    }
    updateInspector();
  }

  function selectRule(rule) {
    selectedId = rule.id;
    updateSelection();
  }

  function updateInspector() {
    const rule = selectedRule();
    $('blk-selection-empty').hidden = Boolean(rule);
    $('blk-selection-editor').hidden = !rule;
    if (!rule) return;
    $('blk-selected-to').textContent = rule.to;
    if (document.activeElement !== $('blk-selected-from')) $('blk-selected-from').value = rule.from;
    const commandSelect = $('blk-selected-command');
    commandSelect.value = rule.command;
    const normal = blkFileGroup(rule.to) === 'n';
    commandSelect.hidden = normal;
    const commandWrapper = commandSelect.closest('.custom-select');
    if (commandWrapper) {
      commandWrapper.hidden = normal;
      commandWrapper.querySelector('.custom-select-button').textContent = commandSelect.selectedOptions[0]?.textContent || '请选择指令';
    }
    $('blk-selected-fixed').hidden = !normal;
    $('blk-selected-param').hidden = rule.command !== 'set_tex';
    $('blk-selected-param-input').checked = rule.camoSkinTex;
    $('blk-selected-enabled').checked = rule.enabled;
    $('blk-selected-delete').hidden = !rule.extra;
    const selectedThumb = $('blk-selected-thumb');
    if (selectedThumb.dataset.dds !== rule.to) watchThumb(selectedThumb, rule.to, true);
  }

  function addMapping(to) {
    const index = rules.findLastIndex(rule => rule.to.toLowerCase() === to.toLowerCase());
    const original = rules[index];
    const rule = { id: ++nextRuleId, to, from: '', enabled: true, command: original.command, camoSkinTex: false, extra: true };
    rules.splice(index + 1, 0, rule);
    selectedId = rule.id;
    renderRows();
    $('blk-selected-from').focus();
  }

  function renderRows() {
    rows.replaceChildren();
    links = [];
    thumbObserver.disconnect();
    resizeObserver.disconnect();
    $('blk-empty').hidden = files.length > 0;
    $('blk-count').textContent = `${files.length} 张 DDS · ${rules.filter(rule => rule.enabled).length} 条启用`;
    if (!selectedRule()) selectedId = rules[0]?.id ?? null;

    for (const [group, title, description] of groups) {
      const members = rules.filter(rule => blkFileGroup(rule.to) === group);
      if (!members.length) continue;
      const section = document.createElement('section');
      section.className = 'blk-rule-group';
      const heading = document.createElement('div');
      heading.className = 'blk-group-heading';
      const copy = document.createElement('div');
      const name = document.createElement('strong');
      name.textContent = `${title} · ${members.length} 条映射`;
      const hint = document.createElement('small');
      hint.textContent = description;
      copy.append(name, hint);
      heading.append(copy);
      if (group === 'c') {
        const bulk = document.createElement('div');
        bulk.className = 'blk-bulk-actions';
        for (const [value, caption] of [['replace_tex', '全部 replace_tex'], ['set_tex', '全部 set_tex']]) {
          const button = document.createElement('button');
          button.type = 'button';
          button.className = 'secondary-action';
          button.textContent = caption;
          button.setAttribute('aria-pressed', String(members.every(rule => rule.command === value)));
          button.addEventListener('click', () => { members.forEach(rule => { rule.command = value; }); renderRows(); });
          bulk.append(button);
        }
        heading.append(bulk);
      }
      const list = document.createElement('div');
      list.className = 'blk-group-list';
      const targets = [...new Map(members.map(rule => [rule.to.toLowerCase(), rule.to])).values()];
      for (const to of targets) {
        const bundle = document.createElement('article');
        bundle.className = 'blk-bundle';
        const sources = document.createElement('div');
        sources.className = 'blk-source-list';
        const svg = document.createElementNS(svgNS, 'svg');
        svg.classList.add('blk-link-layer');
        svg.setAttribute('aria-label', `${to} 的映射连线`);
        const targetColumn = document.createElement('div');
        targetColumn.className = 'blk-target-column';
        const thumb = document.createElement('button');
        thumb.type = 'button';
        thumb.className = 'blk-thumb';
        thumb.setAttribute('aria-label', `放大查看 ${to}`);
        thumb.addEventListener('click', () => openLargePreview(to));
        watchThumb(thumb, to);
        const fileLabel = document.createElement('strong');
        fileLabel.className = 'blk-target-name';
        fileLabel.textContent = to;
        fileLabel.title = to;
        const add = document.createElement('button');
        add.type = 'button';
        add.className = 'secondary-action blk-add-mapping';
        add.textContent = '+ 添加映射';
        add.setAttribute('aria-label', `为 ${to} 添加映射`);
        add.addEventListener('click', () => addMapping(to));
        targetColumn.append(thumb, fileLabel, add);
        for (const rule of members.filter(item => item.to.toLowerCase() === to.toLowerCase())) {
          const source = document.createElement('button');
          source.type = 'button';
          source.className = 'blk-source-node';
          source.dataset.ruleId = String(rule.id);
          source.setAttribute('aria-label', `编辑 ${rule.from || '未填写来源'} 到 ${to} 的映射`);
          const label = document.createElement('span');
          label.className = 'blk-source-name';
          label.textContent = rule.from || '填写游戏原贴图名';
          const detail = document.createElement('small');
          detail.textContent = rule.enabled ? '游戏原贴图' : '已排除';
          source.append(label, detail);
          source.addEventListener('click', () => selectRule(rule));
          sources.append(source);

          const link = document.createElementNS(svgNS, 'g');
          link.classList.add('blk-link');
          const path = document.createElementNS(svgNS, 'path');
          path.classList.add('blk-link-visible');
          const hit = document.createElementNS(svgNS, 'path');
          hit.classList.add('blk-link-hit');
          hit.setAttribute('role', 'button');
          hit.setAttribute('tabindex', '0');
          hit.setAttribute('aria-label', `编辑 ${rule.from || '未填写来源'} 到 ${to} 的 ${rule.command || '未选指令'} 连线`);
          hit.addEventListener('click', () => selectRule(rule));
          hit.addEventListener('keydown', event => { if (event.key === 'Enter' || event.key === ' ') { event.preventDefault(); selectRule(rule); } });
          const lineLabel = document.createElementNS(svgNS, 'text');
          lineLabel.classList.add('blk-link-label');
          lineLabel.textContent = rule.command || '选指令';
          link.append(path, hit, lineLabel);
          svg.append(link);
          links.push({ bundle, source, target: thumb, path, hit, label: lineLabel, link, rule });
        }
        bundle.append(sources, svg, targetColumn);
        list.append(bundle);
        resizeObserver.observe(bundle);
      }
      section.append(heading, list);
      rows.append(section);
    }
    updateSelection();
    updatePreview();
    queueDraw();
  }

  function blocker() {
    if (!directory.value) return '请选择 DDS 文件夹。';
    return validateBlk(fileName.value, files, rules);
  }

  function updatePreview() {
    const error = blocker();
    preview.textContent = error ? '' : renderBlk(rules);
    $('blk-preview-status').textContent = error || `${fileName.value.replace(/\.blk$/i, '')}.blk · ${rules.filter(rule => rule.enabled).length} 条规则`;
    $('blk-preview-status').classList.toggle('invalid', Boolean(error));
    changed();
  }

  async function rescan({ preserve = true, refreshThumbnails = true } = {}) {
    if (!directory.value) return;
    const version = ++scanVersion;
    if (refreshThumbnails) resetThumbs();
    const result = await inspect();
    if (version !== scanVersion) return;
    const previous = new Map();
    if (preserve) for (const rule of rules) {
      const key = rule.to.toLowerCase();
      if (!previous.has(key)) previous.set(key, []);
      previous.get(key).push(rule);
    }
    files = result.files;
    rules = createBlkRules(files).flatMap(rule => {
      const restored = previous.get(rule.to.toLowerCase());
      if (!restored) return [{ ...rule, id: ++nextRuleId }];
      return restored.map(item => ({ ...item, to: rule.to, command: blkFileGroup(rule.to) === 'n' ? 'replace_tex' : item.command }));
    });
    existingContent = result.existingContent;
    renderRows();
  }

  async function chooseDirectory() {
    if (!desktop) { browserInput.click(); return; }
    const selected = await pickDirectory();
    if (!selected) return;
    directory.value = selected;
    fileName.value = defaultBlkName(selected);
    selectedId = null;
    await rescan({ preserve: false });
    await saveDirectory();
  }

  $('blk-pick-directory').addEventListener('click', () => chooseDirectory().catch(error => notify(error.message || String(error), 'error')));
  $('blk-rescan').addEventListener('click', () => rescan().catch(error => notify(error.message || String(error), 'error')));
  browserInput.addEventListener('change', () => {
    browserFiles = [...browserInput.files];
    if (!browserFiles.length) return;
    directory.value = browserFiles[0].webkitRelativePath.split('/')[0];
    fileName.value = defaultBlkName(directory.value);
    selectedId = null;
    rescan({ preserve: false }).catch(error => notify(error.message || String(error), 'error'));
  });
  let nameTimer;
  fileName.addEventListener('input', () => {
    ++scanVersion;
    existingContent = null;
    updatePreview();
    clearTimeout(nameTimer);
    nameTimer = setTimeout(() => rescan({ refreshThumbnails: false }).catch(error => notify(error.message || String(error), 'error')), 250);
  });

  $('blk-selected-from').addEventListener('input', event => {
    const rule = selectedRule();
    if (!rule) return;
    rule.from = event.target.value;
    const source = rows.querySelector(`.blk-source-node[data-rule-id="${rule.id}"]`);
    if (source) {
      source.querySelector('.blk-source-name').textContent = rule.from || '填写游戏原贴图名';
      source.setAttribute('aria-label', `编辑 ${rule.from || '未填写来源'} 到 ${rule.to} 的映射`);
    }
    const link = links.find(item => item.rule.id === rule.id);
    link?.hit.setAttribute('aria-label', `编辑 ${rule.from || '未填写来源'} 到 ${rule.to} 的 ${rule.command || '未选指令'} 连线`);
    updatePreview();
    queueDraw();
  });
  $('blk-selected-command').addEventListener('change', event => {
    const rule = selectedRule();
    if (!rule || blkFileGroup(rule.to) === 'n') return;
    rule.command = event.target.value;
    renderRows();
  });
  $('blk-selected-param-input').addEventListener('change', event => {
    const rule = selectedRule();
    if (!rule) return;
    rule.camoSkinTex = event.target.checked;
    updatePreview();
  });
  $('blk-selected-enabled').addEventListener('change', event => {
    const rule = selectedRule();
    if (!rule) return;
    rule.enabled = event.target.checked;
    renderRows();
  });
  $('blk-selected-delete').addEventListener('click', () => {
    const rule = selectedRule();
    if (!rule?.extra) return;
    rules.splice(rules.indexOf(rule), 1);
    selectedId = rules.find(item => item.to.toLowerCase() === rule.to.toLowerCase())?.id ?? rules[0]?.id ?? null;
    renderRows();
  });
  $('blk-selected-thumb').addEventListener('click', () => { const rule = selectedRule(); if (rule) openLargePreview(rule.to); });

  async function generate() {
    const error = blocker();
    if (error) throw new Error(error);
    const content = renderBlk(rules);
    const latest = await inspect();
    if (latest.existingContent !== existingContent) {
      existingContent = latest.existingContent;
      throw new Error('已有 BLK 在预览后发生变化，请检查后重新生成。');
    }
    const current = new Set(latest.files.map(name => name.toLowerCase()));
    const reviewed = new Set(rules.map(rule => rule.to.toLowerCase()));
    if (reviewed.size !== current.size || rules.some(rule => !current.has(rule.to.toLowerCase()))) throw new Error('DDS 文件列表发生变化，请重新扫描。');
    if (latest.existingContent !== null) {
      const diff = diffBlk(latest.existingContent, content);
      const accepted = await confirm('确认覆盖已有 BLK', `目标：${fileName.value.replace(/\.blk$/i, '')}.blk\n\n变更预览：\n${diff}`, { confirmText: '确认覆盖', danger: true });
      if (!accepted) return null;
    }
    if (desktop) {
      const path = await exportFile({ directory: directory.value, fileName: fileName.value, rules, expectedExisting: latest.existingContent, previewContent: content });
      existingContent = content;
      return path;
    }
    const blob = new Blob([content], { type: 'text/plain;charset=utf-8' });
    const url = URL.createObjectURL(blob);
    const link = document.createElement('a');
    link.href = url;
    link.download = `${fileName.value.replace(/\.blk$/i, '')}.blk`;
    document.body.append(link);
    link.click();
    link.remove();
    setTimeout(() => URL.revokeObjectURL(url), 30000);
    return link.download;
  }

  function activate(mode) {
    if (mode === 'blk') {
      active = true;
      if (files.length) renderRows();
    } else if (active) {
      active = false;
      resetThumbs();
      document.querySelector('.blk-image-modal')?.remove();
    }
  }

  updatePreview();
  return { blocker, generate, rescan, activate };
}
