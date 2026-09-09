import { createMaterialMaps } from "./material-maps.js";
let materialMapsUI;
import { invoke, convertFileSrc } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { openPath } from "@tauri-apps/plugin-opener";
import { check, Update } from "@tauri-apps/plugin-updater";
import { getVersion } from "@tauri-apps/api/app";
import { animateView, toggleGroup } from "./motion.js";
import { createUpdateController, scheduleUpdateCheck } from "./updater.mjs";
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
  Scissors,
  Settings,
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
  X,
  ImagePlus,
  ChevronsLeftRight,
  ImageOff,
  Check,
  AlertTriangle,
  Download,
  Maximize2,
  Wand2
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
  scaleTarget: "none",
  skinManagerPath: "",
  animeModel: "anime-specialist",
  animeHairRefiner: false,
  animeDetailRecovery: false,
  animeCutoutOutputPath: "",
  superresOutputPath: "",
  superresScale: "4"
};

const animeModelCatalog = {
  "anime-specialist": {
    label: "动漫专精（AnimeSeg）",
    notice: "该模型采用 DINOv3 相关许可；下载或分发时请一并遵守其条款与用途限制。"
  },
  toonout: { label: "动漫特化（ToonOut）" },
  "birefnet-general": { label: "高质量抠图（BiRefNet 1024）" },
  "birefnet-lite": { label: "轻量快速（BiRefNet Lite）" },
  simple: { label: "动漫标准（ISNet）" },
  advanced: { label: "动漫精细（RTMDet + 精修）" }
};

const modeMeta = {
  "normal-map": { title: "生成法线图", description: "从素材高度变化生成法线贴图，支持可移动光照预览" },
  "height-map": { title: "生成高度图", description: "从亮度或指定通道生成 8/16 位高度贴图" },
  merge: { title: "PBR 多通道合成", description: "生成游戏可用的 _c 与 _n 通道贴图" },
  split: { title: "PBR 多通道拆分", description: "提取 BaseColor、Alpha、材质与法线通道" },
  mipmap: { title: "Mipmap 生成", description: "将分层图片序列组装为单个 DDS" },
  "image-dds": { title: "图片转 DDS", description: "批量转换图片并统一 DDS 压缩格式" },
  "anime-cutout": { title: "AI 抠图", description: "动漫 / 人像 / 商品等任意图片智能抠图，输出透明背景 PNG" },
  "superres-anime": { title: "动漫超分", description: "RealESRGAN 动漫特化模型，立绘插画放大 4 倍" },
  "superres-general": { title: "通用超分", description: "RealESRGAN 通用模型，照片素材放大 4 倍" },
  skins: { title: "涂装管理", description: "管理 War Thunder UserSkins 资源" },
  settings: { title: "应用设置", description: "更新、数据路径与版本信息" }
};

const state = {
  settings: {},
  splitFiles: [],
  imageFiles: [],
  animeFiles: [],
  animeModels: [],
  animeDownloading: false,
  animeHairStatus: null,
  animeHairDownloading: false,
  animeRunning: false,
  gpuRuntime: null,
  gpuDownloading: false,
  animeResults: new Map(),
  animeProbed: new Set(),
  animeResultEpoch: 0,
  animeActiveIndex: 0,
  animeComparePos: 50,
  superresFiles: [],
  superresModels: [],
  superresDownloadingId: "",
  superresRunning: false,
  superresResults: new Map(),
  superresProbed: new Set(),
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
  Scissors,
  Settings,
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
  X,
  ImagePlus,
  ChevronsLeftRight,
  ImageOff,
  Check,
  AlertTriangle,
  Download,
  Maximize2,
  Wand2
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
    "anime-cutout": "anime-output",
    "superres-anime": "superres-output",
    "superres-general": "superres-output",
    "normal-map": "map-output",
    "height-map": "map-output",
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
  button.disabled = select.disabled;
}

function openCustomSelect(select, wrapper, button) {
  if (select.disabled) return;
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
      if (select.disabled) { closeCustomSelect(); return; }
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

function openPreviewConfirm(title, body, options = {}) {
  const { confirmText = "确定", danger = false } = options;
  return new Promise((resolve) => {
    const overlay = document.createElement("div");
    overlay.className = "preview-picker-backdrop";

    const dialog = document.createElement("div");
    dialog.className = "preview-picker";
    if (danger) dialog.classList.add("danger");

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
    confirm.textContent = confirmText;
    if (danger) confirm.className = "danger-action";

    const { close, addCleanup } = createPreviewDialogCloser(overlay, resolve, false);

    if (danger) {
      const glyph = document.createElement("span");
      glyph.className = "confirm-glyph";
      glyph.innerHTML = '<i data-lucide="alert-triangle"></i>';
      dialog.append(glyph);
    }

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
    if (danger) refreshIcons(dialog);
    confirm.focus();
  });
}

function createBrowserPreviewApi() {
  let settings = {
    ...defaults,
    ...JSON.parse(localStorage.getItem("aias-preview-settings") || "{}")
  };

  const previewModels = [
    { id: "anime-specialist", label: "动漫专精（AnimeSeg）", totalSize: 117239813, files: [{ name: "birefnext-aniseg-int8-v0.1.onnx", expectedSize: 117239813 }] },
    { id: "toonout", label: "动漫特化（ToonOut）", totalSize: 492381880, files: [{ name: "birefnet-toonout-fp16.onnx", expectedSize: 492381880 }] },
    { id: "birefnet-general", label: "高质量抠图（BiRefNet 1024）", totalSize: 489666272, files: [{ name: "birefnet-general-1024-fp16.onnx", expectedSize: 489666272 }] },
    { id: "birefnet-lite", label: "轻量快速（BiRefNet Lite）", totalSize: 114538787, files: [{ name: "birefnet-lite-fp16.onnx", expectedSize: 114538787 }] },
    { id: "simple", label: "动漫标准（ISNet）", totalSize: 176069933, files: [{ name: "isnetis.onnx", expectedSize: 176069933 }] },
    { id: "advanced", label: "动漫精细（RTMDet + 精修）", totalSize: 414883269, files: [{ name: "anime_segmentor_rtmdet_e60_simplified.onnx", expectedSize: 238686077 }, { name: "mask_refiner_isnetdis_refine_last_simplified.onnx", expectedSize: 176197192 }] }
  ].map((model) => ({
    ...model,
    installed: true,
    files: model.files.map((file) => ({ ...file, present: true, size: file.expectedSize }))
  }));

  const modelsStatus = () => previewModels.map((model) => ({
    ...model,
    files: model.files.map((file) => ({ ...file }))
  }));

  const setModelInstalled = (modelId, installed) => {
    const model = previewModels.find((item) => item.id === modelId);
    if (!model) return modelsStatus();
    model.installed = installed;
    model.files = model.files.map((file) => ({
      ...file,
      present: installed,
      size: installed ? file.expectedSize : 0
    }));
    return modelsStatus();
  };

  const superresPreviewModels = [
    { id: "anime", label: "动漫超分（RealESRGAN 动漫 4x）", installed: true, totalSize: 18352469 },
    { id: "general", label: "通用超分（RealESRGAN 通用 4x）", installed: false, totalSize: 67051616 }
  ];

  const setSuperresInstalled = (model, installed) => {
    const target = superresPreviewModels.find((item) => item.id === model);
    if (target) target.installed = installed;
    return superresPreviewModels.map((item) => ({ ...item }));
  };

  const save = () => {
    localStorage.setItem("aias-preview-settings", JSON.stringify(settings));
    return { ...settings };
  };

  const previewOnly = async (feature) => ({
    completed: 1,
    total: 1,
    logs: [
      `${feature} 预览任务已完成。`,
      "已模拟桌面端的运行状态与完成反馈；未写入本地文件。"
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
    anime: {
      modelsStatus: async () => modelsStatus(),
      hairRefinerStatus: async () => null,
      hairRefinerDownload: async () => { throw new Error("网页预览不能下载或运行模型，请在桌面软件中操作。"); },
      hairRefinerUninstall: async () => { throw new Error("请在桌面软件中管理模型。"); },
      modelDownload: async (modelId) => setModelInstalled(modelId, true),
      modelUninstall: async (modelId) => setModelInstalled(modelId, false),
      cutout: () => previewOnly("AI 抠图")
    },
    superres: {
      modelsStatus: async () => superresPreviewModels.map((model) => ({ ...model })),
      modelDownload: async (model) => setSuperresInstalled(model, true),
      modelUninstall: async (model) => setSuperresInstalled(model, false),
      run: () => previewOnly("图片超分")
    },
    skin: {
      autoDetect: async () => null,
      list: async () => [],
      import: async () => ({ imported: 0, errors: [] }),
      toggle: async (filePath) => ({ path: filePath }),
      delete: async () => ({ deleted: true })
    },
    system: {
      stats: async () => {
        const total = 32 * 1024 ** 3;
        return { cpuUsage: 31, memoryUsed: Math.round(total * 0.48), memoryTotal: total };
      }
    },
    gpu: {
      stats: async () => {
        const total = 12 * 1024 ** 3;
        return {
          available: true,
          name: "NVIDIA GeForce RTX 4070 SUPER",
          utilization: 27,
          memoryUsed: Math.round(total * 0.36),
          memoryTotal: total
        };
      }
    },
    shell: {
      openPath: async (filePath) => addActivity("已打开输出目录", filePath, "success")
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
    anime: {
      modelsStatus: () => invoke("anime_models_status"),
      hairRefinerStatus: () => invoke("anime_hair_refiner_status"),
      hairRefinerDownload: () => invoke("anime_hair_refiner_download"),
      hairRefinerUninstall: () => invoke("anime_hair_refiner_uninstall"),
      modelDownload: (modelId) => invoke("anime_model_download", { modelId }),
      modelUninstall: (modelId) => invoke("anime_model_uninstall", { modelId }),
      cutout: (options) => invoke("anime_cutout", { options }),
      gpuRuntimeState: () => invoke("gpu_runtime_state"),
      installGpuRuntime: () => invoke("install_gpu_runtime")
    },
    superres: {
      modelsStatus: () => invoke("superres_models_status"),
      modelDownload: (model) => invoke("superres_model_download", { model }),
      modelUninstall: (model) => invoke("superres_model_uninstall", { model }),
      run: (options) => invoke("superres_run", { options })
    },
    skin: {
      autoDetect: () => invoke("skin_auto_detect"),
      list: (directory) => invoke("skin_list", { directory }),
      import: (options) => invoke("skin_import", { options }),
      toggle: (filePath) => invoke("skin_toggle", { filePath }),
      delete: (filePath) => invoke("skin_delete", { filePath })
    },
    system: {
      stats: () => invoke("system_stats")
    },
    gpu: {
      stats: () => invoke("gpu_stats")
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

function formatBytes(bytes) {
  const value = Number(bytes) || 0;
  if (value >= 1024 ** 3) return `${(value / 1024 ** 3).toFixed(2)} GB`;
  if (value >= 1024 ** 2) return `${(value / 1024 ** 2).toFixed(1)} MB`;
  if (value >= 1024) return `${(value / 1024).toFixed(0)} KB`;
  return `${value} B`;
}

const MONITOR_VALUE_ANIM_MS = 700;

function setMonitorDonut(donutId, valueId, percent, labelOverride) {
  const donut = $(donutId);
  if (!donut) return;
  const clamped = Math.min(100, Math.max(0, Math.round(percent)));
  donut.style.setProperty("--usage", `${clamped}%`);
  const label = $(valueId);
  if (!label) return;
  if (labelOverride !== undefined) {
    label.textContent = labelOverride;
    return;
  }
  // 数字与 CSS 弧线过渡同步缓动。
  const from = Number(label.dataset.usage || "0");
  label.dataset.usage = String(clamped);
  const startedAt = performance.now();
  const step = () => {
    const t = Math.min(1, (performance.now() - startedAt) / MONITOR_VALUE_ANIM_MS);
    const eased = 1 - (1 - t) ** 3;
    label.textContent = `${Math.round(from + (clamped - from) * eased)}%`;
    if (t < 1) requestAnimationFrame(step);
  };
  requestAnimationFrame(step);
}

const monitorDetails = { memory: "", gpu: "" };

function updateMonitorTitle() {
  const text = [monitorDetails.memory, monitorDetails.gpu].filter(Boolean).join("\n");
  $("system-monitor")?.setAttribute("title", text || "系统资源读取中…");
}

function renderSystemStats(stats) {
  if (!stats) return;
  setMonitorDonut("monitor-cpu", "monitor-cpu-value", Number(stats.cpuUsage) || 0);
  const total = Number(stats.memoryTotal) || 0;
  const used = Number(stats.memoryUsed) || 0;
  setMonitorDonut("monitor-memory", "monitor-memory-value", total > 0 ? (used / total) * 100 : 0);
  monitorDetails.memory = total > 0 ? `内存：已用 ${formatBytes(used)} / 共 ${formatBytes(total)}` : "内存：读取中…";
  updateMonitorTitle();
}

function renderGpuStats(stats) {
  if (!stats?.available) {
    setMonitorDonut("monitor-gpu", "monitor-gpu-value", 0, "--");
    setMonitorDonut("monitor-vram", "monitor-vram-value", 0, "--");
    monitorDetails.gpu = "未检测到 GPU（需要 NVIDIA 驱动）";
    updateMonitorTitle();
    return;
  }
  setMonitorDonut("monitor-gpu", "monitor-gpu-value", Number(stats.utilization) || 0);
  const total = Number(stats.memoryTotal) || 0;
  const used = Number(stats.memoryUsed) || 0;
  setMonitorDonut("monitor-vram", "monitor-vram-value", total > 0 ? (used / total) * 100 : 0);
  monitorDetails.gpu =
    total > 0 ? `${stats.name} · 显存已用 ${formatBytes(used)} / 共 ${formatBytes(total)}` : stats.name;
  updateMonitorTitle();
}

function startSystemMonitor() {
  const poll = async () => {
    try {
      renderSystemStats(await api.system.stats());
    } catch {
      // 轮询失败静默跳过，下一轮继续
    }
    try {
      renderGpuStats(await api.gpu.stats());
    } catch {
      renderGpuStats(null);
    }
  };
  poll();
  window.setInterval(poll, 2000);
}

function animeModelById(id) {
  return state.animeModels.find((model) => model.id === id) || null;
}

function isAnimeModelReady(id) {
  const model = animeModelById(id);
  return Boolean(model?.installed);
}

function describeAnimeModel(model) {
  if (!model) return "状态未知";
  const size = formatBytes(model.totalSize);
  const notice = animeModelCatalog[model.id]?.notice;
  const suffix = notice ? ` · ${notice}` : "";
  if (model.installed) return `已安装 · ${size}${suffix}`;
  const missing = model.files.filter((file) => !file.present || file.size !== file.expectedSize);
  if (!model.files.some((file) => file.present)) return `未安装 · 共 ${size}${suffix}`;
  return `未完整 · 缺 ${formatBytes(missing.reduce((sum, file) => sum + Math.max(0, file.expectedSize - file.size), 0))}${suffix}`;
}

async function refreshAnimeModelStatus() {
  try {
    state.animeModels = await api.anime.modelsStatus();
    state.animeHairStatus = await api.anime.hairRefinerStatus();
  } catch (error) {
    state.animeModels = [];
    setText("anime-model-status", `无法读取模型状态：${error.message || error}`);
    return;
  }
  renderAnimeModelStatus();
}

function describeGpuRuntime() {
  const runtime = state.gpuRuntime;
  if (!runtime?.nvidiaGpu) return { relevant: false };
  if (runtime.cudaActive) {
    return { relevant: true, text: "GPU 加速已生效（CUDA），抠图推理运行在显卡上。" };
  }
  if (runtime.runtimeInstalled) {
    return {
      relevant: true,
      text: runtime.ortInitialized
        ? "GPU 运行库已就绪，但本次会话已先加载了 CPU 运行库，重启应用后生效。"
        : "GPU 运行库已就绪，开始抠图后自动启用（CUDA）。"
    };
  }
  return {
    relevant: true,
    text: "检测到 NVIDIA 显卡。下载 GPU 运行库（约 233 MB，一次性）可大幅提升抠图速度。"
  };
}

async function refreshGpuRuntime() {
  if (!isTauriRuntime) {
    renderGpuRuntime();
    return;
  }
  try {
    state.gpuRuntime = await api.anime.gpuRuntimeState();
  } catch {
    state.gpuRuntime = null;
  }
  renderGpuRuntime();
}

function renderGpuRuntime() {
  const section = $("anime-gpu-section");
  if (!section) return;
  const info = describeGpuRuntime();
  section.classList.toggle("hidden", state.activeMode !== "anime-cutout" || !info.relevant);
  if (!info.relevant) return;
  setText("anime-gpu-status", info.text);
  const installing = state.gpuDownloading;
  $("anime-gpu-install")?.classList.toggle(
    "hidden",
    installing || Boolean(state.gpuRuntime?.runtimeInstalled)
  );
  $("anime-gpu-progress")?.classList.toggle("hidden", !installing);
}

async function installGpuRuntime() {
  if (!isTauriRuntime || state.gpuDownloading) return;
  state.gpuDownloading = true;
  renderGpuRuntime();
  addActivity("开始下载 GPU 运行库", "onnxruntime-gpu");
  try {
    const result = await api.anime.installGpuRuntime();
    addActivity(
      "GPU 运行库下载完成",
      result?.requiresRestart ? "重启应用后生效" : "立即生效",
      "success"
    );
  } catch (error) {
    addActivity("GPU 运行库下载失败", error.message || String(error), "error");
  }
  state.gpuDownloading = false;
  await refreshGpuRuntime();
}

function renderAnimeModelStatus() {
  const model = animeModelById($("anime-model")?.value || "anime-specialist");
  setText("anime-model-status", isTauriRuntime ? describeAnimeModel(model) : `网页演示状态：${describeAnimeModel(model)}（实际安装状态请在桌面软件中查看）`);
  const ready = Boolean(model?.installed);
  const downloading = state.animeDownloading;
  const busy = downloading || state.animeHairDownloading || state.animeRunning;
  for (const id of ["anime-model", "anime-hair-refiner", "anime-detail-recovery", "anime-model-download", "anime-model-uninstall", "anime-hair-download", "anime-hair-uninstall"]) {
    if ($(id)) $(id).disabled = busy;
  }
  if ($("anime-hair-refiner")) $("anime-hair-refiner").disabled = busy || model?.id !== "anime-specialist";
  if ($("anime-detail-recovery")) $("anime-detail-recovery").disabled = busy || model?.id !== "anime-specialist";
  if ($("anime-model")) syncCustomSelect($("anime-model"));
  setText("anime-hair-status", isTauriRuntime ? describeAnimeModel(state.animeHairStatus) : "网页仅预览界面；下载和推理请在桌面软件中操作");
  $("anime-hair-download")?.classList.toggle("hidden", Boolean(state.animeHairStatus?.installed) || state.animeHairDownloading);
  $("anime-hair-uninstall")?.classList.toggle("hidden", !state.animeHairStatus?.installed || state.animeHairDownloading);
  $("anime-hair-progress")?.classList.toggle("hidden", !state.animeHairDownloading);
  $("anime-model-download")?.classList.toggle("hidden", ready || downloading);
  $("anime-model-uninstall")?.classList.toggle("hidden", !ready || downloading);
  $("anime-model-progress")?.classList.toggle("hidden", !downloading);
  const runButton = $("run-anime-cutout");
  if (runButton) runButton.disabled = busy;
  if (downloading) {
    $("anime-model-progress")?.classList.remove("hidden");
  } else {
    $("anime-model-progress-fill")?.style.setProperty("width", "0%");
    setText("anime-model-progress-text", "");
  }
  updateStatus();
}

async function downloadAnimeModel() {
  const modelId = $("anime-model")?.value || "anime-specialist";
  if (state.animeDownloading || state.animeHairDownloading || state.animeRunning) return;
  state.animeDownloading = true;
  renderAnimeModelStatus();
  addActivity("开始下载模型", animeModelCatalog[modelId]?.label || modelId);
  try {
    state.animeModels = await api.anime.modelDownload(modelId);
    addActivity("模型下载完成", animeModelCatalog[modelId]?.label || modelId, "success");
  } catch (error) {
    addActivity("模型下载失败", error.message || String(error), "error");
  }
  state.animeDownloading = false;
  renderAnimeModelStatus();
}

async function uninstallAnimeModel() {
  const modelId = $("anime-model")?.value || "anime-specialist";
  if (state.animeDownloading || state.animeHairDownloading || state.animeRunning) return;
  const model = animeModelById(modelId);
  const label = model?.label || animeModelCatalog[modelId]?.label || modelId;
  const confirmed = await openPreviewConfirm(
    "卸载模型",
    `即将删除「${label}」的本地模型文件（共 ${formatBytes(model?.totalSize || 0)}）。卸载后需要重新下载才能使用该模型，确定继续吗？`,
    { confirmText: "卸载", danger: true }
  );
  if (!confirmed || state.animeRunning || state.animeDownloading || state.animeHairDownloading) return;
  state.animeDownloading = true;
  renderAnimeModelStatus();
  try {
    state.animeModels = await api.anime.modelUninstall(modelId);
    addActivity("模型已卸载", animeModelCatalog[modelId]?.label || modelId, "success");
  } catch (error) {
    addActivity("模型卸载失败", error.message || String(error), "error");
  }
  state.animeDownloading = false;
  renderAnimeModelStatus();
}

// ---------------------------------------------------------------- 动漫抠图画廊

function wantsHairRefiner() {
  return $("anime-model")?.value === "anime-specialist" && Boolean($("anime-hair-refiner")?.checked);
}

function wantsDetailRecovery() {
  return $("anime-model")?.value === "anime-specialist" && Boolean($("anime-detail-recovery")?.checked);
}

async function manageHairRefiner(uninstall = false) {
  if (state.animeDownloading || state.animeHairDownloading || state.animeRunning) return;
  if (uninstall && !await openPreviewConfirm("卸载边缘模型", "仅删除 ViTMatte 模型，保留抠图模型及图片。确定卸载吗？", { confirmText: "卸载", danger: true })) return;
  if (state.animeDownloading || state.animeHairDownloading || state.animeRunning) return;
  state.animeHairDownloading = true;
  renderAnimeModelStatus();
  try {
    state.animeHairStatus = await (uninstall ? api.anime.hairRefinerUninstall() : api.anime.hairRefinerDownload());
    addActivity(uninstall ? "边缘模型已卸载" : "边缘模型下载完成", "ViTMatte", "success");
  } catch (error) {
    addActivity("边缘模型操作失败", error.message || String(error), "error");
  } finally {
    state.animeHairDownloading = false;
    renderAnimeModelStatus();
  }
}

function animeStem(file) {
  const name = basename(file);
  const dot = name.lastIndexOf(".");
  return dot > 0 ? name.slice(0, dot) : name;
}

// 后端输出名为 {原图stem}_{模型id}.png，这里用同样的组合键匹配结果，避免与原图同名覆盖
function animeResultKey(file) {
  const suffix = wantsDetailRecovery() && wantsHairRefiner()
    ? "_detail-hair"
    : wantsDetailRecovery()
      ? "_detail"
      : wantsHairRefiner()
        ? "_hair"
        : "";
  return `${animeStem(file)}_${$("anime-model")?.value || "anime-specialist"}${suffix}`;
}

function animeLocalSrc(path) {
  return isTauriRuntime && path ? convertFileSrc(path) : "";
}

function applyAnimeOutputs(paths, requestKeys = null) {
  // 输出文件路径固定不变，重跑后内容已更新；换时间戳强制 <img> 重新加载
  state.animeResultEpoch = Date.now();
  for (const path of paths || []) {
    // 后端返回的实际输出路径（ToonOut 在复杂背景上可能回退到 _advanced 或 _simple）。
    // 用「原图 stem」匹配前端当前选中的文件，把结果挂到 animeResultKey(file) 下，
    // 这样渲染/角标/对比图用同一个 key 就能查到，与后端实际模型无关。
    const pathStem = animeStem(path); // 例如 73307539_p0_simple
    const inputStem = pathStem.replace(/_(?:anime-specialist|toonout|birefnet-general|birefnet-lite|advanced|simple)(?:_detail-hair|_detail|_hair)?$/, "");
    const hit = (state.animeFiles || []).find((file) => animeStem(file) === inputStem);
    if (hit) {
      state.animeResults.set(requestKeys?.get(hit) || animeResultKey(hit), path);
    } else {
      // 匹配不到前端文件时退化为按路径 stem 存（保持原行为）。
      state.animeResults.set(pathStem, path);
    }
  }
  renderAnimeGallery();
}

function resetAnimeResults() {
  state.animeResults.clear();
  state.animeProbed.clear();
  state.animeResultEpoch = Date.now();
}

function probeAnimeResult(file) {
  const key = animeResultKey(file);
  if (state.animeResults.has(key) || state.animeProbed.has(key)) return;
  const dir = $("anime-output")?.value?.trim();
  if (!dir || !isTauriRuntime) return;
  state.animeProbed.add(key);
  const dirTrimmed = dir.replace(/[\\/]+$/, "");
  const stem = animeStem(file);
  // ToonOut 在复杂背景上可能被后端自动回退到 General、advanced 或 simple。
  // 主候选（用户选择的模型）失败时，回退候选也要探测，否则回退结果在刷新后丢失角标/预览。
  const model = $("anime-model")?.value || "anime-specialist";
  const candidates = [`${key}.png`];
  if (model === "toonout") {
    candidates.push(
      `${stem}_anime-specialist.png`,
      `${stem}_birefnet-general.png`,
      `${stem}_advanced.png`,
      `${stem}_simple.png`
    );
  }
  const probeNext = (index) => {
    if (index >= candidates.length) return;
    const candidate = `${dirTrimmed}/${candidates[index]}`;
    const probe = new window.Image();
    probe.onload = () => {
      if ($("anime-output")?.value?.trim() !== dir || !state.animeFiles.includes(file) || state.animeResults.has(key)) return;
      state.animeResults.set(key, candidate);
      renderAnimeGallery();
    };
    probe.onerror = () => probeNext(index + 1);
    probe.src = animeLocalSrc(candidate);
  };
  probeNext(0);
}

function renderAnimeGallery() {
  const files = state.animeFiles;
  if (state.animeActiveIndex >= files.length) state.animeActiveIndex = Math.max(0, files.length - 1);
  const hasFiles = files.length > 0;
  $("anime-empty")?.classList.toggle("hidden", hasFiles);
  $("anime-gallery")?.classList.toggle("hidden", !hasFiles);
  setText("anime-toolbar-count", hasFiles ? `${files.length} 张图片` : "尚未添加图片");
  updateStatus();
  if (!hasFiles) return;
  renderAnimeThumbs(files);
  renderAnimeCompare(files[state.animeActiveIndex]);
  files.forEach((file) => probeAnimeResult(file));
}

function renderAnimeThumbs(files) {
  const strip = $("anime-thumbs");
  if (!strip) return;
  strip.innerHTML = "";
  files.forEach((file, index) => {
    const thumb = document.createElement("button");
    thumb.type = "button";
    thumb.className = "anime-thumb";
    thumb.classList.toggle("active", index === state.animeActiveIndex);
    thumb.title = file;

    const img = document.createElement("img");
    img.alt = basename(file);
    img.draggable = false;
    const src = animeLocalSrc(file);
    if (src) {
      img.src = src;
    } else {
      thumb.classList.add("placeholder");
    }

    const done = state.animeResults.has(animeResultKey(file));
    const badge = document.createElement("span");
    badge.className = `thumb-badge ${done ? "done" : "pending"}`;
    badge.title = done ? "已有抠图结果" : "待处理";
    badge.innerHTML = done ? '<i data-lucide="check"></i>' : "";

    const remove = document.createElement("span");
    remove.className = "thumb-remove";
    remove.title = `移除 ${basename(file)}`;
    remove.setAttribute("aria-label", remove.title);
    remove.innerHTML = '<i data-lucide="x"></i>';
    remove.addEventListener("click", (event) => {
      event.stopPropagation();
      removeAnimeFile(file);
    });

    thumb.addEventListener("click", () => {
      state.animeActiveIndex = index;
      renderAnimeGallery();
    });

    thumb.append(img, badge, remove);
    strip.appendChild(thumb);
  });
  refreshIcons(strip);
}

function removeAnimeFile(file) {
  state.animeFiles = state.animeFiles.filter((item) => item !== file);
  state.animeResults.delete(animeResultKey(file));
  renderAnimeGallery();
  updateStatus();
}

function renderAnimeCompare(file) {
  const stage = $("anime-compare");
  if (!stage || !file) return;
  const result = state.animeResults.get(animeResultKey(file)) || "";
  const hasResult = Boolean(result);
  const hasLocal = Boolean(animeLocalSrc(file));
  stage.classList.toggle("no-result", !hasResult);
  stage.classList.toggle("no-file", !hasLocal);

  const original = $("anime-compare-original");
  const after = $("anime-compare-after");
  const originalSrc = animeLocalSrc(file);
  const rawResultSrc = animeLocalSrc(result);
  const resultSrc = rawResultSrc && state.animeResultEpoch
    ? `${rawResultSrc}?v=${state.animeResultEpoch}`
    : rawResultSrc;
  if (original) {
    if (originalSrc) original.src = originalSrc;
    else original.removeAttribute("src");
  }
  if (after) {
    if (resultSrc) after.src = resultSrc;
    else after.removeAttribute("src");
  }
  if (original) original.onload = fitAnimeCompareFrame;

  if (hasResult) {
    stage.style.setProperty("--compare-pos", `${state.animeComparePos}%`);
  } else {
    stage.style.setProperty("--compare-pos", "100%");
  }
  const hint = $("anime-compare-hint");
  if (hint) hint.hidden = hasResult || !hasLocal;
  const empty = $("anime-compare-empty");
  if (empty) empty.hidden = hasLocal;
  $("anime-compare-divider")?.setAttribute("aria-valuenow", String(Math.round(hasResult ? state.animeComparePos : 100)));
  fitAnimeCompareFrame();
}

// 对比框占满整个舞台，图片用 object-fit:contain 居中，不再按宽高比收窄
// （那会让肖像图变成窄条、两侧留下大片空白）。此函数仅清理历史遗留的
// 内联宽高，确保 CSS 铺满生效。
function fitAnimeCompareFrame() {
  const frame = $("anime-compare-frame");
  if (!frame) return;
  frame.classList.add("aspect-fit");
  frame.style.width = "";
  frame.style.height = "";
}

function setAnimeComparePosition(percent) {
  const stage = $("anime-compare");
  if (!stage) return;
  state.animeComparePos = Math.min(100, Math.max(0, percent));
  stage.style.setProperty("--compare-pos", `${state.animeComparePos}%`);
  $("anime-compare-divider")?.setAttribute("aria-valuenow", String(Math.round(state.animeComparePos)));
}

function bindAnimeGallery() {
  const stage = $("anime-compare");
  const divider = $("anime-compare-divider");
  if (!stage || !divider) return;

  const positionFromEvent = (event) => {
    const rect = stage.getBoundingClientRect();
    if (!rect.width) return state.animeComparePos;
    return ((event.clientX - rect.left) / rect.width) * 100;
  };

  stage.addEventListener("pointerdown", (event) => {
    if (stage.classList.contains("no-result") || stage.classList.contains("no-file")) return;
    event.preventDefault();
    stage.setPointerCapture(event.pointerId);
    setAnimeComparePosition(positionFromEvent(event));
  });
  stage.addEventListener("pointermove", (event) => {
    if (!stage.hasPointerCapture(event.pointerId)) return;
    setAnimeComparePosition(positionFromEvent(event));
  });
  const release = (event) => {
    if (stage.hasPointerCapture(event.pointerId)) stage.releasePointerCapture(event.pointerId);
  };
  stage.addEventListener("pointerup", release);
  stage.addEventListener("pointercancel", release);

  divider.addEventListener("keydown", (event) => {
    if (stage.classList.contains("no-result") || stage.classList.contains("no-file")) return;
    const step = event.shiftKey ? 10 : 2;
    if (event.key === "ArrowLeft") setAnimeComparePosition(state.animeComparePos - step);
    else if (event.key === "ArrowRight") setAnimeComparePosition(state.animeComparePos + step);
    else if (event.key === "Home") setAnimeComparePosition(0);
    else if (event.key === "End") setAnimeComparePosition(100);
    else return;
    event.preventDefault();
  });

  window.addEventListener("resize", fitAnimeCompareFrame);
}

// ---------------------------------------------------------------- 图片超分

const superresLabels = {
  anime: "动漫超分",
  general: "通用超分"
};

const SUPERRES_SCALE_MIN = 2;
const SUPERRES_SCALE_MAX = 4;

function superresScale() {
  const value = Number($("superres-scale")?.value);
  if (!Number.isFinite(value)) return 4;
  return Math.min(SUPERRES_SCALE_MAX, Math.max(SUPERRES_SCALE_MIN, Math.round(value)));
}

function updateSuperresScaleControl() {
  const scale = superresScale();
  const slider = $("superres-scale");
  if (slider && Number(slider.value) !== scale) slider.value = String(scale);
  // 自绘圆点位置（0–1），由 CSS 对 left 做过渡实现滑动动画
  $("superres-scale-slider")?.style?.setProperty?.(
    "--pos",
    String((scale - SUPERRES_SCALE_MIN) / (SUPERRES_SCALE_MAX - SUPERRES_SCALE_MIN))
  );
  const label = `放大 ${scale} 倍`;
  const textEl = $("superres-scale-text");
  if (textEl && textEl.textContent !== label) {
    textEl.textContent = label;
    // 重触发数值文字的弹跳动画（真实 DOM 需先移除类再强制回流）
    textEl.classList?.remove("scale-pop");
    void textEl.offsetWidth;
    textEl.classList?.add("scale-pop");
  }
}

function superresResultKey(file, modelId, scale = superresScale()) {
  // 后端输出名固定为 {stem}_{倍率}x_{模型id}.png
  return `${modelId}\u0000${scale}\u0000${file}`;
}

function superresLocalSrc(path) {
  return isTauriRuntime && path ? convertFileSrc(path) : "";
}

function describeSuperresModel(model) {
  if (!model) return "状态未知";
  const size = formatBytes(model.totalSize);
  if (model.installed) return `已安装 · ${size}`;
  return `未安装 · 共 ${size}`;
}

function isSuperresModelReady(modelId) {
  return Boolean(state.superresModels.find((model) => model.id === modelId)?.installed);
}

async function refreshSuperresModelStatus() {
  try {
    state.superresModels = await api.superres.modelsStatus();
  } catch (error) {
    state.superresModels = [];
    setText("superres-anime-status", `无法读取模型状态：${error.message || error}`);
    setText("superres-general-status", "");
    return;
  }
  renderSuperresModelStatus();
}

function renderSuperresModelStatus() {
  const busy = Boolean(state.superresDownloadingId) || state.superresRunning;
  for (const modelId of ["anime", "general"]) {
    const model = state.superresModels.find((item) => item.id === modelId) || null;
    const ready = Boolean(model?.installed);
    const downloading = state.superresDownloadingId === modelId;
    setText(`superres-${modelId}-status`, describeSuperresModel(model));
    const downloadButton = $(`superres-${modelId}-download`);
    const uninstallButton = $(`superres-${modelId}-uninstall`);
    if (downloadButton) {
      downloadButton.disabled = busy;
      downloadButton.classList.toggle("hidden", ready || Boolean(state.superresDownloadingId));
    }
    if (uninstallButton) {
      uninstallButton.disabled = busy;
      uninstallButton.classList.toggle("hidden", !ready || Boolean(state.superresDownloadingId));
    }
    $(`superres-${modelId}-progress`)?.classList.toggle("hidden", !downloading);
    if (!downloading) {
      $(`superres-${modelId}-progress-fill`)?.style.setProperty("width", "0%");
      setText(`superres-${modelId}-progress-text`, "");
    }
  }
  updateStatus();
}

async function downloadSuperresModel(modelId) {
  if (state.superresDownloadingId || state.superresRunning) return;
  state.superresDownloadingId = modelId;
  renderSuperresModelStatus();
  addActivity("开始下载模型", superresLabels[modelId] || modelId);
  try {
    state.superresModels = await api.superres.modelDownload(modelId);
    addActivity("模型下载完成", superresLabels[modelId] || modelId, "success");
  } catch (error) {
    addActivity("模型下载失败", error.message || String(error), "error");
  }
  state.superresDownloadingId = "";
  renderSuperresModelStatus();
}

async function uninstallSuperresModel(modelId) {
  if (state.superresDownloadingId || state.superresRunning) return;
  const confirmed = await openPreviewConfirm(
    "卸载模型",
    `即将删除「${superresLabels[modelId] || modelId}」的本地模型文件。卸载后需要重新下载才能使用，确定继续吗？`,
    { confirmText: "卸载", danger: true }
  );
  if (!confirmed || state.superresDownloadingId || state.superresRunning) return;
  state.superresDownloadingId = modelId;
  renderSuperresModelStatus();
  try {
    state.superresModels = await api.superres.modelUninstall(modelId);
    addActivity("模型已卸载", superresLabels[modelId] || modelId, "success");
  } catch (error) {
    addActivity("模型卸载失败", error.message || String(error), "error");
  }
  state.superresDownloadingId = "";
  renderSuperresModelStatus();
}

function applySuperresOutputs(paths, requestKeys) {
  for (const path of paths || []) {
    // 输出名固定为 {stem}_{倍率}x_{模型id}.png，按后缀还原倍率与模型 id
    const name = basename(path);
    const match = name.match(/^(.*)_([2-4])x_(anime|general)\.png$/i);
    if (!match) continue;
    const stem = match[1];
    const scale = Number(match[2]);
    const modelId = match[3].toLowerCase();
    const hit = (state.superresFiles || []).find((file) => animeStem(file) === stem);
    if (!hit) continue;
    // 运行开始时捕获的键优先（运行期间用户可能已经改了倍率）。
    state.superresResults.set(requestKeys?.get(hit) || superresResultKey(hit, modelId, scale), path);
  }
  renderSuperresGallery();
}

function resetSuperresResults() {
  state.superresPreviewRevision = (state.superresPreviewRevision || 0) + 1;
  state.superresResults.clear();
  state.superresProbed.clear();
}

function probeSuperresResult(file, modelId) {
  const key = superresResultKey(file, modelId);
  if (state.superresResults.has(key) || state.superresProbed.has(key)) return;
  const dir = $("superres-output")?.value?.trim();
  if (!dir || !isTauriRuntime) return;
  state.superresProbed.add(key);
  const revision = state.superresPreviewRevision || 0;
  const candidate = `${dir.replace(/[\\/]+$/, "")}/${animeStem(file)}_${superresScale()}x_${modelId}.png`;
  const probe = new window.Image();
  probe.onload = () => {
    if ((state.superresPreviewRevision || 0) !== revision || $("superres-output")?.value?.trim() !== dir || !state.superresFiles.includes(file) || state.superresResults.has(key)) return;
    state.superresResults.set(key, candidate);
    renderSuperresGallery();
  };
  probe.onerror = () => {
    if ((state.superresPreviewRevision || 0) === revision) state.superresProbed.delete(key);
  };
  probe.src = superresLocalSrc(candidate);
}

function renderSuperresGallery() {
  const files = state.superresFiles;
  const hasFiles = files.length > 0;
  $("superres-empty")?.classList.toggle("hidden", hasFiles);
  $("superres-gallery")?.classList.toggle("hidden", !hasFiles);
  setText("superres-toolbar-count", hasFiles ? `${files.length} 张图片` : "尚未添加图片");
  updateStatus();
  if (!hasFiles) return;
  renderSuperresGrid(files);
  files.forEach((file) => {
    probeSuperresResult(file, "anime");
    probeSuperresResult(file, "general");
  });
}

function renderSuperresGrid(files) {
  const grid = $("superres-grid");
  if (!grid) return;
  grid.innerHTML = "";
  files.forEach((file) => {
    const card = document.createElement("div");
    card.className = "superres-card";
    card.title = file;

    const img = document.createElement("img");
    img.alt = basename(file);
    img.draggable = false;
    const animePath = state.superresResults.get(superresResultKey(file, "anime"));
    const generalPath = state.superresResults.get(superresResultKey(file, "general"));
    const resultPath = state.activeMode === "superres-anime" ? animePath : generalPath;
    const preview = superresLocalSrc(resultPath || file);
    img.title = resultPath ? `${superresScale()}x ${superresLabels[state.activeMode === "superres-anime" ? "anime" : "general"]}结果` : "原图 · 当前模型和倍率尚无结果";
    if (preview) {
      img.src = preview;
    } else {
      card.classList.add("placeholder");
    }

    const info = document.createElement("div");
    info.className = "superres-card-info";
    const name = document.createElement("strong");
    name.textContent = basename(file);

    const badges = document.createElement("div");
    badges.className = "superres-card-badges";
    for (const modelId of ["anime", "general"]) {
      const done = Boolean(state.superresResults.get(superresResultKey(file, modelId)));
      const badge = document.createElement("span");
      badge.className = `thumb-badge ${done ? "done" : "pending"}`;
      badge.textContent = superresLabels[modelId].replace("超分", "");
      badge.title = done ? `已生成 ${superresScale()}x 结果` : "待处理";
      badges.appendChild(badge);
    }
    info.append(name, badges);

    const remove = document.createElement("span");
    remove.className = "thumb-remove";
    remove.title = `移除 ${basename(file)}`;
    remove.setAttribute("aria-label", remove.title);
    remove.innerHTML = '<i data-lucide="x"></i>';
    remove.addEventListener("click", (event) => {
      event.stopPropagation();
      removeSuperresFile(file);
    });

    card.append(img, info, remove);
    grid.appendChild(card);
  });
  refreshIcons(grid);
}

function removeSuperresFile(file) {
  state.superresFiles = state.superresFiles.filter((item) => item !== file);
  resetSuperresResults();
  renderSuperresGallery();
  updateStatus();
}

async function runSuperres(modelId, button) {
  await saveSettings();
  const blocker = getRunBlocker(`superres-${modelId}`);
  if (blocker) {
    reportRunBlocker(blocker);
    return;
  }
  if (!isSuperresModelReady(modelId)) {
    reportRunBlocker(`${superresLabels[modelId]}模型未安装，请先在右侧栏下载。`);
    return;
  }
  const scale = superresScale();
  addActivity(`开始${superresLabels[modelId]}`, `${state.superresFiles.length} 张图片 · 放大 ${scale} 倍`);
  const files = [...state.superresFiles];
  const outputPath = $("superres-output").value;
  const requestKeys = new Map(files.map((file) => [file, superresResultKey(file, modelId, scale)]));
  const revision = state.superresPreviewRevision || 0;
  state.superresRunning = true;
  renderSuperresModelStatus();
  const result = await withLog(
    "superres-log",
    button,
    () => api.superres.run({ files, outputPath, model: modelId, scale }),
    "图片超分"
  );
  state.superresRunning = false;
  renderSuperresModelStatus();
  if (result?.outputs?.length && (state.superresPreviewRevision || 0) === revision && $("superres-output").value === outputPath) applySuperresOutputs(result.outputs, requestKeys);
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
    "image-dds": "image-log",
    "anime-cutout": "anime-log",
    "normal-map": "material-maps-log",
    "height-map": "material-maps-log",
    "superres-anime": "superres-log",
    "superres-general": "superres-log"
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

function setTaskProgress(completed, total, message, percent = null) {
  const panel = $("task-progress");
  if (!panel) return;
  // percent 由后端直传时（含单文件内的图块/阶段细分进度）优先使用
  const raw = percent ?? (total ? completed / total * 100 : 0);
  const value = Number.isFinite(raw) ? Math.max(0, Math.min(100, raw)) : 0;
  panel.dataset.percent = String(value);
  panel.classList.remove("hidden");
  setText("task-progress-label", message || "正在处理");
  setText("task-progress-value", `${value < 100 ? Math.min(99, Math.round(value)) : 100}%`);
  $("task-progress-fill")?.style.setProperty("width", `${value}%`);
  $("task-progress-track")?.setAttribute("aria-valuenow", String(Math.round(value)));
}

async function withLog(logId, button, action, title) {
  if (state.taskProgressActive) {
    reportRunBlocker("另一个任务正在运行，请等待完成后重试。");
    return null;
  }
  const log = $(logId);
  if (log) log.textContent = "";
  setActivityPanel(true);
  setText("activity-summary", `${title}运行中`);
  setBusy(button, true);
  state.taskProgressActive = true;
  const panel = $("task-progress");
  if (panel) panel.dataset.status = "running";
  const started = Date.now();
  const updateElapsed = () => {
    const seconds = Math.floor((Date.now() - started) / 1000);
    setText("task-progress-detail", `已用时 ${Math.floor(seconds / 60)} 分 ${seconds % 60} 秒`);
  };
  updateElapsed();
  const elapsedTimer = setInterval(updateElapsed, 1000);
  setTaskProgress(0, 1, "正在准备任务");
  try {
    const result = await action();
    for (const line of result.logs || []) {
      if (log) log.textContent += `${line}\n`;
    }
    if (log) log.textContent += `完成：${result.completed} / ${result.total}`;
    const partial = result.completed < result.total;
    if (panel) panel.dataset.status = partial ? "partial" : "success";
    setTaskProgress(result.completed, result.total, partial ? `处理结束 · 成功 ${result.completed}/${result.total}，其余项目请查看日志` : `任务完成 · ${result.completed}/${result.total}`, 100);
    addActivity(title || "任务完成", `${result.completed} / ${result.total}`, partial ? "idle" : "success");
    state.lastOutputPath = getModeOutputPath();
    $("open-current-output")?.classList.toggle("hidden", !state.lastOutputPath);
    return result;
  } catch (error) {
    if (panel) panel.dataset.status = "error";
    setTaskProgress(0, 1, `任务失败 · ${error.message || error}`, Number(panel?.dataset.percent || 0));
    if (log) log.textContent += `失败：${error.message || error}`;
    addActivity(title || "任务失败", error.message || String(error), "error");
    return null;
  } finally {
    clearInterval(elapsedTimer);
    updateElapsed();
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
    mipmapIntermediate: Boolean($("mipmap-intermediate")?.checked),
    mipmapFormat: $("mipmap-format")?.value || "DXT5",
    imageToDdsOutputPath: $("image-output")?.value || "",
    imageToDdsAlpha: $("image-alpha")?.value || "keep",
    imageToDdsFormat: $("image-format")?.value || "DXT5",
    scaleTarget: $("scale-target")?.value || "none",
    skinManagerPath: $("skin-path")?.value || "",
    animeModel: $("anime-model")?.value || "anime-specialist",
    animeHairRefiner: Boolean($("anime-hair-refiner")?.checked),
    animeDetailRecovery: Boolean($("anime-detail-recovery")?.checked),
    animeCutoutOutputPath: $("anime-output")?.value || "",
    superresOutputPath: $("superres-output")?.value || "",
    superresScale: String(superresScale())
  };
}

function applySettingsToForm() {
  const settings = state.settings || {};
  if ($("mipmap-intermediate")) $("mipmap-intermediate").checked = Boolean(settings.mipmapIntermediate);
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
    "scale-target": settings.scaleTarget,
    "skin-path": settings.skinManagerPath,
    "anime-model": settings.animeModel || "anime-specialist",
    "anime-hair-refiner": Boolean(settings.animeHairRefiner),
    "anime-detail-recovery": Boolean(settings.animeDetailRecovery),
    "anime-output": settings.animeCutoutOutputPath,
    "superres-output": settings.superresOutputPath,
    "superres-scale": settings.superresScale || "4"
  };

  for (const [id, value] of Object.entries(map)) {
    const el = $(id);
    if (!el) continue;
    if (el.type === "checkbox") el.checked = Boolean(value);
    else el.value = value || "";
    if (el.tagName === "SELECT") syncCustomSelect(el);
  }
  updateSuperresScaleControl();
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
    "image-dds": "run-image-dds",
    "anime-cutout": "run-anime-cutout",
    "superres-anime": "run-superres-anime",
    "normal-map": "map-run",
    "height-map": "map-run",
    "superres-general": "run-superres-general"
  };

  document.querySelectorAll(".run-button").forEach((button) => button.classList.add("hidden"));
  const active = $(mapping[mode]);
  if (active) active.classList.remove("hidden");

  $("clear-split-files")?.classList.toggle("hidden", mode !== "split");
  $("clear-image-files")?.classList.toggle("hidden", mode !== "image-dds");
  $("clear-anime-files")?.classList.toggle("hidden", mode !== "anime-cutout");
  $("clear-superres-files")?.classList.toggle("hidden", !mode.startsWith("superres"));
  $("import-skins")?.classList.toggle("hidden", mode !== "skins");
  $("refresh-skins")?.classList.toggle("hidden", mode !== "skins");
}

function updateInspector() {
  document.querySelectorAll(".inspector-group").forEach((group) => {
    const modes = (group.dataset.modes || "").split(/\s+/).filter(Boolean);
    group.classList.toggle("hidden", modes.length > 0 && !modes.includes(state.activeMode));
  });
  renderGpuRuntime();

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
    "anime-output": state.activeMode === "anime-cutout",
    "superres-output": state.activeMode.startsWith("superres")
  };

  for (const [id, visible] of Object.entries(fieldVisibility)) {
    const row = document.querySelector(`[data-field="${id}"]`);
    if (row) row.classList.toggle("hidden", !visible);
  }
}

function updateStatus() {
  const mode = state.activeMode;
  const runnableModes = ["merge", "split", "mipmap", "image-dds", "anime-cutout", "superres-anime", "superres-general", "normal-map", "height-map"];
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
    "image-dds": "run-image-dds",
    "anime-cutout": "run-anime-cutout",
    "superres-anime": "run-superres-anime",
    "normal-map": "map-run",
    "height-map": "map-run",
    "superres-general": "run-superres-general"
  };
  const activeRunButton = $(runButtonByMode[mode]);
  if (activeRunButton && activeRunButton.dataset.busy !== "true") {
    // 超分各模式还要求对应模型已安装
    activeRunButton.disabled = !ready
      || (mode === "superres-anime" && !isSuperresModelReady("anime"))
      || (mode === "superres-general" && !isSuperresModelReady("general"));
  }

  $("open-current-output")?.classList.toggle("hidden", !outputPath || !runnableModes.includes(mode));
}

function getRunBlocker(mode) {
  if (state.taskProgressActive) return "另一个任务正在运行，请等待完成后重试。";
  const batchFiles = mode === "anime-cutout" ? state.animeFiles
    : mode.startsWith("superres") ? state.superresFiles
    : mode === "image-dds" ? state.imageFiles : mode === "split" ? state.splitFiles : [];
  const stems = new Set();
  for (const file of batchFiles || []) {
    const stem = basename(file).replace(/\.[^.]+$/, "").toLowerCase();
    if (stems.has(stem)) return `存在重名图片「${stem}」，请先重命名或分批导出，避免覆盖结果。`;
    stems.add(stem);
  }
  switch (mode) {
    case "normal-map":
    case "height-map":
      return materialMapsUI?.blocker() || null;
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
    case "anime-cutout":
      if (state.animeRunning) return "抠图正在运行，请稍候。";
      if (!state.animeFiles.length) return "请添加图片。";
      if (!$("anime-output")?.value) return "请选择输出文件夹。";
      if (state.animeDownloading || state.animeHairDownloading) return "模型正在下载中，请稍候。";
      if (wantsHairRefiner() && !state.animeHairStatus?.installed) return "请先在右侧栏下载精细发丝边缘模型，或关闭实验选项。";
      if (!isAnimeModelReady($("anime-model")?.value || "anime-specialist")) return "当前模型未安装，请先在「抠图模型」中下载。";
      return null;
    case "superres-anime":
    case "superres-general":
      if (state.superresRunning) return "超分正在运行，请稍候。";
      if (!state.superresFiles.length) return "请添加图片。";
      if (!$("superres-output")?.value) return "请选择输出文件夹。";
      if (state.superresDownloadingId) return "模型正在下载中，请稍候。";
      if (!isSuperresModelReady(mode === "superres-anime" ? "anime" : "general")) {
        return `${superresLabels[mode === "superres-anime" ? "anime" : "general"]}模型未安装，请先在右侧栏下载。`;
      }
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
  const changed = state.activeMode !== mode;
  closeCustomSelect();
  state.activeMode = mode;
  localStorage.setItem("aias-active-mode", mode);
  document.querySelectorAll(".mode-tab").forEach((button) => {
    button.classList.toggle("active", button.dataset.view === mode);
    button.setAttribute("aria-current", button.dataset.view === mode ? "page" : "false");
  });
  $("footer-settings")?.classList.toggle("active", mode === "settings");
  // 超分两个入口共用同一个视图
  const viewId = mode === "normal-map" || mode === "height-map" ? "view-material-maps" : mode.startsWith("superres") ? "view-superres" : `view-${mode}`;
  document.querySelectorAll(".mode-view").forEach((view) => {
    view.classList.toggle("active", view.id === viewId);
  });
  // Settings & skins mode: hide inspector; expand to full width
  const isFull = mode === "settings" || mode === "skins";
  const workspace = document.querySelector(".workspace");
  if (workspace) workspace.classList.toggle("full-width", isFull);
  const inspector = document.querySelector(".inspector");
  if (inspector) inspector.classList.toggle("hidden", isFull);
  if (mode === "settings") syncSettingsView();
  if (mode === "anime-cutout") {
    refreshAnimeModelStatus();
    refreshGpuRuntime();
    renderAnimeGallery();
  }
  if (mode.startsWith("superres")) {
    refreshSuperresModelStatus();
    renderSuperresGallery();
  }
  materialMapsUI?.activate(mode);
  syncActiveLog(mode);
  updateRunButtons(mode);
  updateInspector();
  updateStatus();
  if (changed) animateView($(viewId));
}

function bindSidebar() {
  const shell = document.querySelector(".app-shell");
  const edge = $("sidebar-edge");
  const toggle = $("sidebar-toggle");
  const compact = window.matchMedia("(max-width: 1120px)");
  let preference = localStorage.getItem("aias-sidebar");
  let collapsed = preference === "collapsed" || (preference !== "expanded" && compact.matches);
  const render = () => {
    shell.classList.toggle("sidebar-collapsed", collapsed);
    toggle.setAttribute("aria-expanded", String(!collapsed));
    toggle.setAttribute("aria-label", collapsed ? "展开侧栏" : "收起侧栏");
    edge.title = collapsed ? "点击展开侧栏" : "点击收起侧栏";
  };
  const change = (next) => {
    collapsed = next;
    preference = collapsed ? "collapsed" : "expanded";
    localStorage.setItem("aias-sidebar", preference);
    closeCustomSelect();
    render();
  };
  edge.addEventListener("click", () => change(!collapsed));
  edge.addEventListener("keydown", (event) => {
    if (!["Enter", " ", "ArrowLeft", "ArrowRight"].includes(event.key)) return;
    event.preventDefault();
    change(event.key === "ArrowLeft" ? true : event.key === "ArrowRight" ? false : !collapsed);
  });
  compact.addEventListener("change", () => {
    if (preference === "collapsed" || preference === "expanded") return;
    collapsed = compact.matches;
    closeCustomSelect();
    render();
  });
  render();
}

function bindTabs() {
  document.querySelectorAll(".mode-tab").forEach((button) => {
    button.title = button.textContent.trim();
    button.setAttribute("aria-label", button.title);
    button.addEventListener("click", () => applyMode(button.dataset.view));
  });
  $("footer-settings")?.addEventListener("click", () => applyMode("settings"));
}

function bindInspectorGroups() {
  document.querySelectorAll(".group-toggle").forEach((button) => {
    button.setAttribute("aria-expanded", "true");
    button.addEventListener("click", () => {
      toggleGroup(button);
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

  $("anime-model")?.addEventListener("change", () => {
    updateStatus();
    saveSettings();
    renderAnimeModelStatus();
    // 结果键带模型 id：切换模型后按新后缀探测/展示对应结果
    renderAnimeGallery();
  });

  $("anime-model-download")?.addEventListener("click", () => downloadAnimeModel());
  $("anime-hair-download")?.addEventListener("click", () => manageHairRefiner());
  $("anime-hair-uninstall")?.addEventListener("click", () => manageHairRefiner(true));
  $("anime-hair-refiner")?.addEventListener("change", () => {
    saveSettings();
    renderAnimeModelStatus();
    renderAnimeGallery();
  });
  $("anime-detail-recovery")?.addEventListener("change", () => {
    saveSettings();
    renderAnimeModelStatus();
    renderAnimeGallery();
  });
  $("anime-gpu-install")?.addEventListener("click", () => installGpuRuntime());
  $("anime-model-uninstall")?.addEventListener("click", () => uninstallAnimeModel());

  $("superres-anime-download")?.addEventListener("click", () => downloadSuperresModel("anime"));
  $("superres-general-download")?.addEventListener("click", () => downloadSuperresModel("general"));
  $("superres-anime-uninstall")?.addEventListener("click", () => uninstallSuperresModel("anime"));
  $("superres-general-uninstall")?.addEventListener("click", () => uninstallSuperresModel("general"));

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
      updateStatus();
      if (button.dataset.pickDir === "skin-path") {
        await refreshSkins();
      }
      if (button.dataset.pickDir === "anime-output") {
        resetAnimeResults();
        renderAnimeGallery();
      }
      if (button.dataset.pickDir === "superres-output") {
        resetSuperresResults();
        renderSuperresGallery();
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

  $("pick-anime-files")?.addEventListener("click", async () => {
    const files = await api.dialog.selectFiles({
      filters: [{ name: "Images", extensions: ["png", "jpg", "jpeg", "webp"] }]
    });
    state.animeFiles = [...new Set([...state.animeFiles, ...files])];
    renderAnimeGallery();
    updateStatus();
  });

  $("pick-superres-files")?.addEventListener("click", async () => {
    const files = await api.dialog.selectFiles({
      filters: [{ name: "Images", extensions: ["png", "jpg", "jpeg", "webp", "tga"] }]
    });
    state.superresFiles = [...new Set([...state.superresFiles, ...files])];
    renderSuperresGallery();
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

  $("clear-anime-files")?.addEventListener("click", () => {
    state.animeFiles = [];
    resetAnimeResults();
    state.animeActiveIndex = 0;
    renderAnimeGallery();
    updateStatus();
    addActivity("已清空列表", "抠图图片列表已清空。");
  });

  $("clear-superres-files")?.addEventListener("click", () => {
    state.superresFiles = [];
    resetSuperresResults();
    renderSuperresGallery();
    updateStatus();
    addActivity("已清空列表", "超分图片列表已清空。");
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
          format: $("pbr-format").value,
          scale: $("scale-target")?.value || "none"
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
          exportAlpha: $("split-alpha").checked,
          scale: $("scale-target")?.value || "none"
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
          intermediate: Boolean($("mipmap-intermediate")?.checked),
          format: $("mipmap-format").value,
          scale: $("scale-target")?.value || "none"
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
          format: $("image-format").value,
          scale: $("scale-target")?.value || "none"
        }),
      "图片转 DDS"
    );
  });

  $("run-anime-cutout")?.addEventListener("click", async (event) => {
    await saveSettings();
    const blocker = getRunBlocker("anime-cutout");
    if (blocker) {
      reportRunBlocker(blocker);
      return;
    }
    const modelId = $("anime-model")?.value || "anime-specialist";
    const recoverDetails = wantsDetailRecovery();
    const refineHair = wantsHairRefiner();
    const experimental = [recoverDetails && "高分辨率细节补全", refineHair && "精细发丝边缘"].filter(Boolean).join(" + ");
    addActivity("开始抠图", `${state.animeFiles.length} 张图片 · ${animeModelCatalog[modelId]?.label || modelId}${experimental ? ` · ${experimental}` : ""}`);
    const files = [...state.animeFiles];
    const outputPath = $("anime-output").value;
    const requestKeys = new Map(files.map((file) => [file, animeResultKey(file)]));
    state.animeRunning = true;
    renderAnimeModelStatus();
    const result = await withLog(
      "anime-log",
      event.currentTarget,
      () =>
        api.anime.cutout({
          files,
          outputPath,
          model: modelId,
          refineHair,
          recoverDetails
        }),
      "AI 抠图"
    );
    state.animeRunning = false;
    renderAnimeModelStatus();
    if (result?.outputs?.length && $("anime-output").value === outputPath) applyAnimeOutputs(result.outputs, requestKeys);
    refreshGpuRuntime();
  });

  $("run-superres-anime")?.addEventListener("click", (event) => {
    runSuperres("anime", event.currentTarget);
  });

  $("run-superres-general")?.addEventListener("click", (event) => {
    runSuperres("general", event.currentTarget);
  });

  $("superres-scale")?.addEventListener("input", () => {
    updateSuperresScaleControl();
    resetSuperresResults();
    renderSuperresGallery();
  });
  $("superres-scale")?.addEventListener("change", async () => {
    updateSuperresScaleControl();
    // 倍率变了，旧结果文件名对不上新倍率：清空后按新倍率重新探测。
    resetSuperresResults();
    renderSuperresGallery();
    await saveSettings();
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

const checkForUpdates = createUpdateController({
  isDesktop: isTauriRuntime,
  check,
  checkMirror: async () => {
    const metadata = await invoke("updater_check_mirror");
    return metadata ? new Update(metadata) : null;
  },
  getVersion,
  ui: {
    busy(value) {
      state.updateInProgress = value;
      for (const id of ["update-button", "set-check-update"]) {
        const button = $(id);
        if (button) button.disabled = value;
      }
    },
    available(value) { $("update-button")?.classList.toggle("hidden", !value); },
    activity: addActivity,
    status(value) { setText("runtime-badge", value); },
    confirm: openPreviewConfirm,
    message: openPreviewMessage,
    formatSize
  }
});

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
        saveSettings().then(() => {
          syncPathChips();
          updateStatus();
        });
        break;
      case "mipmap":
        $("mipmap-input").value = paths[0];
        saveSettings().then(() => {
          syncPathChips();
          updateStatus();
        });
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
      case "anime-cutout":
        state.animeFiles = [...new Set([...state.animeFiles, ...paths])];
        renderAnimeGallery();
        updateStatus();
        break;
      case "normal-map":
      case "height-map":
        materialMapsUI?.addFiles(paths);
        break;
      case "superres-anime":
      case "superres-general":
        state.superresFiles = [...new Set([...state.superresFiles, ...paths])];
        renderSuperresGallery();
        updateStatus();
        break;
    }
  });
}

async function init() {
  bindSidebar();
  const appVersion = isTauriRuntime ? await getVersion().catch(() => __APP_VERSION__) : __APP_VERSION__;
  setText("set-version", "当前版本 " + appVersion);
  setText("about-version", appVersion);
  state.settings = await api.settings.get();
  refreshIcons();
  enhanceSelectMenus();
  applySettingsToForm();
  materialMapsUI = createMaterialMaps({
    root: $("view-material-maps"), desktop: isTauriRuntime, invoke, open, openPath,
    inspector: document.querySelector('.inspector-scroll'), runArea: document.querySelector('.run-area'), syncSelect: syncCustomSelect,
    settings: state.settings.materialMaps, busy: () => state.taskProgressActive,
    save: async materialMaps => { state.settings = await api.settings.set({ materialMaps }); },
    withLog, notify: error => addActivity("材质生成", error, "error"),
  });
  enhanceSelectMenus();
  refreshIcons();
  bindTabs();
  bindInspectorGroups();
  bindWorkspaceActions();
  bindDropZones();
  bindDragDrop();
  bindFileControls();
  bindRunActions();
  bindAnimeGallery();
  bindSkinActions();
  bindSettingsActions();
  if (isTauriRuntime) {
    await listen("task-progress", (event) => {
      if (!state.taskProgressActive) return;
      const progress = event.payload;
      setTaskProgress(progress.completed, progress.total, progress.message, progress.percent ?? null);
    });
    await listen("model-progress", (event) => {
      const { modelId, file, completed, total } = event.payload;
      if (modelId === "vitmatte-hair-refiner") {
        const percent = total > 0 ? Math.min(100, Math.round(completed / total * 100)) : 0;
        $("anime-hair-progress-fill")?.style.setProperty("width", `${percent}%`);
        setText("anime-hair-progress-text", `${file} · ${percent}%`);
        return;
      }
      if (modelId === "ort-gpu") {
        const percent = total > 0 ? Math.min(100, Math.round((completed / total) * 100)) : 0;
        const fill = $("anime-gpu-progress-fill");
        if (fill) fill.style.width = `${percent}%`;
        setText("anime-gpu-progress-text", `${file} · ${percent}%（${formatBytes(completed)} / ${formatBytes(total)}）`);
        return;
      }
      // 超分模型下载进度（modelId 为 superres-anime / superres-general）
      if (modelId.startsWith("superres-")) {
        const target = modelId.slice("superres-".length);
        const percent = total > 0 ? Math.min(100, Math.round((completed / total) * 100)) : 0;
        const fill = $(`superres-${target}-progress-fill`);
        if (fill) fill.style.width = `${percent}%`;
        setText(`superres-${target}-progress-text`, `${file} · ${percent}%（${formatBytes(completed)} / ${formatBytes(total)}）`);
        return;
      }
      if (modelId !== ($("anime-model")?.value || "anime-specialist")) return;
      const percent = total > 0 ? Math.min(100, Math.round((completed / total) * 100)) : 0;
      const fill = $("anime-model-progress-fill");
      if (fill) fill.style.width = `${percent}%`;
      setText("anime-model-progress-text", `${file} · ${percent}%（${formatBytes(completed)} / ${formatBytes(total)}）`);
    });
  }
  $("update-button")?.addEventListener("click", () => checkForUpdates(false));
  startSystemMonitor();
  // 兜底轮询：文件/目录变化事件若被遗漏，抠图按钮可用性 1.2s 内自动纠正
  setInterval(() => {
    if (state.activeMode === "anime-cutout" || state.activeMode.startsWith("superres")) updateStatus();
  }, 1200);

  renderChips("merge-chip-list", []);
  renderChips("split-file-list", []);
  renderChips("mipmap-chip-list", []);
  renderChips("image-file-list", []);
  renderAnimeGallery();
  renderSuperresGallery();
  refreshSuperresModelStatus();
  await refreshSkins();
  syncPathChips();
  const savedMode = localStorage.getItem("aias-active-mode");
  applyMode(modeMeta[savedMode] ? savedMode : "merge");

  // Check for updates silently on startup
  scheduleUpdateCheck(isTauriRuntime, state.settings, setTimeout, checkForUpdates);
}

init();

if (import.meta.hot) import.meta.hot.dispose(() => materialMapsUI?.dispose());
