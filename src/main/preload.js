const { contextBridge, ipcRenderer } = require("electron");

contextBridge.exposeInMainWorld("aias", {
  settings: {
    get: () => ipcRenderer.invoke("settings:get"),
    set: (patch) => ipcRenderer.invoke("settings:set", patch)
  },
  dialog: {
    selectDirectory: () => ipcRenderer.invoke("dialog:selectDirectory"),
    selectFiles: (options) => ipcRenderer.invoke("dialog:selectFiles", options)
  },
  texture: {
    findGroups: (inputPath) => ipcRenderer.invoke("texture:findGroups", inputPath),
    mergePbr: (options) => ipcRenderer.invoke("texture:mergePbr", options),
    splitPbr: (options) => ipcRenderer.invoke("texture:splitPbr", options),
    createMipmap: (options) => ipcRenderer.invoke("texture:createMipmap", options),
    convertImagesToDds: (options) => ipcRenderer.invoke("texture:convertImagesToDds", options)
  },
  skin: {
    autoDetect: () => ipcRenderer.invoke("skin:autoDetect"),
    list: (directory) => ipcRenderer.invoke("skin:list", directory),
    import: (options) => ipcRenderer.invoke("skin:import", options),
    importFiles: (options) => ipcRenderer.invoke("skin:import", options),
    toggle: (filePath) => ipcRenderer.invoke("skin:toggle", filePath),
    delete: (filePath) => ipcRenderer.invoke("skin:delete", filePath)
  },
  shell: {
    openPath: (filePath) => ipcRenderer.invoke("shell:openPath", filePath)
  }
});
