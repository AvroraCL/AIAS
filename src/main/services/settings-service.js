const fs = require("node:fs");
const path = require("node:path");

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

class SettingsService {
  constructor(userDataPath) {
    this.filePath = path.join(userDataPath, "settings.json");
    this.data = this.load();
  }

  load() {
    try {
      if (!fs.existsSync(this.filePath)) return { ...defaults };
      const parsed = JSON.parse(fs.readFileSync(this.filePath, "utf8"));
      return { ...defaults, ...parsed };
    } catch {
      return { ...defaults };
    }
  }

  save() {
    fs.mkdirSync(path.dirname(this.filePath), { recursive: true });
    fs.writeFileSync(this.filePath, `${JSON.stringify(this.data, null, 2)}\n`, "utf8");
  }

  getAll() {
    return { ...this.data };
  }

  setMany(patch) {
    for (const [key, value] of Object.entries(patch || {})) {
      this.data[key] = value;
    }
    this.save();
    return this.getAll();
  }
}

module.exports = { SettingsService };
