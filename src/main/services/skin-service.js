const fs = require("node:fs/promises");
const fssync = require("node:fs");
const path = require("node:path");
const { spawnFile } = require("../utils/process");

class SkinService {
  constructor(settingsService) {
    this.settingsService = settingsService;
  }

  async autoDetectUserSkinsPath() {
    const steamPath = await this.findSteamPath();
    if (!steamPath) return null;

    const libraries = await this.findSteamLibraries(steamPath);
    for (const library of libraries) {
      const candidate = path.join(library, "steamapps", "common", "War Thunder", "UserSkins");
      if (fssync.existsSync(candidate)) return candidate;
    }
    return null;
  }

  async findSteamPath() {
    if (process.platform === "win32") {
      try {
        const result = await spawnFile("reg", ["query", "HKCU\\Software\\Valve\\Steam", "/v", "SteamPath"]);
        const match = result.stdout.match(/SteamPath\s+REG_\w+\s+(.+)/i);
        if (match && fssync.existsSync(match[1].trim())) return match[1].trim();
      } catch {
        // Fall back to common paths below.
      }
    }

    const candidates = [
      "C:\\Program Files (x86)\\Steam",
      "C:\\Program Files\\Steam",
      "D:\\Steam",
      "E:\\Steam"
    ];
    return candidates.find((candidate) => fssync.existsSync(candidate)) || null;
  }

  async findSteamLibraries(steamPath) {
    const libraries = new Set([steamPath]);
    const vdfPath = path.join(steamPath, "steamapps", "libraryfolders.vdf");
    if (!fssync.existsSync(vdfPath)) return [...libraries];

    const content = await fs.readFile(vdfPath, "utf8");
    const matches = content.matchAll(/"path"\s+"([^"]+)"/gi);
    for (const match of matches) {
      const libraryPath = match[1].replace(/\\\\/g, "\\");
      if (fssync.existsSync(libraryPath)) libraries.add(libraryPath);
    }
    return [...libraries];
  }

  async listSkins(directory) {
    this.requireDirectory(directory);
    const entries = await fs.readdir(directory, { withFileTypes: true });
    return entries
      .filter((entry) => entry.isFile())
      .map((entry) => {
        const filePath = path.join(directory, entry.name);
        const stats = fssync.statSync(filePath);
        return {
          name: entry.name,
          path: filePath,
          disabled: entry.name.endsWith(".disabled"),
          size: stats.size,
          modifiedAt: stats.mtimeMs
        };
      })
      .sort((a, b) => a.name.localeCompare(b.name, "en"));
  }

  async importFiles({ files = [], targetDirectory }) {
    this.requireDirectory(targetDirectory);
    let imported = 0;
    for (const file of files) {
      if (!fssync.existsSync(file) || !fssync.statSync(file).isFile()) continue;
      await fs.copyFile(file, path.join(targetDirectory, path.basename(file)));
      imported += 1;
    }
    return { imported };
  }

  async toggleSkin(filePath) {
    if (!fssync.existsSync(filePath)) throw new Error("文件不存在。");
    const nextPath = filePath.endsWith(".disabled")
      ? filePath.slice(0, -".disabled".length)
      : `${filePath}.disabled`;
    await fs.rename(filePath, nextPath);
    return { path: nextPath };
  }

  async deleteSkin(filePath) {
    if (!fssync.existsSync(filePath)) return { deleted: false };
    await fs.unlink(filePath);
    return { deleted: true };
  }

  requireDirectory(directory) {
    if (!directory || !fssync.existsSync(directory) || !fssync.statSync(directory).isDirectory()) {
      throw new Error("涂装目录无效。");
    }
  }
}

module.exports = { SkinService };
