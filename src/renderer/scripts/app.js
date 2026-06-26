import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { openPath } from "@tauri-apps/plugin-opener";
import { check } from "@tauri-apps/plugin-updater";

const defaults = {
  autoUpdate: false,
  pbrInputPath: "",
  pbrOutputPath: "",
  pbrAlpha: "black",
  pbrFormat: "DXT5",
  splitOutputPath: "",
  splitExportFormat: "png",
  splitExportAlpha: true,
  mipmapInputPath: "",
  mipmapOutputPath: "",
  mipmapFormat: "DXT5",
  mipmapAlpha: "keep",
  imageToDdsOutputPath: "",
  imageToDdsAlpha: "keep",
  imageToDdsFormat: "DXT5",
  skinManagerPath: ""
};

const modeMeta = {
  merge: ["PBR 多通道合成", "扫描 Blender BSDF 命名贴图并输出游戏用 DDS 通道。"],
  split: ["PBR 多通道拆分", "把 _c.dds 与 _n.dds 拆回 BaseColor、Roughness、Metallic、Normal。"],
  mipmap: ["Mipmap 生成", "读取 p0、p1、p2... 图片序列并生成 DDS。"],
  "image-dds": ["图片转 DDS", "批量把 PNG、TGA、JPG 转换为 DDS。"],
  skins: ["涂装管理", "管理 War Thunder UserSkins 文件。"],
  settings: ["设置", "自动更新、数据和关于信息。"]
};

const state = {
  settings: {},
  splitFiles: [],
  imageFiles: [],
  activeMode: "merge"
};

const TOAST_LIMIT = 4;
const TOAST_TIMEOUT_MS = 4200;
let closeOpenPreviewDialog = null;
let closeOpenCustomSelect = null;

function $(id) {
  return document.getElementById(id);
}

function basename(value) {
  return String(value).split(/[\\/]/).pop();
}

function getSelectLabel(select) {
  return select.selectedOptions[0]?.textContent || select.options[select.selectedIndex]?.textContent || "";
}

function closeCustomSelect() {
  if (!closeOpenCustomSelect) return;
  closeOpenCustomSelect();
  closeOpenCustomSelect = null;
}

function positionCustomSelectMenu(wrapper, menu) {
  const rect = wrapper.getBoundingClientRect();
  const gap = 6;
  const availableBelow = window.innerHeight - rect.bottom - gap;
  const availableAbove = rect.top - gap;
  const menuHeight = Math.min(menu.scrollHeight || 0, 260);
  const openAbove = availableBelow < Math.min(menuHeight, 160) && availableAbove > availableBelow;
  const top = openAbove ? Math.max(gap, rect.top - menuHeight - gap) : Math.min(rect.bottom + gap, window.innerHeight - gap);

  menu.style.left = `${Math.round(rect.left)}px`;
  menu.style.top = `${Math.round(top)}px`;
  menu.style.width = `${Math.round(rect.width)}px`;
  menu.style.maxHeight = `${Math.max(120, Math.round(openAbove ? availableAbove : availableBelow))}px`;
}

function syncCustomSelect(select) {
  const wrapper = select.closest(".custom-select");
  const button = wrapper?.querySelector(".custom-select-button");
  if (!button) return;
  button.textContent = getSelectLabel(select);
}

function openCustomSelect(select, wrapper, button) {
  closeCustomSelect();

  const menu = document.createElement("div");
  menu.className = "custom-select-menu";
  menu.setAttribute("role", "listbox");
  menu.setAttribute("aria-label", select.getAttribute("aria-label") || select.title || "选择");

  [...select.options].forEach((option) => {
    const item = document.createElement("button");
    item.type = "button";
    item.className = "custom-select-option";
    item.textContent = option.textContent;
    item.dataset.value = option.value;
    item.setAttribute("role", "option");
    item.setAttribute("aria-selected", String(option.selected));
    if (option.selected) item.classList.add("selected");
    item.addEventListener("click", () => {
      select.value = option.value;
      select.dispatchEvent(new Event("change", { bubbles: true }));
      syncCustomSelect(select);
      closeCustomSelect();
      button.focus();
    });
    menu.appendChild(item);
  });

  document.body.appendChild(menu);
  wrapper.classList.add("open");
  button.setAttribute("aria-expanded", "true");
  positionCustomSelectMenu(wrapper, menu);

  const onPointerDown = (event) => {
    if (menu.contains(event.target) || wrapper.contains(event.target)) return;
    closeCustomSelect();
  };
  const onKeydown = (event) => {
    if (event.key === "Escape") {
      event.preventDefault();
      closeCustomSelect();
      button.focus();
      return;
    }
    if (event.key !== "ArrowDown" && event.key !== "ArrowUp") return;
    event.preventDefault();
    const options = [...menu.querySelectorAll(".custom-select-option")];
    const currentIndex = Math.max(0, options.findIndex((item) => item.classList.contains("selected")));
    const nextIndex = event.key === "ArrowDown"
      ? Math.min(options.length - 1, currentIndex + 1)
      : Math.max(0, currentIndex - 1);
    options[nextIndex]?.click();
  };
  const onReposition = () => positionCustomSelectMenu(wrapper, menu);

  document.addEventListener("pointerdown", onPointerDown, true);
  document.addEventListener("keydown", onKeydown);
  window.addEventListener("resize", onReposition);
  window.addEventListener("scroll", onReposition, true);

  closeOpenCustomSelect = () => {
    document.removeEventListener("pointerdown", onPointerDown, true);
    document.removeEventListener("keydown", onKeydown);
    window.removeEventListener("resize", onReposition);
    window.removeEventListener("scroll", onReposition, true);
    wrapper.classList.remove("open");
    button.setAttribute("aria-expanded", "false");
    menu.remove();
  };
}

function enhanceSelectMenus() {
  document.querySelectorAll("select").forEach((select) => {
    if (select.closest(".custom-select")) return;
    const wrapper = document.createElement("div");
    wrapper.className = "custom-select";
    select.parentNode.insertBefore(wrapper, select);
    wrapper.appendChild(select);

    const button = document.createElement("button");
    button.type = "button";
    button.className = "custom-select-button";
    button.setAttribute("aria-haspopup", "listbox");
    button.setAttribute("aria-expanded", "false");
    button.setAttribute("aria-label", select.getAttribute("aria-label") || select.title || "选择");
    wrapper.appendChild(button);
    syncCustomSelect(select);

    button.addEventListener("click", () => {
      if (wrapper.classList.contains("open")) closeCustomSelect();
      else openCustomSelect(select, wrapper, button);
    });
    select.addEventListener("change", () => {
      syncCustomSelect(select);
    });
  });
}

function createPreviewDialogCloser(overlay, resolve, fallbackValue = null) {
  if (closeOpenPreviewDialog) {
    closeOpenPreviewDialog(fallbackValue);
  } else {
    document.querySelectorAll(".preview-picker-backdrop").forEach((item) => item.remove());
  }

  let closed = false;
  const cleanup = [];
  const close = (value = fallbackValue) => {
    if (closed) return;
    closed = true;
    for (const dispose of cleanup) dispose();
    if (closeOpenPreviewDialog === close) closeOpenPreviewDialog = null;
    overlay.remove();
    resolve(value);
  };

  closeOpenPreviewDialog = close;
  return {
    close,
    addCleanup(dispose) {
      cleanup.push(dispose);
    }
  };
}

function openPreviewPicker({ title, defaultValue = "", multiline = false }) {
  return new Promise((resolve) => {
    const overlay = document.createElement("div");
    overlay.className = "preview-picker-backdrop";

    const dialog = document.createElement("div");
    dialog.className = "preview-picker";

    const heading = document.createElement("strong");
    heading.textContent = title;

    const input = multiline ? document.createElement("textarea") : document.createElement("input");
    input.setAttribute("aria-label", title);
    input.title = title;
    input.value = defaultValue;
    input.rows = 4;

    const actions = document.createElement("div");
    actions.className = "preview-picker-actions";

    const cancel = document.createElement("button");
    cancel.type = "button";
    cancel.textContent = "取消";

    const confirm = document.createElement("button");
    confirm.type = "button";
    confirm.textContent = "确定";

    const { close } = createPreviewDialogCloser(overlay, resolve, null);

    cancel.addEventListener("click", () => close(null));
    confirm.addEventListener("click", () => close(input.value.trim()));
    overlay.addEventListener("click", (event) => {
      if (event.target === overlay) close(null);
    });
    input.addEventListener("keydown", (event) => {
      if (event.key === "Escape") close(null);
      if (event.key === "Enter" && !multiline) close(input.value.trim());
    });

    actions.append(cancel, confirm);
    dialog.append(heading, input, actions);
    overlay.appendChild(dialog);
    document.body.appendChild(overlay);
    input.focus();
    input.select();
  });
}

function openPreviewMessage(title, body) {
  return new Promise((resolve) => {
    const overlay = document.createElement("div");
    overlay.className = "preview-picker-backdrop";

    const dialog = document.createElement("div");
    dialog.className = "preview-picker";

    const heading = document.createElement("strong");
    heading.textContent = title;

    const message = document.createElement("p");
    message.className = "preview-picker-message";
    message.textContent = body;

    const actions = document.createElement("div");
    actions.className = "preview-picker-actions";

    const confirm = document.createElement("button");
    confirm.type = "button";
    confirm.textContent = "确定";

    const { close, addCleanup } = createPreviewDialogCloser(overlay, resolve, undefined);

    confirm.addEventListener("click", close);
    overlay.addEventListener("click", (event) => {
      if (event.target === overlay) close();
    });
    const onKeydown = (event) => {
      if (event.key !== "Escape" && event.key !== "Enter") return;
      close();
    };
    document.addEventListener("keydown", onKeydown);
    addCleanup(() => document.removeEventListener("keydown", onKeydown));

    actions.append(confirm);
    dialog.append(heading, message, actions);
    overlay.appendChild(dialog);
    document.body.appendChild(overlay);
    confirm.focus();
  });
}

function openPreviewConfirm(title, body) {
  return new Promise((resolve) => {
    const overlay = document.createElement("div");
    overlay.className = "preview-picker-backdrop";

    const dialog = document.createElement("div");
    dialog.className = "preview-picker";

    const heading = document.createElement("strong");
    heading.textContent = title;

    const message = document.createElement("p");
    message.className = "preview-picker-message";
    message.textContent = body;

    const actions = document.createElement("div");
    actions.className = "preview-picker-actions";

    const cancel = document.createElement("button");
    cancel.type = "button";
    cancel.textContent = "取消";

    const confirm = document.createElement("button");
    confirm.type = "button";
    confirm.textContent = "确定";

    const { close, addCleanup } = createPreviewDialogCloser(overlay, resolve, false);

    cancel.addEventListener("click", () => close(false));
    confirm.addEventListener("click", () => close(true));
    overlay.addEventListener("click", (event) => {
      if (event.target === overlay) close(false);
    });
    const onKeydown = (event) => {
      if (event.key === "Escape") close(false);
      if (event.key === "Enter") close(true);
    };
    document.addEventListener("keydown", onKeydown);
    addCleanup(() => document.removeEventListener("keydown", onKeydown));

    actions.append(cancel, confirm);
    dialog.append(heading, message, actions);
    overlay.appendChild(dialog);
    document.body.appendChild(overlay);
    confirm.focus();
  });
}

function createBrowserPreviewApi() {
  let settings = {
    ...defaults,
    ...JSON.parse(localStorage.getItem("aias-preview-settings") || "{}")
  };

  const save = () => {
    localStorage.setItem("aias-preview-settings", JSON.stringify(settings));
    return { ...settings };
  };

  const previewOnly = async (feature) => ({
    completed: 0,
    total: 0,
    logs: [
      `${feature} 需要 Tauri 窗口里的本地文件权限。`,
      "当前浏览器预览用于调试界面交互、布局和状态。"
    ]
  });

  return {
    settings: {
      get: async () => ({ ...settings }),
      set: async (patch) => {
        settings = { ...settings, ...patch };
        return save();
      }
    },
    dialog: {
      selectDirectory: async () => {
        const value = await openPreviewPicker({
          title: "输入用于预览的文件夹路径",
          defaultValue: "F:\\AIAS\\Input"
        });
        return value || null;
      },
      selectFiles: async () => {
        const value = await openPreviewPicker({
          title: "输入用于预览的文件名，多个文件用逗号分隔",
          defaultValue: "sample_c.dds,sample_n.dds",
          multiline: true
        });
        return value ? value.split(",").map((item) => item.trim()).filter(Boolean) : [];
      }
    },
    texture: {
      findGroups: async () => [],
      mergePbr: () => previewOnly("PBR 合成"),
      splitPbr: () => previewOnly("PBR 拆分"),
      createMipmap: () => previewOnly("Mipmap 生成"),
      convertImagesToDds: () => previewOnly("图片转 DDS")
    },
    skin: {
      autoDetect: async () => {
        addActivity("浏览器预览", "自动检测 UserSkins 需要 Tauri 窗口。");
        return null;
      },
      list: async () => [
        { name: "sample_skin.blk", path: "sample_skin.blk", disabled: false, size: 2048 },
        { name: "disabled_skin.blk.disabled", path: "disabled_skin.blk.disabled", disabled: true, size: 1024 }
      ],
      import: async () => ({ imported: 0 }),
      toggle: async (filePath) => ({
        path: filePath.endsWith(".disabled") ? filePath.slice(0, -9) : `${filePath}.disabled`
      }),
      delete: async () => ({ deleted: true })
    },
    shell: {
      openPath: async (filePath) => addActivity("浏览器预览", `不能打开本地文件：${filePath}`)
    }
  };
}

function createTauriApi() {
  return {
    settings: {
      get: () => invoke("settings_get"),
      set: (patch) => invoke("settings_set", { patch })
    },
    dialog: {
      selectDirectory: async () => {
        const selected = await open({ directory: true, multiple: false });
        return Array.isArray(selected) ? selected[0] || null : selected;
      },
      selectFiles: async (options = {}) => {
        const selected = await open({
          multiple: true,
          filters: options.filters || []
        });
        if (!selected) return [];
        return Array.isArray(selected) ? selected : [selected];
      }
    },
    texture: {
      findGroups: (inputPath) => invoke("texture_find_groups", { inputPath }),
      mergePbr: (options) => invoke("texture_merge_pbr", { options }),
      splitPbr: (options) => invoke("texture_split_pbr", { options }),
      createMipmap: (options) => invoke("texture_create_mipmap", { options }),
      convertImagesToDds: (options) => invoke("texture_convert_images_to_dds", { options })
    },
    skin: {
      autoDetect: () => invoke("skin_auto_detect"),
      list: (directory) => invoke("skin_list", { directory }),
      import: (options) => invoke("skin_import", { options }),
      importFiles: (options) => invoke("skin_import", { options }),
      toggle: (filePath) => invoke("skin_toggle", { file_path: filePath }),
      delete: (filePath) => invoke("skin_delete", { file_path: filePath })
    },
    shell: {
      openPath
    }
  };
}

const isTauriRuntime = Boolean(window.__TAURI_INTERNALS__);
const api = isTauriRuntime ? createTauriApi() : createBrowserPreviewApi();

function setText(id, value) {
  const el = $(id);
  if (el) el.textContent = value;
}

function addActivity(title, body, tone = "idle") {
  const feed = $("toast-region");
  if (!feed) return;
  const key = `${tone}\u0000${title}\u0000${body}`;
  const latest = feed.firstElementChild;
  if (latest?.dataset.activityKey === key) {
    const detail = latest.querySelector("span");
    const count = Number(latest.dataset.activityCount || "1") + 1;
    latest.dataset.activityCount = String(count);
    if (detail) detail.textContent = `${body} ×${count}`;
    window.clearTimeout(Number(latest.dataset.dismissTimer || "0"));
    latest.dataset.dismissTimer = String(window.setTimeout(() => latest.remove(), TOAST_TIMEOUT_MS));
    return;
  }

  const item = document.createElement("article");
  item.className = `toast-item ${tone}`;
  item.dataset.activityKey = key;
  item.dataset.activityCount = "1";
  const heading = document.createElement("strong");
  heading.textContent = title;
  const detail = document.createElement("span");
  detail.textContent = body;
  const dismiss = document.createElement("button");
  dismiss.type = "button";
  dismiss.className = "toast-dismiss";
  dismiss.setAttribute("aria-label", "关闭通知");
  dismiss.textContent = "×";
  dismiss.addEventListener("click", () => item.remove());
  item.append(heading, detail);
  item.appendChild(dismiss);
  feed.prepend(item);
  item.dataset.dismissTimer = String(window.setTimeout(() => item.remove(), TOAST_TIMEOUT_MS));
  while (feed.children.length > TOAST_LIMIT) {
    feed.lastElementChild?.remove();
  }
}

function renderChips(containerId, files) {
  const container = $(containerId);
  if (!container) return;
  container.innerHTML = "";
  if (!files.length) {
    const empty = document.createElement("div");
    empty.className = "file-chip";
    empty.innerHTML = "<strong>暂无文件</strong><small>拖入或选择文件后会显示在这里</small>";
    container.appendChild(empty);
    return;
  }

  for (const file of files) {
    const chip = document.createElement("div");
    chip.className = "file-chip";
    const name = document.createElement("strong");
    name.textContent = basename(file);
    chip.appendChild(name);
    container.appendChild(chip);
  }
}

function syncPathChips() {
  renderChips("merge-chip-list", $("pbr-input")?.value ? [$("pbr-input").value] : []);
  renderChips("mipmap-chip-list", $("mipmap-input")?.value ? [$("mipmap-input").value] : []);
}

function renderSkinList(items) {
  const list = $("skin-list");
  if (!list) return;
  list.innerHTML = "";

  if (!items.length) {
    const empty = document.createElement("li");
    empty.className = "skin-card";
    empty.innerHTML = '<div class="skin-meta"><strong>暂无涂装</strong><small>选择目录后会显示文件</small></div>';
    list.appendChild(empty);
    return;
  }

  for (const file of items) {
    const item = document.createElement("li");
    item.className = "skin-card";

    const meta = document.createElement("div");
    meta.className = "skin-meta";
    const title = document.createElement("strong");
    title.textContent = file.name;
    const info = document.createElement("small");
    info.textContent = `${file.disabled ? "已禁用" : "已启用"} · ${Math.round(file.size / 1024)} KB`;
    meta.append(title, info);

    const actions = document.createElement("div");
    actions.className = "skin-actions";

    const toggle = document.createElement("button");
    toggle.textContent = file.disabled ? "启用" : "禁用";
    toggle.addEventListener("click", async () => {
      await api.skin.toggle(file.path);
      await refreshSkins();
    });

    const open = document.createElement("button");
    open.textContent = "打开";
    open.addEventListener("click", () => api.shell.openPath(file.path));

    const remove = document.createElement("button");
    remove.textContent = "删除";
    remove.className = "danger";
    remove.addEventListener("click", async () => {
      const confirmed = await openPreviewConfirm("删除涂装", `删除 ${file.name}？`);
      if (!confirmed) return;
      await api.skin.delete(file.path);
      addActivity("已删除涂装", file.name, "success");
      await refreshSkins();
    });

    actions.append(toggle, open, remove);
    item.append(meta, actions);
    list.appendChild(item);
  }
}

function setBusy(button, busy) {
  if (!button) return;
  button.disabled = busy;
  button.dataset.originalText ||= button.textContent;
  button.textContent = busy ? "处理中..." : button.dataset.originalText;
}

async function withLog(logId, button, action, title) {
  const log = $(logId);
  if (log) log.textContent = "";
  setBusy(button, true);
  try {
    const result = await action();
    for (const line of result.logs || []) {
      if (log) log.textContent += `${line}\n`;
    }
    if (log) log.textContent += `完成：${result.completed} / ${result.total}`;
    addActivity(title || "任务完成", `${result.completed} / ${result.total}`, "success");
  } catch (error) {
    if (log) log.textContent += `失败：${error.message || error}`;
    addActivity(title || "任务失败", error.message || String(error), "error");
  } finally {
    setBusy(button, false);
  }
}

function collectSettings() {
  return {
    pbrInputPath: $("pbr-input")?.value || "",
    pbrOutputPath: $("pbr-output")?.value || "",
    pbrAlpha: $("pbr-alpha")?.value || "black",
    pbrFormat: $("pbr-format")?.value || "DXT5",
    splitOutputPath: $("split-output")?.value || "",
    splitExportFormat: $("split-format")?.value || "png",
    splitExportAlpha: $("split-alpha")?.checked ?? true,
    mipmapInputPath: $("mipmap-input")?.value || "",
    mipmapOutputPath: $("mipmap-output")?.value || "",
    mipmapAlpha: $("mipmap-alpha")?.value || "keep",
    mipmapFormat: $("mipmap-format")?.value || "DXT5",
    imageToDdsOutputPath: $("image-output")?.value || "",
    imageToDdsAlpha: $("image-alpha")?.value || "keep",
    imageToDdsFormat: $("image-format")?.value || "DXT5",
    skinManagerPath: $("skin-path")?.value || ""
  };
}

function applySettingsToForm() {
  const settings = state.settings || {};
  const map = {
    "pbr-input": settings.pbrInputPath,
    "pbr-output": settings.pbrOutputPath,
    "pbr-alpha": settings.pbrAlpha,
    "pbr-format": settings.pbrFormat,
    "split-output": settings.splitOutputPath,
    "split-format": settings.splitExportFormat,
    "split-alpha": settings.splitExportAlpha,
    "mipmap-input": settings.mipmapInputPath,
    "mipmap-output": settings.mipmapOutputPath,
    "mipmap-alpha": settings.mipmapAlpha,
    "mipmap-format": settings.mipmapFormat,
    "image-output": settings.imageToDdsOutputPath,
    "image-alpha": settings.imageToDdsAlpha,
    "image-format": settings.imageToDdsFormat,
    "skin-path": settings.skinManagerPath
  };

  for (const [id, value] of Object.entries(map)) {
    const el = $(id);
    if (!el) continue;
    if (el.type === "checkbox") el.checked = Boolean(value);
    else el.value = value || "";
    if (el.tagName === "SELECT") syncCustomSelect(el);
  }
  syncPathChips();
}

async function saveSettings() {
  state.settings = await api.settings.set(collectSettings());
  updateStatus();
  syncPathChips();
  addActivity("配置已保存", isTauriRuntime ? "设置已写入本地配置。" : "设置已保存到浏览器预览存储。", "success");
}

function updateRunButtons(mode) {
  const mapping = {
    merge: "run-merge",
    split: "run-split",
    mipmap: "run-mipmap",
    "image-dds": "run-image-dds"
  };

  document.querySelectorAll(".run-button").forEach((button) => button.classList.add("hidden"));
  const active = $(mapping[mode]);
  if (active) active.classList.remove("hidden");

  $("auto-detect-skins")?.classList.toggle("hidden", mode !== "skins");
  $("clear-split-files")?.classList.toggle("hidden", mode !== "split");
  $("clear-image-files")?.classList.toggle("hidden", mode !== "image-dds");
  $("import-skins")?.classList.toggle("hidden", mode !== "skins");
  $("refresh-skins")?.classList.toggle("hidden", mode !== "skins");
}

function updateInspector() {
  document.querySelectorAll(".inspector-group").forEach((group) => {
    const modes = (group.dataset.modes || "").split(/\s+/).filter(Boolean);
    group.classList.toggle("hidden", modes.length > 0 && !modes.includes(state.activeMode));
  });

  document.querySelectorAll(".mode-field").forEach((field) => {
    const modes = (field.dataset.modes || "").split(/\s+/).filter(Boolean);
    field.classList.toggle("hidden", modes.length > 0 && !modes.includes(state.activeMode));
  });

  const fieldVisibility = {
    "pbr-input": state.activeMode === "merge",
    "pbr-output": state.activeMode === "merge",
    "split-output": state.activeMode === "split",
    "mipmap-input": state.activeMode === "mipmap",
    "mipmap-output": state.activeMode === "mipmap",
    "image-output": state.activeMode === "image-dds",
    "skin-path": state.activeMode === "skins"
  };

  for (const [id, visible] of Object.entries(fieldVisibility)) {
    const row = document.querySelector(`[data-field="${id}"]`);
    if (row) row.classList.toggle("hidden", !visible);
  }
}

function updateStatus() {
  const mode = state.activeMode;
  const ready = (() => {
    switch (mode) {
      case "merge":
        return Boolean($("pbr-input")?.value && $("pbr-output")?.value);
      case "split":
        return Boolean($("split-output")?.value && state.splitFiles.length);
      case "mipmap":
        return Boolean($("mipmap-input")?.value && $("mipmap-output")?.value);
      case "image-dds":
        return Boolean($("image-output")?.value && state.imageFiles.length);
      case "skins":
        return Boolean($("skin-path")?.value);
      default:
        return false;
    }
  })();

  const inputText = (() => {
    switch (mode) {
      case "merge":
        return $("pbr-input")?.value || "未选择";
      case "split":
        return `${state.splitFiles.length} 个 DDS`;
      case "mipmap":
        return $("mipmap-input")?.value || "未选择";
      case "image-dds":
        return `${state.imageFiles.length} 张图片`;
      case "skins":
        return $("skin-path")?.value || "未选择";
      default:
        return "未选择";
    }
  })();

  const outputText = (() => {
    switch (mode) {
      case "merge":
        return $("pbr-output")?.value || "未就绪";
      case "split":
        return $("split-output")?.value || "未就绪";
      case "mipmap":
        return $("mipmap-output")?.value || "未就绪";
      case "image-dds":
        return $("image-output")?.value || "未就绪";
      case "skins":
        return $("skin-path")?.value || "未就绪";
      default:
        return "未就绪";
    }
  })();

  setText("status-mode", modeMeta[mode][0]);
  setText("status-input", inputText);
  setText("status-output", outputText);
  setText("status-ready", ready ? "可运行" : "等待配置");
  setText("current-task", modeMeta[mode][0]);
  setText("view-title", modeMeta[mode][0]);
  setText("view-subtitle", modeMeta[mode][1]);
  setText("inspector-mode", modeMeta[mode][0]);
  setText("runtime-badge", isTauriRuntime ? "Tauri" : "Browser Preview");
}

function getRunBlocker(mode) {
  switch (mode) {
    case "merge":
      if (!$("pbr-input")?.value) return "请选择输入文件夹。";
      if (!$("pbr-output")?.value) return "请选择输出文件夹。";
      return null;
    case "split":
      if (!state.splitFiles.length) return "请添加 DDS 文件。";
      if (!$("split-output")?.value) return "请选择输出文件夹。";
      return null;
    case "mipmap":
      if (!$("mipmap-input")?.value) return "请选择输入文件夹。";
      if (!$("mipmap-output")?.value) return "请选择输出文件夹。";
      return null;
    case "image-dds":
      if (!state.imageFiles.length) return "请添加图片文件。";
      if (!$("image-output")?.value) return "请选择输出文件夹。";
      return null;
    default:
      return null;
  }
}

function reportRunBlocker(message) {
  addActivity("无法运行", message, "error");
  if (!isTauriRuntime) {
    openPreviewMessage("无法运行", message);
  }
}

function moveModeIndicator(tab) {
  const track = $("modeTabsTrack");
  const indicator = $("modeTabsIndicator");
  if (!track || !indicator) return;
  const tr = track.getBoundingClientRect();
  const tr2 = tab.getBoundingClientRect();
  indicator.style.left = (tr2.left - tr.left) + "px";
  indicator.style.width = tr2.width + "px";
}

function applyMode(mode) {
  closeCustomSelect();
  state.activeMode = mode;
  document.querySelectorAll(".mode-tab").forEach((button) => {
    button.classList.toggle("active", button.dataset.view === mode);
    if (button.dataset.view === mode) moveModeIndicator(button);
  });
  document.querySelectorAll(".mode-view").forEach((view) => {
    view.classList.toggle("active", view.id === `view-${mode}`);
  });
  // Settings mode: hide status strip, inspector; expand to full width
  const statusStrip = document.querySelector(".status-strip");
  if (statusStrip) statusStrip.classList.toggle("hidden", mode === "settings");
  const workspace = document.querySelector(".workspace");
  if (workspace) workspace.classList.toggle("full-width", mode === "settings");
  const inspector = document.querySelector(".inspector");
  if (inspector) inspector.classList.toggle("hidden", mode === "settings");
  if (mode === "settings") syncSettingsView();
  updateRunButtons(mode);
  updateInspector();
  updateStatus();
}

function bindTabs() {
  document.querySelectorAll(".mode-tab").forEach((button) => {
    button.addEventListener("click", () => applyMode(button.dataset.view));
  });
}

function bindInspectorGroups() {
  document.querySelectorAll(".group-toggle").forEach((button) => {
    button.setAttribute("aria-expanded", "true");
  });
}

function syncSettingsView() {
  const el = $("set-auto-update");
  if (el) el.checked = state.settings.autoUpdate !== false;
  // Show data directory
  (async () => {
    try {
      const { appDataDir } = await import("@tauri-apps/api/path");
      const dir = await appDataDir();
      const input = $("set-data-dir");
      if (input) input.value = dir;
    } catch (e) { /* not in Tauri */ }
  })();
}

function bindSettingsActions() {
  $("set-auto-update")?.addEventListener("change", async () => {
    state.settings.autoUpdate = $("set-auto-update")?.checked ?? true;
    await api.settings.set({ autoUpdate: state.settings.autoUpdate });
    addActivity("已更新", state.settings.autoUpdate ? "自动检查更新已开启" : "自动检查更新已关闭", "success");
  });

  $("set-check-update")?.addEventListener("click", () => checkForUpdates(false));

  $("set-open-dir")?.addEventListener("click", async () => {
    try {
      const { appDataDir } = await import("@tauri-apps/api/path");
      const dir = await appDataDir();
      if (isTauriRuntime) await openPath(dir);
      else addActivity("数据目录", dir);
    } catch (e) {
      addActivity("打开失败", e.message || String(e), "error");
    }
  });

  $("set-reset")?.addEventListener("click", async () => {
    const confirmed = await openPreviewConfirm("重置设置", "所有路径和选项将恢复默认值，确定继续？");
    if (!confirmed) return;
    await api.settings.set(defaults);
    state.settings = { ...defaults };
    applySettingsToForm();
    updateStatus();
    addActivity("已重置", "所有设置已恢复默认值", "success");
  });

  $("set-license")?.addEventListener("click", () => {
    openPreviewMessage("MIT License", "Copyright (c) 2025 Avrora.CL\n\nPermission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files, to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software.");
  });
}

function bindDropZones() {
  document.querySelectorAll("[data-pick-dir]").forEach((button) => {
    button.addEventListener("click", async () => {
      const target = $(button.dataset.pickDir);
      const directory = await api.dialog.selectDirectory();
      if (!directory || !target) return;
      target.value = directory;
      await saveSettings();
      syncPathChips();
      if (button.dataset.pickDir === "skin-path") {
        await refreshSkins();
      }
    });
    button.addEventListener("keydown", async (event) => {
      if (event.key !== "Enter" && event.key !== " ") return;
      event.preventDefault();
      button.click();
    });
  });

  $("pick-split-files")?.addEventListener("click", async () => {
    const files = await api.dialog.selectFiles({ filters: [{ name: "DDS", extensions: ["dds"] }] });
    state.splitFiles = [...new Set([...state.splitFiles, ...files])];
    renderChips("split-file-list", state.splitFiles);
    updateStatus();
  });

  $("pick-image-files")?.addEventListener("click", async () => {
    const files = await api.dialog.selectFiles({
      filters: [{ name: "Images", extensions: ["png", "tga", "jpg", "jpeg"] }]
    });
    state.imageFiles = [...new Set([...state.imageFiles, ...files])];
    renderChips("image-file-list", state.imageFiles);
    updateStatus();
  });
}

function bindFileControls() {
  $("clear-split-files")?.addEventListener("click", () => {
    state.splitFiles = [];
    renderChips("split-file-list", []);
    updateStatus();
    addActivity("已清空列表", "DDS 文件列表已清空。");
  });

  $("clear-image-files")?.addEventListener("click", () => {
    state.imageFiles = [];
    renderChips("image-file-list", []);
    updateStatus();
    addActivity("已清空列表", "图片文件列表已清空。");
  });
}

function bindRunActions() {
  $("run-merge")?.addEventListener("click", async (event) => {
    await saveSettings();
    const blocker = getRunBlocker("merge");
    if (blocker) {
      reportRunBlocker(blocker);
      return;
    }
    addActivity("开始合成", $("pbr-input")?.value || "未选择输入目录");
    await withLog(
      "merge-log",
      event.currentTarget,
      () =>
        api.texture.mergePbr({
          inputPath: $("pbr-input").value,
          outputPath: $("pbr-output").value,
          alpha: $("pbr-alpha").value,
          format: $("pbr-format").value
        }),
      "PBR 合成"
    );
  });

  $("run-split")?.addEventListener("click", async (event) => {
    await saveSettings();
    const blocker = getRunBlocker("split");
    if (blocker) {
      reportRunBlocker(blocker);
      return;
    }
    addActivity("开始拆分", `${state.splitFiles.length} 个 DDS 文件`);
    await withLog(
      "split-log",
      event.currentTarget,
      () =>
        api.texture.splitPbr({
          files: state.splitFiles,
          outputPath: $("split-output").value,
          exportFormat: $("split-format").value,
          exportAlpha: $("split-alpha").checked
        }),
      "PBR 拆分"
    );
  });

  $("run-mipmap")?.addEventListener("click", async (event) => {
    await saveSettings();
    const blocker = getRunBlocker("mipmap");
    if (blocker) {
      reportRunBlocker(blocker);
      return;
    }
    addActivity("开始生成", $("mipmap-input")?.value || "未选择输入目录");
    await withLog(
      "mipmap-log",
      event.currentTarget,
      () =>
        api.texture.createMipmap({
          inputPath: $("mipmap-input").value,
          outputPath: $("mipmap-output").value,
          alpha: $("mipmap-alpha").value,
          format: $("mipmap-format").value
        }),
      "Mipmap 生成"
    );
  });

  $("run-image-dds")?.addEventListener("click", async (event) => {
    await saveSettings();
    const blocker = getRunBlocker("image-dds");
    if (blocker) {
      reportRunBlocker(blocker);
      return;
    }
    addActivity("开始转换", `${state.imageFiles.length} 张图片`);
    await withLog(
      "image-log",
      event.currentTarget,
      () =>
        api.texture.convertImagesToDds({
          files: state.imageFiles,
          outputPath: $("image-output").value,
          alpha: $("image-alpha").value,
          format: $("image-format").value
        }),
      "图片转 DDS"
    );
  });
}

function bindSkinActions() {
  $("auto-detect-skins")?.addEventListener("click", async () => {
    const found = await api.skin.autoDetect();
    if (found) {
      $("skin-path").value = found;
      await saveSettings();
      await refreshSkins();
    } else {
      addActivity("未检测到目录", "未检测到 War Thunder UserSkins 目录。", "error");
      if (!isTauriRuntime) {
        await openPreviewMessage("浏览器预览", "自动检测 UserSkins 需要 Tauri 窗口。");
      }
    }
  });

  $("refresh-skins")?.addEventListener("click", () => refreshSkins({ notify: true }));

  $("import-skins")?.addEventListener("click", async () => {
    const files = await api.dialog.selectFiles();
    if (!files.length) return;
    await api.skin.import({ files, targetDirectory: $("skin-path").value });
    await refreshSkins({ notify: true });
  });
}

async function refreshSkins({ notify = false } = {}) {
  const directory = $("skin-path")?.value;
  if (!directory) {
    renderSkinList([]);
    updateStatus();
    if (notify) {
      addActivity("涂装列表", "请选择 UserSkins 目录后刷新。");
    }
    return;
  }

  try {
    const files = await api.skin.list(directory);
    renderSkinList(files);
    if (notify) {
      addActivity("涂装列表已刷新", `${files.length} 个文件`, "success");
    }
  } catch (error) {
    const list = $("skin-list");
    if (list) {
      list.innerHTML = "";
      const item = document.createElement("li");
      item.className = "skin-card";
      item.textContent = `读取失败：${error.message || error}`;
      list.appendChild(item);
    }
    if (notify) {
      addActivity("读取失败", error.message || String(error), "error");
    }
  }

  updateStatus();
}

async function checkForUpdates(silent = true) {
  try {
    const update = await check();
    if (!update) {
      if (!silent) addActivity("已是最新版本", "当前版本 " + (state.settings.version || "4.1.0"), "success");
      return;
    }
    $("update-button")?.classList.remove("hidden");
    addActivity("更新可用", update.version + " — 点击下载", "success");
    if (!silent) {
      const confirmed = await openPreviewConfirm("发现新版本", "版本 " + update.version + " 可用。\n\n是否立即下载并安装更新？");
      if (!confirmed) return;
      addActivity("正在下载更新", update.version);
      let downloaded = 0;
      await update.downloadAndInstall((event) => {
        if (event.event === "Progress") {
          downloaded = Math.round((event.data.downloaded / event.data.total) * 100);
          $("runtime-badge").textContent = "下载 " + downloaded + "%";
        }
      });
      addActivity("更新已下载", "重启应用以完成更新", "success");
    }
  } catch (error) {
    if (!silent) addActivity("检查更新失败", error.message || String(error), "error");
  }
}

async function init() {
  state.settings = await api.settings.get();
  enhanceSelectMenus();
  applySettingsToForm();
  bindTabs();
  bindInspectorGroups();
  bindDropZones();
  bindFileControls();
  bindRunActions();
  bindSkinActions();
  bindSettingsActions();
  $("update-button")?.addEventListener("click", () => checkForUpdates(false));

  applyMode("merge");
  // Init tab indicator position
  setTimeout(() => {
    const activeTab = document.querySelector(".mode-tab.active");
    if (activeTab) moveModeIndicator(activeTab);
  }, 50);
  renderChips("merge-chip-list", []);
  renderChips("split-file-list", []);
  renderChips("mipmap-chip-list", []);
  renderChips("image-file-list", []);
  await refreshSkins();
  syncPathChips();
  updateStatus();

  // Check for updates silently on startup
  if (isTauriRuntime) setTimeout(() => checkForUpdates(true), 2000);
}

init();
