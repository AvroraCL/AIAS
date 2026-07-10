import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { openPath } from "@tauri-apps/plugin-opener";
import { check } from "@tauri-apps/plugin-updater";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { listen } from "@tauri-apps/api/event";
import {
  createIcons,
  Layers3,
  Split,
  GalleryVerticalEnd,
  Image,
  Images,
  Shirt,
  Settings2,
  History,
  Bell,
  FolderInput,
  FileStack,
  Search,
  FolderOpen,
  HardDrive,
  Upload,
  RefreshCw,
  Database,
  RotateCcw,
  Info,
  ExternalLink,
  ChevronDown,
  Trash2,
  Play,
  X
} from "lucide";

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
  merge: { title: "PBR 多通道合成", description: "生成游戏可用的 _c 与 _n 通道贴图" },
  split: { title: "PBR 多通道拆分", description: "提取 BaseColor、Alpha、材质与法线通道" },
  mipmap: { title: "Mipmap 生成", description: "将分层图片序列组装为单个 DDS" },
  "image-dds": { title: "图片转 DDS", description: "批量转换图片并统一 DDS 压缩格式" },
  skins: { title: "涂装管理", description: "管理 War Thunder UserSkins 资源" },
  settings: { title: "应用设置", description: "更新、数据路径与版本信息" }
};

const state = {
  settings: {},
  splitFiles: [],
  imageFiles: [],
  activeMode: "merge",
  activityCount: 0,
  lastOutputPath: "",
  updateInProgress: false,
  taskProgressActive: false
};

const iconSet = {
  Layers3,
  Split,
  GalleryVerticalEnd,
  Image,
  Images,
  Shirt,
  Settings2,
  History,
  Bell,
  FolderInput,
  FileStack,
  Search,
  FolderOpen,
  HardDrive,
  Upload,
  RefreshCw,
  Database,
  RotateCcw,
  Info,
  ExternalLink,
  ChevronDown,
  Trash2,
  Play,
  X
};

const TOAST_LIMIT = 4;
const TOAST_TIMEOUT_MS = 2500;
let closeOpenPreviewDialog = null;
let closeOpenCustomSelect = null;

function $(id) {
  return document.getElementById(id);
}

function refreshIcons(root = document) {
  createIcons({
    icons: iconSet,
    attrs: { "aria-hidden": "true" },
    nameAttr: "data-lucide",
    root
  });
}

function basename(value) {
  return String(value).split(/[\\/]/).pop();
}

function compactPath(value, fallback) {
  if (!value) return fallback;
  return basename(value) || value;
}

function getModeOutputPath(mode = state.activeMode) {
  const fieldByMode = {
    merge: "pbr-output",
    split: "split-output",
    mipmap: "mipmap-output",
    "image-dds": "image-output",
    skins: "skin-path"
  };
  return $(fieldByMode[mode])?.value || "";
}

function formatSize(bytes) {
  if (bytes < 1024) return bytes + " B";
  if (bytes < 1048576) return (bytes / 1024).toFixed(1) + " KB";
  return (bytes / 1048576).toFixed(1) + " MB";
}

function formatDate(timestamp) {
  if (!timestamp) return "未知日期";
  return new Intl.DateTimeFormat("zh-CN", {
    month: "2-digit",
    day: "2-digit"
  }).format(new Date(timestamp));
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
      selectDirectories: async () => {
        const value = await openPreviewPicker({
          title: "输入用于预览的涂装文件夹，多项用逗号分隔",
          defaultValue: "F:\\WarThunder\\UserSkins\\sample",
          multiline: true
        });
        return value ? value.split(",").map((item) => item.trim()).filter(Boolean) : [];
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
      autoDetect: async () => null,
      list: async () => [],
      import: async () => ({ imported: 0, errors: [] }),
      toggle: async (filePath) => ({ path: filePath }),
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
      selectDirectories: async () => {
        const selected = await open({ directory: true, multiple: true });
        if (!selected) return [];
        return Array.isArray(selected) ? selected : [selected];
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
      toggle: (filePath) => invoke("skin_toggle", { filePath }),
      delete: (filePath) => invoke("skin_delete", { filePath })
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

function appendActivityHistory(title, body, tone) {
  const feed = $("activity-feed");
  if (!feed) return;

  if (state.activityCount === 0) feed.innerHTML = "";

  const item = document.createElement("article");
  item.className = `activity-item ${tone}`;
  const heading = document.createElement("strong");
  heading.textContent = title;
  const detail = document.createElement("span");
  detail.textContent = body;
  const time = document.createElement("time");
  time.textContent = new Intl.DateTimeFormat("zh-CN", {
    hour: "2-digit",
    minute: "2-digit"
  }).format(new Date());
  item.append(heading, detail, time);
  feed.prepend(item);

  while (feed.children.length > 40) feed.lastElementChild?.remove();
  state.activityCount += 1;
  setText("activity-count", String(state.activityCount));
  setText("activity-summary", `${state.activityCount} 条记录`);
}

function addActivity(title, body, tone = "idle") {
  appendActivityHistory(title, body, tone);
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
    latest.dataset.dismissTimer = String(window.setTimeout(() => { latest.classList.add("removing"); setTimeout(() => latest.remove(), 250); }, TOAST_TIMEOUT_MS));
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
  const dismissBtn = document.createElement("button");
  dismissBtn.type = "button";
  dismissBtn.className = "toast-dismiss";
  dismissBtn.setAttribute("aria-label", "关闭通知");
  dismissBtn.textContent = "×";
  const removeToast = () => {
    item.classList.add("removing");
    setTimeout(() => item.remove(), 250);
  };
  dismissBtn.addEventListener("click", removeToast);
  item.append(heading, detail);
  item.appendChild(dismissBtn);
  feed.prepend(item);
  item.dataset.dismissTimer = String(window.setTimeout(removeToast, TOAST_TIMEOUT_MS));
  while (feed.children.length > TOAST_LIMIT) {
    const last = feed.lastElementChild;
    if (last) { last.classList.add("removing"); setTimeout(() => last.remove(), 250); }
  }
}

function setActivityPanel(open) {
  const panel = $("activity-panel");
  if (!panel) return;
  panel.classList.toggle("collapsed", !open);
  $("activity-toggle")?.setAttribute("aria-expanded", String(open));
}

function syncActiveLog(mode = state.activeMode) {
  const logByMode = {
    merge: "merge-log",
    split: "split-log",
    mipmap: "mipmap-log",
    "image-dds": "image-log"
  };
  document.querySelectorAll(".task-log").forEach((log) => {
    log.classList.toggle("active", log.id === logByMode[mode]);
  });
}

const selectionCountIds = {
  "merge-chip-list": "merge-selection-count",
  "split-file-list": "split-selection-count",
  "mipmap-chip-list": "mipmap-selection-count",
  "image-file-list": "image-selection-count"
};

async function removeSelection(containerId, file) {
  if (containerId === "split-file-list") {
    state.splitFiles = state.splitFiles.filter((item) => item !== file);
    renderChips(containerId, state.splitFiles);
  } else if (containerId === "image-file-list") {
    state.imageFiles = state.imageFiles.filter((item) => item !== file);
    renderChips(containerId, state.imageFiles);
  } else if (containerId === "merge-chip-list") {
    $("pbr-input").value = "";
    renderChips(containerId, []);
    await saveSettings();
  } else if (containerId === "mipmap-chip-list") {
    $("mipmap-input").value = "";
    renderChips(containerId, []);
    await saveSettings();
  }
  updateStatus();
}

function renderChips(containerId, files) {
  const container = $(containerId);
  if (!container) return;
  container.innerHTML = "";
  setText(selectionCountIds[containerId], `${files.length} 项`);

  if (!files.length) {
    const empty = document.createElement("div");
    empty.className = "file-chip empty-chip";
    empty.innerHTML = "<strong>暂无选择</strong>";
    container.appendChild(empty);
    return;
  }

  for (const file of files) {
    const chip = document.createElement("div");
    chip.className = "file-chip";
    chip.title = file;
    const copy = document.createElement("div");
    const name = document.createElement("strong");
    name.textContent = basename(file);
    const path = document.createElement("small");
    path.textContent = file;
    copy.append(name, path);

    const remove = document.createElement("button");
    remove.type = "button";
    remove.className = "chip-remove";
    remove.title = `移除 ${basename(file)}`;
    remove.setAttribute("aria-label", remove.title);
    remove.innerHTML = '<i data-lucide="x"></i>';
    remove.addEventListener("click", (event) => {
      event.stopPropagation();
      removeSelection(containerId, file);
    });

    chip.append(copy, remove);
    container.appendChild(chip);
  }
  refreshIcons(container);
}

function syncPathChips() {
  renderChips("merge-chip-list", $("pbr-input")?.value ? [$("pbr-input").value] : []);
  renderChips("mipmap-chip-list", $("mipmap-input")?.value ? [$("mipmap-input").value] : []);
}

function renderSkinList(items) {
  const grid = $("skin-grid");
  const empty = $("skin-empty");
  const banner = $("skin-banner");
  const actionsBar = $("skin-actions");
  if (!grid) return;

  grid.innerHTML = "";
  const hasDir = Boolean($("skin-path")?.value);

  if (banner) banner.classList.toggle("hidden", !hasDir);
  if (empty) empty.classList.toggle("hidden", items.length > 0);
  if (actionsBar) actionsBar.classList.toggle("hidden", !hasDir);
  if ($("skin-count")) $("skin-count").textContent = `${items.length} 个涂装`;
  if ($("skin-dir-path")) $("skin-dir-path").textContent = $("skin-path")?.value || "未设置";

  setText("skin-empty-title", hasDir ? "目录中暂无涂装" : "尚未连接 UserSkins 目录");
  setText("skin-empty-description", hasDir ? "导入涂装文件夹后会显示在这里" : "选择目录后即可管理涂装");

  if (!hasDir || !items.length) return;

  // Apply sort
  const sortBy = $("skin-sort")?.value || "name-asc";
  const sorted = [...items].sort((a, b) => {
    switch (sortBy) {
      case "name-desc": return b.name.localeCompare(a.name);
      case "size-desc": return (b.fileCount || 0) - (a.fileCount || 0);
      case "date-desc": return (b.modifiedAt || 0) - (a.modifiedAt || 0);
      default: return a.name.localeCompare(b.name); // name-asc
    }
  });

  for (const entry of sorted) {
    const card = document.createElement("div");
    card.className = "skin-card";

    const name = document.createElement("span");
    name.className = "skin-card-name";
    name.textContent = entry.name.replace(/\.disabled$/, "");
    name.title = entry.name;

    const meta = document.createElement("span");
    meta.className = "skin-card-meta";
    meta.textContent = `${formatSize(entry.fileCount)} · ${formatDate(entry.modifiedAt)}`;

    const toggle = document.createElement("label");
    toggle.className = "toggle-label";
    const cb = document.createElement("input");
    cb.type = "checkbox";
    cb.checked = !entry.disabled;
    cb.setAttribute("aria-label", `${cb.checked ? "禁用" : "启用"} ${name.textContent}`);
    cb.addEventListener("change", async () => {
      await api.skin.toggle(entry.path);
      addActivity(cb.checked ? "已启用" : "已禁用", entry.name.replace(/\.disabled$/, ""), "success");
      await refreshSkins();
    });
    const track = document.createElement("span");
    track.className = "toggle-track";
    const thumb = document.createElement("span");
    thumb.className = "toggle-thumb";
    track.appendChild(thumb);
    toggle.append(cb, track);

    const del = document.createElement("button");
    del.className = "skin-delete danger";
    del.type = "button";
    del.title = `删除 ${name.textContent}`;
    del.setAttribute("aria-label", del.title);
    del.innerHTML = '<i data-lucide="trash-2"></i>';
    del.addEventListener("click", async () => {
      const confirmed = await openPreviewConfirm("删除涂装", `确定删除 ${entry.name.replace(/\.disabled$/, "")}？`);
      if (!confirmed) return;
      try {
        await api.skin.delete(entry.path);
        addActivity("已删除", entry.name.replace(/\.disabled$/, ""), "success");
        await refreshSkins();
      } catch (e) {
        addActivity("删除失败", e.message || String(e), "error");
      }
    });

    card.append(name, meta, toggle, del);

    grid.appendChild(card);
  }
  refreshIcons(grid);
}

function setBusy(button, busy) {
  if (!button) return;
  const label = button.querySelector("span") || button;
  button.dataset.busy = String(busy);
  button.dataset.originalText ||= label.textContent;
  label.textContent = busy ? "处理中..." : button.dataset.originalText;
  button.classList.toggle("busy", busy);
  button.disabled = busy;
}

function setTaskProgress(completed, total, message) {
  const panel = $("task-progress");
  if (!panel) return;
  const percent = total ? Math.min(100, Math.round((completed / total) * 100)) : 0;
  panel.classList.remove("hidden");
  setText("task-progress-label", message || "正在处理");
  setText("task-progress-value", `${percent}%`);
  $("task-progress-fill")?.style.setProperty("width", `${percent}%`);
}

async function withLog(logId, button, action, title) {
  const log = $(logId);
  if (log) log.textContent = "";
  setActivityPanel(true);
  setText("activity-summary", `${title}运行中`);
  setBusy(button, true);
  state.taskProgressActive = true;
  setTaskProgress(0, 1, "正在准备任务");
  try {
    const result = await action();
    for (const line of result.logs || []) {
      if (log) log.textContent += `${line}\n`;
    }
    if (log) log.textContent += `完成：${result.completed} / ${result.total}`;
    setTaskProgress(result.completed, result.total, "任务完成");
    addActivity(title || "任务完成", `${result.completed} / ${result.total}`, "success");
    state.lastOutputPath = getModeOutputPath();
    $("open-current-output")?.classList.toggle("hidden", !state.lastOutputPath);
  } catch (error) {
    if (log) log.textContent += `失败：${error.message || error}`;
    addActivity(title || "任务失败", error.message || String(error), "error");
  } finally {
    state.taskProgressActive = false;
    setBusy(button, false);
    updateStatus();
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

async function saveSettings({ notify = false } = {}) {
  state.settings = await api.settings.set(collectSettings());
  updateStatus();
  syncPathChips();
  if (notify) {
    addActivity("配置已保存", isTauriRuntime ? "设置已写入本地配置。" : "设置已保存到浏览器预览存储。", "success");
  }
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
    "image-output": state.activeMode === "image-dds"
  };

  for (const [id, visible] of Object.entries(fieldVisibility)) {
    const row = document.querySelector(`[data-field="${id}"]`);
    if (row) row.classList.toggle("hidden", !visible);
  }
}

function updateStatus() {
  const mode = state.activeMode;
  const runnableModes = ["merge", "split", "mipmap", "image-dds"];
  const blocker = getRunBlocker(mode);
  const ready = runnableModes.includes(mode) && !blocker;

  const outputPath = getModeOutputPath(mode);
  const meta = modeMeta[mode] || modeMeta.merge;

  setText("current-task", meta.title);
  setText("current-description", meta.description);
  setText("inspector-mode", meta.title.replace("多通道", ""));
  setText("runtime-badge", isTauriRuntime ? "Tauri Runtime" : "Browser Preview");
  setText("run-readiness", ready ? "已就绪" : "待配置");
  setText("run-hint", ready ? "配置完成，可开始运行" : blocker || "当前模式无需运行");

  $("run-readiness")?.classList.toggle("ready", ready);
  $("run-hint")?.classList.toggle("ready", ready);

  const runButtonByMode = {
    merge: "run-merge",
    split: "run-split",
    mipmap: "run-mipmap",
    "image-dds": "run-image-dds"
  };
  const activeRunButton = $(runButtonByMode[mode]);
  if (activeRunButton && activeRunButton.dataset.busy !== "true") {
    activeRunButton.disabled = !ready;
  }

  $("open-current-output")?.classList.toggle("hidden", !outputPath || !runnableModes.includes(mode));
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

function applyMode(mode) {
  closeCustomSelect();
  state.activeMode = mode;
  localStorage.setItem("aias-active-mode", mode);
  document.querySelectorAll(".mode-tab").forEach((button) => {
    button.classList.toggle("active", button.dataset.view === mode);
    button.setAttribute("aria-current", button.dataset.view === mode ? "page" : "false");
  });
  document.querySelectorAll(".mode-view").forEach((view) => {
    view.classList.toggle("active", view.id === `view-${mode}`);
  });
  // Settings & skins mode: hide inspector; expand to full width
  const isFull = mode === "settings" || mode === "skins";
  const workspace = document.querySelector(".workspace");
  if (workspace) workspace.classList.toggle("full-width", isFull);
  const inspector = document.querySelector(".inspector");
  if (inspector) inspector.classList.toggle("hidden", isFull);
  if (mode === "settings") syncSettingsView();
  syncActiveLog(mode);
  updateRunButtons(mode);
  updateInspector();
  updateStatus();
}

function bindTabs() {
  document.querySelectorAll(".mode-tab").forEach((button) => {
    button.title = button.textContent.trim();
    button.addEventListener("click", () => applyMode(button.dataset.view));
  });
}

function bindInspectorGroups() {
  document.querySelectorAll(".group-toggle").forEach((button) => {
    button.setAttribute("aria-expanded", "true");
    button.addEventListener("click", () => {
      const expanded = button.getAttribute("aria-expanded") !== "false";
      button.setAttribute("aria-expanded", String(!expanded));
    });
  });
}

function bindWorkspaceActions() {
  $("activity-toggle")?.addEventListener("click", () => {
    const open = $("activity-panel")?.classList.contains("collapsed") ?? true;
    setActivityPanel(open);
  });
  $("activity-close")?.addEventListener("click", () => setActivityPanel(false));
  $("clear-activity")?.addEventListener("click", () => {
    const feed = $("activity-feed");
    if (feed) {
      feed.innerHTML = '<article class="activity-item idle"><strong>暂无记录</strong><span>新的任务会显示在这里</span></article>';
    }
    document.querySelectorAll(".task-log").forEach((log) => { log.textContent = ""; });
    state.activityCount = 0;
    setText("activity-count", "0");
    setText("activity-summary", "暂无任务");
  });
  $("open-current-output")?.addEventListener("click", async () => {
    const outputPath = getModeOutputPath();
    if (!outputPath) return;
    try {
      await api.shell.openPath(outputPath);
    } catch (error) {
      addActivity("无法打开目录", error.message || String(error), "error");
    }
  });

  document.querySelectorAll(".inspector select, .inspector input[type='checkbox']").forEach((control) => {
    control.addEventListener("change", () => {
      updateStatus();
      saveSettings();
    });
  });

  document.addEventListener("keydown", (event) => {
    if (!(event.ctrlKey || event.metaKey) || event.key !== "Enter") return;
    const activeButton = document.querySelector(".run-button:not(.hidden)");
    if (!activeButton || activeButton.disabled) return;
    event.preventDefault();
    activeButton.click();
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
    if (button.tagName === "BUTTON") return;
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
  $("skin-auto-detect")?.addEventListener("click", async () => {
    const found = await api.skin.autoDetect();
    if (found) {
      $("skin-path").value = found;
      await saveSettings();
      await refreshSkins();
    } else {
      addActivity("未检测到目录", "未检测到 War Thunder UserSkins 目录。", "error");
    }
  });

  $("skin-dir-pick")?.addEventListener("click", async () => {
    const dir = await api.dialog.selectDirectory();
    if (!dir) return;
    $("skin-path").value = dir;
    await saveSettings();
    await refreshSkins();
  });

  $("skin-import-btn")?.addEventListener("click", async () => {
    const sources = await api.dialog.selectDirectories();
    if (!sources.length) return;
    const result = await api.skin.import({ sources, targetDirectory: $("skin-path").value });
    addActivity("导入完成", `已导入 ${result.imported} 个涂装`, "success");
    await refreshSkins();
  });

  $("skin-sort")?.addEventListener("change", () => {
    refreshSkins();
  });
}

async function refreshSkins({ notify = false } = {}) {
  const directory = $("skin-path")?.value;
  if (!directory) {
    renderSkinList([]);
    updateStatus();
    return;
  }

  try {
    const entries = await api.skin.list(directory);
    renderSkinList(entries);
    if (notify) addActivity("已刷新", `${entries.length} 个涂装`, "success");
  } catch (error) {
    $("skin-grid").innerHTML = "";
    addActivity("读取失败", error.message || String(error), "error");
  }
  updateStatus();
}

async function checkForUpdates(silent = true) {
  if (state.updateInProgress) return;

  try {
    const update = await check();
    if (!update) {
      if (!silent) addActivity("已是最新版本", "当前版本 " + (state.settings.version || "5.1.7"), "success");
      return;
    }
    $("update-button")?.classList.remove("hidden");
    addActivity("更新可用", update.version + " — 点击下载", "success");
    if (!silent) {
      const confirmed = await openPreviewConfirm("发现新版本", "版本 " + update.version + " 可用。\n\n是否立即下载并安装更新？");
      if (!confirmed) return;
      state.updateInProgress = true;
      $("update-button")?.setAttribute("disabled", "");
      addActivity("正在下载更新", update.version);
      let downloaded = 0;
      let total = 0;
      await update.downloadAndInstall((event) => {
        if (event.event === "Started") {
          total = Number(event.data.contentLength) || 0;
          $("runtime-badge").textContent = total ? "下载 0%" : "正在下载";
        } else if (event.event === "Progress") {
          downloaded += Number(event.data.chunkLength) || 0;
          $("runtime-badge").textContent = total
            ? "下载 " + Math.min(100, Math.round((downloaded / total) * 100)) + "%"
            : "已下载 " + formatSize(downloaded);
        } else if (event.event === "Finished") {
          $("runtime-badge").textContent = "正在验签并启动安装器";
          addActivity("下载完成", "正在验证更新并启动安装器");
        }
      });
      addActivity("更新安装器已启动", "应用将退出以完成更新", "success");
    }
  } catch (error) {
    if (!silent) {
      const message = error.message || String(error);
      $("runtime-badge").textContent = "更新失败";
      addActivity("更新失败", message, "error");
      setActivityPanel(true);
      await openPreviewMessage("更新失败", message);
    }
  } finally {
    state.updateInProgress = false;
    $("update-button")?.removeAttribute("disabled");
  }
}

function bindDragDrop() {
  if (!isTauriRuntime) return;

  const dropZones = document.querySelectorAll(".drop-zone");
  const highlight = (el, on) => el.classList.toggle("drag-over", on);

  dropZones.forEach((zone) => {
    zone.addEventListener("dragover", (e) => { e.preventDefault(); highlight(zone, true); });
    zone.addEventListener("dragleave", () => highlight(zone, false));
    zone.addEventListener("drop", (e) => { e.preventDefault(); highlight(zone, false); });
  });

  getCurrentWindow().onDragDropEvent((event) => {
    if (event.payload.type !== "drop") return;
    const paths = event.payload.paths;
    if (!paths.length) return;

    const mode = state.activeMode;
    switch (mode) {
      case "merge":
        $("pbr-input").value = paths[0];
        saveSettings().then(() => syncPathChips());
        break;
      case "mipmap":
        $("mipmap-input").value = paths[0];
        saveSettings().then(() => syncPathChips());
        break;
      case "skins":
        $("skin-path").value = paths[0];
        saveSettings().then(() => refreshSkins());
        break;
      case "split":
        state.splitFiles = [...new Set([...state.splitFiles, ...paths])];
        renderChips("split-file-list", state.splitFiles);
        updateStatus();
        break;
      case "image-dds":
        state.imageFiles = [...new Set([...state.imageFiles, ...paths])];
        renderChips("image-file-list", state.imageFiles);
        updateStatus();
        break;
    }
  });
}

async function init() {
  state.settings = await api.settings.get();
  refreshIcons();
  enhanceSelectMenus();
  applySettingsToForm();
  bindTabs();
  bindInspectorGroups();
  bindWorkspaceActions();
  bindDropZones();
  bindDragDrop();
  bindFileControls();
  bindRunActions();
  bindSkinActions();
  bindSettingsActions();
  if (isTauriRuntime) {
    await listen("task-progress", (event) => {
      if (!state.taskProgressActive) return;
      const progress = event.payload;
      setTaskProgress(progress.completed, progress.total, progress.message);
    });
  }
  $("update-button")?.addEventListener("click", () => checkForUpdates(false));

  renderChips("merge-chip-list", []);
  renderChips("split-file-list", []);
  renderChips("mipmap-chip-list", []);
  renderChips("image-file-list", []);
  await refreshSkins();
  syncPathChips();
  const savedMode = localStorage.getItem("aias-active-mode");
  applyMode(modeMeta[savedMode] ? savedMode : "merge");

  // Check for updates silently on startup
  if (isTauriRuntime) setTimeout(() => checkForUpdates(true), 2000);
}

init();
