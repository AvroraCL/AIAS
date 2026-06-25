const path = require("node:path");
const { app, BrowserWindow, dialog, ipcMain, shell } = require("electron");
const { SettingsService } = require("./services/settings-service");
const { TextureService } = require("./services/texture-service");
const { SkinService } = require("./services/skin-service");

let settingsService;
let textureService;
let skinService;

let mainWindow;

function createWindow() {
  const rendererUrl = process.env.AIAS_RENDERER_URL;
  mainWindow = new BrowserWindow({
    width: 1320,
    height: 820,
    minWidth: 1120,
    minHeight: 820,
    title: "AIAS",
    backgroundColor: "#121316",
    icon: path.join(app.getAppPath(), "favicon.ico"),
    webPreferences: {
      preload: path.join(__dirname, "preload.js"),
      contextIsolation: true,
      nodeIntegration: false,
      sandbox: false
    }
  });

  if (rendererUrl) {
    mainWindow.loadURL(rendererUrl);
    mainWindow.webContents.openDevTools({ mode: "detach" });
  } else {
    mainWindow.loadFile(path.join(__dirname, "../renderer/index.html"));
  }
}

function bindIpc() {
  ipcMain.handle("settings:get", () => settingsService.getAll());
  ipcMain.handle("settings:set", (_event, patch) => settingsService.setMany(patch));

  ipcMain.handle("dialog:selectDirectory", async () => {
    const result = await dialog.showOpenDialog(mainWindow, {
      properties: ["openDirectory", "createDirectory"]
    });
    return result.canceled ? null : result.filePaths[0];
  });

  ipcMain.handle("dialog:selectFiles", async (_event, options = {}) => {
    const result = await dialog.showOpenDialog(mainWindow, {
      properties: ["openFile", "multiSelections"],
      filters: options.filters || []
    });
    return result.canceled ? [] : result.filePaths;
  });

  ipcMain.handle("texture:findGroups", (_event, inputPath) => textureService.findTextureGroups(inputPath));
  ipcMain.handle("texture:mergePbr", (_event, options) => textureService.mergePbr(options));
  ipcMain.handle("texture:splitPbr", (_event, options) => textureService.splitPbr(options));
  ipcMain.handle("texture:createMipmap", (_event, options) => textureService.createMipmap(options));
  ipcMain.handle("texture:convertImagesToDds", (_event, options) => textureService.convertImagesToDds(options));

  ipcMain.handle("skin:autoDetect", () => skinService.autoDetectUserSkinsPath());
  ipcMain.handle("skin:list", (_event, directory) => skinService.listSkins(directory));
  ipcMain.handle("skin:import", (_event, options) => skinService.importFiles(options));
  ipcMain.handle("skin:toggle", (_event, filePath) => skinService.toggleSkin(filePath));
  ipcMain.handle("skin:delete", (_event, filePath) => skinService.deleteSkin(filePath));
  ipcMain.handle("shell:openPath", (_event, filePath) => shell.openPath(filePath));
}

app.whenReady().then(() => {
  settingsService = new SettingsService(app.getPath("userData"));
  textureService = new TextureService({
    appRoot: app.getAppPath(),
    isPackaged: app.isPackaged
  });
  skinService = new SkinService(settingsService);
  bindIpc();
  createWindow();

  app.on("activate", () => {
    if (BrowserWindow.getAllWindows().length === 0) createWindow();
  });
});

app.on("window-all-closed", () => {
  if (process.platform !== "darwin") app.quit();
});
