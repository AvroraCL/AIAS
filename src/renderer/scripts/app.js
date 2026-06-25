const state = {
  settings: {},
  splitFiles: [],
  imageFiles: [],
  activeView: "merge"
};

const viewMeta = {
  merge: ["PBR 多通道合成", "扫描 Blender BSDF 命名贴图并输出游戏用 DDS 通道。"],
  split: ["PBR 多通道拆分", "把 _c.dds 与 _n.dds 拆回 BaseColor、Roughness、Metallic、Normal。"],
  mipmap: ["Mipmap 生成", "读取 p0、p1、p2... 图片序列并生成 DDS。"],
  "image-dds": ["图片转 DDS", "批量把 PNG、TGA、JPG 转换为 DDS。"],
  skins: ["涂装管理", "管理 War Thunder UserSkins 文件。"]
};

const settingBindings = {
  "pbr-input": "pbrInputPath",
  "pbr-output": "pbrOutputPath",
  "pbr-alpha": "pbrAlpha",
  "pbr-format": "pbrFormat",
  "split-output": "splitOutputPath",
  "split-format": "splitExportFormat",
  "split-alpha": "splitExportAlpha",
  "mipmap-input": "mipmapInputPath",
  "mipmap-output": "mipmapOutputPath",
  "mipmap-alpha": "mipmapAlpha",
  "mipmap-format": "mipmapFormat",
  "image-output": "imageToDdsOutputPath",
  "image-alpha": "imageToDdsAlpha",
  "image-format": "imageToDdsFormat",
  "skin-path": "skinManagerPath"
};

function $(id) {
  return document.getElementById(id);
}

function appendLog(id, message) {
  const target = $(id);
  target.textContent += `${message}\n`;
  target.scrollTop = target.scrollHeight;
}

function clearLog(id) {
  $(id).textContent = "";
}

function setBusy(button, busy) {
  button.disabled = busy;
  button.dataset.originalText ||= button.textContent;
  button.textContent = busy ? "处理中..." : button.dataset.originalText;
}

async function withLog(logId, button, action) {
  clearLog(logId);
  setBusy(button, true);
  try {
    const result = await action();
    for (const line of result.logs || []) appendLog(logId, line);
    appendLog(logId, `完成：${result.completed} / ${result.total}`);
  } catch (error) {
    appendLog(logId, `失败：${error.message || error}`);
  } finally {
    setBusy(button, false);
  }
}

function applySettingsToForm() {
  for (const [id, key] of Object.entries(settingBindings)) {
    const element = $(id);
    if (!element) continue;
    if (element.type === "checkbox") {
      element.checked = Boolean(state.settings[key]);
    } else {
      element.value = state.settings[key] || "";
    }
  }
}

function collectSettingsFromForm() {
  const patch = {};
  for (const [id, key] of Object.entries(settingBindings)) {
    const element = $(id);
    if (!element) continue;
    patch[key] = element.type === "checkbox" ? element.checked : element.value;
  }
  return patch;
}

async function saveSettings() {
  state.settings = await window.aias.settings.set(collectSettingsFromForm());
}

function bindNavigation() {
  document.querySelectorAll(".nav-item").forEach((button) => {
    button.addEventListener("click", () => {
      state.activeView = button.dataset.view;
      document.querySelectorAll(".nav-item").forEach((item) => item.classList.toggle("active", item === button));
      document.querySelectorAll(".view").forEach((view) => view.classList.toggle("active", view.id === `view-${state.activeView}`));
      const [title, subtitle] = viewMeta[state.activeView];
      $("view-title").textContent = title;
      $("view-subtitle").textContent = subtitle;
    });
  });
}

function bindDirectoryPickers() {
  document.querySelectorAll("[data-pick-dir]").forEach((button) => {
    button.addEventListener("click", async () => {
      const target = $(button.dataset.pickDir);
      const directory = await window.aias.dialog.selectDirectory();
      if (!directory) return;
      target.value = directory;
      await saveSettings();
      if (button.dataset.pickDir === "skin-path") await refreshSkins();
    });
  });
}

function renderFileList(id, files) {
  const list = $(id);
  list.innerHTML = "";
  for (const file of files) {
    const item = document.createElement("li");
    item.textContent = file;
    list.appendChild(item);
  }
}

function bindFilePickers() {
  $("pick-split-files").addEventListener("click", async () => {
    const files = await window.aias.dialog.selectFiles({ filters: [{ name: "DDS", extensions: ["dds"] }] });
    state.splitFiles = [...new Set([...state.splitFiles, ...files])];
    renderFileList("split-file-list", state.splitFiles);
  });
  $("clear-split-files").addEventListener("click", () => {
    state.splitFiles = [];
    renderFileList("split-file-list", state.splitFiles);
  });

  $("pick-image-files").addEventListener("click", async () => {
    const files = await window.aias.dialog.selectFiles({
      filters: [{ name: "Images", extensions: ["png", "tga", "jpg", "jpeg"] }]
    });
    state.imageFiles = [...new Set([...state.imageFiles, ...files])];
    renderFileList("image-file-list", state.imageFiles);
  });
  $("clear-image-files").addEventListener("click", () => {
    state.imageFiles = [];
    renderFileList("image-file-list", state.imageFiles);
  });
}

function bindTextureActions() {
  $("run-merge").addEventListener("click", async (event) => {
    await saveSettings();
    await withLog("merge-log", event.currentTarget, () =>
      window.aias.texture.mergePbr({
        inputPath: $("pbr-input").value,
        outputPath: $("pbr-output").value,
        alpha: $("pbr-alpha").value,
        format: $("pbr-format").value
      })
    );
  });

  $("run-split").addEventListener("click", async (event) => {
    await saveSettings();
    await withLog("split-log", event.currentTarget, () =>
      window.aias.texture.splitPbr({
        files: state.splitFiles,
        outputPath: $("split-output").value,
        exportFormat: $("split-format").value,
        exportAlpha: $("split-alpha").checked
      })
    );
  });

  $("run-mipmap").addEventListener("click", async (event) => {
    await saveSettings();
    await withLog("mipmap-log", event.currentTarget, () =>
      window.aias.texture.createMipmap({
        inputPath: $("mipmap-input").value,
        outputPath: $("mipmap-output").value,
        alpha: $("mipmap-alpha").value,
        format: $("mipmap-format").value
      })
    );
  });

  $("run-image-dds").addEventListener("click", async (event) => {
    await saveSettings();
    await withLog("image-log", event.currentTarget, () =>
      window.aias.texture.convertImagesToDds({
        files: state.imageFiles,
        outputPath: $("image-output").value,
        alpha: $("image-alpha").value,
        format: $("image-format").value
      })
    );
  });
}

async function refreshSkins() {
  const directory = $("skin-path").value;
  const list = $("skin-list");
  list.innerHTML = "";
  if (!directory) return;

  try {
    const files = await window.aias.skin.list(directory);
    for (const file of files) {
      const item = document.createElement("li");
      const label = document.createElement("div");
      const title = document.createElement("div");
      const meta = document.createElement("small");
      const actions = document.createElement("div");

      title.textContent = file.name;
      meta.textContent = `${file.disabled ? "已禁用" : "已启用"} · ${Math.round(file.size / 1024)} KB`;
      label.append(title, meta);

      actions.className = "skin-actions";
      const toggle = document.createElement("button");
      toggle.textContent = file.disabled ? "启用" : "禁用";
      toggle.addEventListener("click", async () => {
        await window.aias.skin.toggle(file.path);
        await refreshSkins();
      });
      const open = document.createElement("button");
      open.textContent = "打开";
      open.addEventListener("click", () => window.aias.shell.openPath(file.path));
      const remove = document.createElement("button");
      remove.textContent = "删除";
      remove.className = "danger";
      remove.addEventListener("click", async () => {
        if (!confirm(`删除 ${file.name}？`)) return;
        await window.aias.skin.delete(file.path);
        await refreshSkins();
      });
      actions.append(toggle, open, remove);
      item.append(label, actions);
      list.appendChild(item);
    }
  } catch (error) {
    const item = document.createElement("li");
    item.textContent = `读取失败：${error.message || error}`;
    list.appendChild(item);
  }
}

function bindSkinActions() {
  $("auto-detect-skins").addEventListener("click", async () => {
    const found = await window.aias.skin.autoDetect();
    if (found) {
      $("skin-path").value = found;
      await saveSettings();
      await refreshSkins();
    } else {
      alert("未检测到 War Thunder UserSkins 目录。");
    }
  });

  $("refresh-skins").addEventListener("click", refreshSkins);

  $("import-skins").addEventListener("click", async () => {
    const files = await window.aias.dialog.selectFiles();
    if (!files.length) return;
    await window.aias.skin.import({ files, targetDirectory: $("skin-path").value });
    await refreshSkins();
  });
}

async function init() {
  state.settings = await window.aias.settings.get();
  applySettingsToForm();
  bindNavigation();
  bindDirectoryPickers();
  bindFilePickers();
  bindTextureActions();
  bindSkinActions();
  $("settings-save").addEventListener("click", saveSettings);
  if ($("skin-path").value) await refreshSkins();
}

init();
