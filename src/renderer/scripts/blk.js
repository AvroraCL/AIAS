import { blkFileGroup, bulkFromFill, createBlkRules, defaultBlkName, diffBlk, highlightBlk, renderBlk, validateBlk } from './blk-core.mjs';
import { createBlkThumbnails } from './blk-thumbnails.js';

// 行内编辑的紧凑行视图：每条映射一行（启用开关 · 缩略图 · 目标 DDS · 原名输入 ·
// 指令下拉 · camo 参数 · 删除），右侧栏不再承载编辑。原名输入支持 ↑↓/Enter
// 在行间移动，批量填充覆盖最常见的"整包重命名"场景。
const groups = [
  ['c', 'C 文件 · 颜色', '_c.dds · 可统一设置指令与原名'],
  ['n', 'N 文件 · 法线', '_n.dds · 固定 replace_tex'],
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
  let active = true;
  const thumbObserver = new IntersectionObserver(entries => {
    for (const entry of entries) {
      if (!entry.isIntersecting) continue;
      thumbObserver.unobserve(entry.target);
      loadThumb(entry.target, entry.target.dataset.dds);
    }
  }, { rootMargin: '160px' });

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

  function updateCount() {
    $('blk-count').textContent = `${files.length} 张 DDS · ${rules.filter(rule => rule.enabled).length} 条启用`;
  }

  function addMapping(to) {
    const index = rules.findLastIndex(rule => rule.to.toLowerCase() === to.toLowerCase());
    const original = rules[index];
    const rule = { id: ++nextRuleId, to, from: '', enabled: true, command: original.command, camoSkinTex: false, extra: true };
    rules.splice(index + 1, 0, rule);
    renderRows();
    rows.querySelector(`.blk-row[data-rule-id="${rule.id}"] .blk-from-input`)?.focus();
  }

  function focusFromInput(current, offset) {
    const inputs = [...rows.querySelectorAll('.blk-from-input')];
    const index = inputs.indexOf(current);
    const next = inputs[index + offset];
    if (next) { next.focus(); next.select(); }
  }

  /// 一条映射一行。控件就地更新（不整表重渲染），保证输入焦点不丢。
  function buildRow(rule) {
    const row = document.createElement('div');
    row.className = 'blk-row';
    row.dataset.ruleId = String(rule.id);

    const enabled = document.createElement('input');
    enabled.type = 'checkbox';
    enabled.className = 'blk-enabled';
    enabled.checked = rule.enabled;
    enabled.setAttribute('aria-label', `启用 ${rule.to} 映射`);
    enabled.addEventListener('change', () => {
      rule.enabled = enabled.checked;
      row.classList.toggle('is-disabled', !rule.enabled);
      updateCount();
      updatePreview();
    });

    const thumb = document.createElement('button');
    thumb.type = 'button';
    thumb.className = 'blk-thumb';
    thumb.setAttribute('aria-label', `放大查看 ${rule.to}`);
    thumb.addEventListener('click', () => openLargePreview(rule.to));
    watchThumb(thumb, rule.to);

    const toCell = document.createElement('span');
    toCell.className = 'blk-row-target';
    toCell.title = rule.to;
    if (rule.extra) {
      const badge = document.createElement('i');
      badge.className = 'blk-extra-badge';
      badge.textContent = '额外';
      toCell.append(badge);
    }
    const toName = document.createElement('span');
    toName.className = 'blk-target-name';
    toName.textContent = rule.to;
    toCell.append(toName);

    const from = document.createElement('input');
    from.type = 'text';
    from.className = 'blk-from-input';
    from.value = rule.from;
    from.placeholder = '游戏原贴图名';
    from.setAttribute('aria-label', `${rule.to} 的游戏原贴图名`);
    from.spellcheck = false;
    from.addEventListener('input', () => {
      rule.from = from.value;
      updatePreview();
    });
    from.addEventListener('focus', () => { rows.querySelectorAll('.blk-row.selected').forEach(el => el.classList.remove('selected')); row.classList.add('selected'); });
    from.addEventListener('keydown', event => {
      if (event.key === 'ArrowDown' || event.key === 'Enter') { event.preventDefault(); focusFromInput(from, 1); }
      else if (event.key === 'ArrowUp') { event.preventDefault(); focusFromInput(from, -1); }
    });

    const normal = blkFileGroup(rule.to) === 'n';
    let command;
    if (normal) {
      command = document.createElement('span');
      command.className = 'blk-fixed-inline';
      command.textContent = 'replace_tex';
    } else {
      command = document.createElement('select');
      command.className = 'blk-cmd-select';
      command.setAttribute('aria-label', `${rule.to} 的规则指令`);
      command.append(...[['', '请选择指令'], ['replace_tex', 'replace_tex'], ['set_tex', 'set_tex']].map(([value, caption]) => {
        const option = document.createElement('option');
        option.value = value;
        option.textContent = caption;
        return option;
      }));
      command.value = rule.command;
      command.addEventListener('change', () => {
        rule.command = command.value;
        param.hidden = rule.command !== 'set_tex';
        updatePreview();
      });
    }

    const param = document.createElement('label');
    param.className = 'blk-param-inline';
    param.hidden = rule.command !== 'set_tex';
    param.title = 'set_tex 固定迷彩可能需要 camo_skin_tex 参数';
    const paramInput = document.createElement('input');
    paramInput.type = 'checkbox';
    paramInput.checked = rule.camoSkinTex;
    paramInput.setAttribute('aria-label', `${rule.to} 添加 camo_skin_tex 参数`);
    paramInput.addEventListener('change', () => {
      rule.camoSkinTex = paramInput.checked;
      updatePreview();
    });
    const paramText = document.createElement('span');
    paramText.textContent = 'camo';
    param.append(paramInput, paramText);

    row.append(enabled, thumb, toCell, from, command, param);
    if (rule.extra) {
      const remove = document.createElement('button');
      remove.type = 'button';
      remove.className = 'blk-remove-extra';
      remove.textContent = '×';
      remove.setAttribute('aria-label', `删除 ${rule.to} 的这条额外映射`);
      remove.title = '删除额外映射';
      remove.addEventListener('click', () => {
        rules.splice(rules.indexOf(rule), 1);
        renderRows();
      });
      row.append(remove);
    }
    if (!rule.enabled) row.classList.add('is-disabled');
    return row;
  }

  function renderRows() {
    rows.replaceChildren();
    thumbObserver.disconnect();
    $('blk-empty').hidden = files.length > 0;
    updateCount();

    const toolbar = document.createElement('div');
    toolbar.className = 'blk-toolbar';
    const toolbarLabel = document.createElement('span');
    toolbarLabel.className = 'blk-toolbar-label';
    toolbarLabel.textContent = '原名批量：';
    toolbar.append(toolbarLabel);
    for (const [mode, caption] of [['name', '文件名 + *'], ['stem', '去 _c/_n + *'], ['clear', '清空']]) {
      const button = document.createElement('button');
      button.type = 'button';
      button.className = 'secondary-action';
      button.textContent = caption;
      button.title = '对所有映射重新填充游戏原贴图名（可再逐条修改）';
      button.addEventListener('click', () => {
        rules = bulkFromFill(rules, mode);
        renderRows();
      });
      toolbar.append(button);
    }
    rows.append(toolbar);

    for (const [group, title, description] of groups) {
      const members = rules.filter(rule => blkFileGroup(rule.to) === group);
      if (!members.length) continue;
      const section = document.createElement('section');
      section.className = 'blk-rule-group';
      const heading = document.createElement('div');
      heading.className = 'blk-group-heading';
      const copy = document.createElement('div');
      const name = document.createElement('strong');
      name.textContent = `${title} · ${members.length} 条`;
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
        const sameTarget = members.filter(item => item.to.toLowerCase() === to.toLowerCase());
        for (const rule of sameTarget) list.append(buildRow(rule));
        const add = document.createElement('button');
        add.type = 'button';
        add.className = 'blk-add-row';
        add.textContent = `+ ${to} 的额外映射`;
        add.setAttribute('aria-label', `为 ${to} 添加额外映射`);
        add.title = '同一目标 DDS 映射多个游戏原贴图名';
        add.addEventListener('click', () => addMapping(to));
        list.append(add);
      }
      section.append(heading, list);
      rows.append(section);
    }
    updatePreview();
  }

  function blocker() {
    if (!directory.value) return '请选择 DDS 文件夹。';
    return validateBlk(fileName.value, files, rules);
  }

  function updatePreview() {
    const error = blocker();
    preview.innerHTML = error ? '' : highlightBlk(renderBlk(rules));
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
