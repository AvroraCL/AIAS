const fs = require("node:fs/promises");
const fssync = require("node:fs");
const os = require("node:os");
const path = require("node:path");
const { spawnFile } = require("../utils/process");
const sharp = require("sharp");

const IMAGE_EXTS = new Set([".png", ".tga", ".jpg", ".jpeg"]);
const DDS_EXT = ".dds";

class TextureService {
  constructor({ appRoot, isPackaged }) {
    this.appRoot = appRoot;
    this.isPackaged = isPackaged;
  }

  getTexconvPath() {
    const candidates = [
      path.join(this.appRoot, "tools", "texconv.exe"),
      path.join(process.resourcesPath || "", "tools", "texconv.exe"),
      path.join(process.cwd(), "tools", "texconv.exe"),
      path.join(process.cwd(), "texconv.exe")
    ];
    const found = candidates.find((candidate) => candidate && fssync.existsSync(candidate));
    if (!found) {
      throw new Error("未找到 texconv.exe。请确认 tools/texconv.exe 存在。");
    }
    return found;
  }

  async findTextureGroups(folder) {
    if (!folder) return [];
    const entries = await fs.readdir(folder, { withFileTypes: true });
    const groups = new Map();

    for (const entry of entries) {
      if (!entry.isFile()) continue;
      const ext = path.extname(entry.name).toLowerCase();
      if (!IMAGE_EXTS.has(ext)) continue;

      const stem = path.basename(entry.name, ext);
      const lower = stem.toLowerCase();
      for (const type of ["basecolor", "roughness", "metallic", "normal"]) {
        if (!lower.endsWith(type)) continue;
        const prefix = stem.slice(0, -type.length).replace(/[_\-\s]+$/u, "");
        if (!groups.has(prefix)) groups.set(prefix, {});
        groups.get(prefix)[type] = path.join(folder, entry.name);
      }
    }

    return [...groups.entries()]
      .filter(([, value]) => value.basecolor && value.roughness && value.metallic && value.normal)
      .map(([prefix, files]) => ({ prefix, files }));
  }

  async mergePbr({ inputPath, outputPath, alpha = "black", format = "DXT5" }) {
    this.requireDirectory(inputPath, "输入目录");
    await fs.mkdir(outputPath, { recursive: true });

    const groups = await this.findTextureGroups(inputPath);
    const logs = [`找到 ${groups.length} 组完整 PBR 贴图。`];
    let completed = 0;

    for (const group of groups) {
      const cPath = path.join(outputPath, `${group.prefix}_c.dds`);
      const nPath = path.join(outputPath, `${group.prefix}_n.dds`);
      await this.processBaseColor(group.files.basecolor, cPath, alpha, format);
      await this.processRoughnessMetallicNormal(group.files.roughness, group.files.metallic, group.files.normal, nPath, format);
      completed += 1;
      logs.push(`完成 ${group.prefix}`);
    }

    return { completed, total: groups.length, logs };
  }

  async processBaseColor(baseColorPath, outputPath, alpha, format) {
    const image = sharp(baseColorPath).ensureAlpha();
    const metadata = await image.metadata();
    const channels = await image.raw().toBuffer({ resolveWithObject: true });
    const data = Buffer.from(channels.data);
    const alphaValue = alpha === "white" ? 255 : 0;

    for (let i = 3; i < data.length; i += 4) data[i] = alphaValue;

    await this.rawToDds(data, metadata.width, metadata.height, outputPath, format);
  }

  async processRoughnessMetallicNormal(roughnessPath, metallicPath, normalPath, outputPath, format) {
    const normal = sharp(normalPath).ensureAlpha();
    const normalRaw = await normal.raw().toBuffer({ resolveWithObject: true });
    const width = normalRaw.info.width;
    const height = normalRaw.info.height;

    const roughnessRaw = await sharp(roughnessPath)
      .resize(width, height, { fit: "fill" })
      .greyscale()
      .raw()
      .toBuffer();
    const metallicRaw = await sharp(metallicPath)
      .resize(width, height, { fit: "fill" })
      .greyscale()
      .raw()
      .toBuffer();

    const normalData = normalRaw.data;
    const combined = Buffer.alloc(width * height * 4);
    for (let pixel = 0; pixel < width * height; pixel += 1) {
      const source = pixel * 4;
      combined[source] = 255 - roughnessRaw[pixel];
      combined[source + 1] = normalData[source + 1];
      combined[source + 2] = metallicRaw[pixel];
      combined[source + 3] = normalData[source];
    }

    await this.rawToDds(combined, width, height, outputPath, format);
  }

  async splitPbr({ files = [], outputPath, exportAlpha = true, exportFormat = "png" }) {
    await fs.mkdir(outputPath, { recursive: true });
    const logs = [];
    let completed = 0;

    for (const file of files) {
      const stem = path.basename(file, path.extname(file));
      const prefix = stem.replace(/_[cn]$/iu, "");
      const image = await this.ddsToSharp(file);
      const raw = await image.ensureAlpha().raw().toBuffer({ resolveWithObject: true });
      const { width, height } = raw.info;
      const data = raw.data;

      if (stem.toLowerCase().endsWith("_c")) {
        const rgb = Buffer.alloc(width * height * 3);
        const alpha = Buffer.alloc(width * height);
        for (let pixel = 0; pixel < width * height; pixel += 1) {
          const source = pixel * 4;
          const dest = pixel * 3;
          rgb[dest] = data[source];
          rgb[dest + 1] = data[source + 1];
          rgb[dest + 2] = data[source + 2];
          alpha[pixel] = data[source + 3];
        }
        await this.saveRawImage(rgb, width, height, 3, path.join(outputPath, `${prefix}_BaseColor.${exportFormat}`), exportFormat);
        if (exportAlpha) {
          await this.saveRawImage(alpha, width, height, 1, path.join(outputPath, `${prefix}_Alpha.${exportFormat}`), exportFormat);
        }
        logs.push(`拆分 ${stem}: BaseColor${exportAlpha ? " / Alpha" : ""}`);
      } else if (stem.toLowerCase().endsWith("_n")) {
        const roughness = Buffer.alloc(width * height);
        const metallic = Buffer.alloc(width * height);
        const normal = Buffer.alloc(width * height * 4);
        for (let pixel = 0; pixel < width * height; pixel += 1) {
          const source = pixel * 4;
          roughness[pixel] = 255 - data[source];
          metallic[pixel] = data[source + 2];
          normal[source] = data[source + 3];
          normal[source + 1] = data[source + 1];
          normal[source + 2] = 255;
          normal[source + 3] = 255;
        }
        await this.saveRawImage(roughness, width, height, 1, path.join(outputPath, `${prefix}_Roughness.${exportFormat}`), exportFormat);
        await this.saveRawImage(metallic, width, height, 1, path.join(outputPath, `${prefix}_Metallic.${exportFormat}`), exportFormat);
        await this.saveRawImage(normal, width, height, 4, path.join(outputPath, `${prefix}_Normal.${exportFormat}`), exportFormat);
        logs.push(`拆分 ${stem}: Roughness / Metallic / Normal`);
      }
      completed += 1;
    }

    return { completed, total: files.length, logs };
  }

  async createMipmap({ inputPath, outputPath, alpha = "keep", format = "DXT5" }) {
    this.requireDirectory(inputPath, "输入目录");
    await fs.mkdir(outputPath, { recursive: true });

    const files = [];
    for (let i = 0; i < 1000; i += 1) {
      const found = [...IMAGE_EXTS]
        .map((ext) => path.join(inputPath, `p${i}${ext}`))
        .find((candidate) => fssync.existsSync(candidate));
      if (found) files.push(found);
    }

    if (files.length === 0) throw new Error("未找到 p0、p1、p2... mipmap 文件。");

    const outputFile = path.join(outputPath, "Mipmap.dds");
    const tempDir = await fs.mkdtemp(path.join(os.tmpdir(), "aias-mipmap-"));
    try {
      const prepared = [];
      for (const [index, file] of files.entries()) {
        const tempPng = path.join(tempDir, `p${index}.png`);
        await this.prepareImage(file, tempPng, alpha);
        prepared.push(tempPng);
      }

      const levels = [];
      for (const file of prepared) {
        const dds = await this.pngToDdsBuffer(file, format);
        const metadata = await sharp(file).metadata();
        levels.push({
          width: metadata.width,
          height: metadata.height,
          payload: extractDdsPayload(dds)
        });
      }
      await fs.writeFile(outputFile, buildDdsWithMipmaps(levels, format));
    } finally {
      await fs.rm(tempDir, { recursive: true, force: true });
    }

    return { completed: files.length, total: files.length, logs: [`生成 ${outputFile}`] };
  }

  async convertImagesToDds({ files = [], outputPath, alpha = "keep", format = "DXT5" }) {
    await fs.mkdir(outputPath, { recursive: true });
    const logs = [];
    for (const file of files) {
      const outputFile = path.join(outputPath, `${path.basename(file, path.extname(file))}.dds`);
      await this.imageToDds(file, outputFile, alpha, format);
      logs.push(`转换 ${path.basename(file)} -> ${path.basename(outputFile)}`);
    }
    return { completed: files.length, total: files.length, logs };
  }

  async imageToDds(inputPath, outputPath, alpha, format) {
    const tempDir = await fs.mkdtemp(path.join(os.tmpdir(), "aias-image-dds-"));
    try {
      const tempPng = path.join(tempDir, "source.png");
      await this.prepareImage(inputPath, tempPng, alpha);
      await this.convertPngToDds(tempPng, outputPath, format);
    } finally {
      await fs.rm(tempDir, { recursive: true, force: true });
    }
  }

  async prepareImage(inputPath, outputPath, alpha) {
    let image = sharp(inputPath).ensureAlpha();
    if (alpha === "black" || alpha === "white") {
      const metadata = await image.metadata();
      const raw = await image.raw().toBuffer({ resolveWithObject: true });
      const data = Buffer.from(raw.data);
      const alphaValue = alpha === "white" ? 255 : 0;
      for (let i = 3; i < data.length; i += 4) data[i] = alphaValue;
      image = sharp(data, { raw: { width: metadata.width, height: metadata.height, channels: 4 } });
    }
    await image.png().toFile(outputPath);
  }

  async rawToDds(raw, width, height, outputPath, format) {
    const tempDir = await fs.mkdtemp(path.join(os.tmpdir(), "aias-raw-dds-"));
    try {
      const tempPng = path.join(tempDir, "source.png");
      await sharp(raw, { raw: { width, height, channels: 4 } }).png().toFile(tempPng);
      await this.convertPngToDds(tempPng, outputPath, format);
    } finally {
      await fs.rm(tempDir, { recursive: true, force: true });
    }
  }

  async convertPngToDds(inputPng, outputPath, format) {
    const buffer = await this.pngToDdsBuffer(inputPng, format);
    await fs.writeFile(outputPath, buffer);
  }

  async pngToDdsBuffer(inputPng, format) {
    const tempDir = await fs.mkdtemp(path.join(os.tmpdir(), "aias-texconv-"));
    try {
      await spawnFile(this.getTexconvPath(), [
        "-y",
        "-f",
        this.toTexconvFormat(format),
        "-m",
        "1",
        "-o",
        tempDir,
        inputPng
      ]);
      const generated = path.join(tempDir, `${path.basename(inputPng, path.extname(inputPng))}.dds`);
      return await fs.readFile(generated);
    } finally {
      await fs.rm(tempDir, { recursive: true, force: true });
    }
  }

  async ddsToSharp(ddsPath) {
    const tempDir = await fs.mkdtemp(path.join(os.tmpdir(), "aias-dds-png-"));
    try {
      await spawnFile(this.getTexconvPath(), ["-y", "-ft", "png", "-o", tempDir, ddsPath]);
      const pngPath = path.join(tempDir, `${path.basename(ddsPath, DDS_EXT)}.png`);
      const buffer = await fs.readFile(pngPath);
      return sharp(buffer);
    } finally {
      await fs.rm(tempDir, { recursive: true, force: true });
    }
  }

  async saveRawImage(raw, width, height, channels, outputPath, format) {
    if (format.toLowerCase() === "tga") {
      await fs.writeFile(outputPath, encodeTga(raw, width, height, channels));
      return;
    }
    await sharp(raw, { raw: { width, height, channels } }).toFile(outputPath);
  }

  toTexconvFormat(format) {
    if (format === "8.8.8.8" || format === "R8G8B8A8_UNORM") return "R8G8B8A8_UNORM";
    return "BC3_UNORM";
  }

  requireDirectory(directory, label) {
    if (!directory || !fssync.existsSync(directory) || !fssync.statSync(directory).isDirectory()) {
      throw new Error(`${label}无效。`);
    }
  }
}

module.exports = { TextureService, IMAGE_EXTS };

function encodeTga(raw, width, height, channels) {
  const pixelDepth = channels === 1 ? 8 : channels === 4 ? 32 : 24;
  const imageType = channels === 1 ? 3 : 2;
  const descriptor = channels === 4 ? 8 | 0x20 : 0x20;
  const header = Buffer.alloc(18);
  header[2] = imageType;
  header.writeUInt16LE(width, 12);
  header.writeUInt16LE(height, 14);
  header[16] = pixelDepth;
  header[17] = descriptor;

  if (channels === 1) return Buffer.concat([header, raw]);

  const pixels = Buffer.alloc(width * height * channels);
  for (let i = 0; i < width * height; i += 1) {
    const source = i * channels;
    pixels[source] = raw[source + 2];
    pixels[source + 1] = raw[source + 1];
    pixels[source + 2] = raw[source];
    if (channels === 4) pixels[source + 3] = raw[source + 3];
  }
  return Buffer.concat([header, pixels]);
}

function extractDdsPayload(dds) {
  if (dds.length < 128) throw new Error("DDS 数据过短。");
  if (dds.subarray(0, 4).toString("ascii") !== "DDS ") throw new Error("DDS Magic 无效。");
  const headerSize = dds.readUInt32LE(4);
  let headerEnd = 4 + headerSize;
  const pixelFlags = dds.readUInt32LE(80);
  const fourCc = dds.subarray(84, 88).toString("ascii");
  if ((pixelFlags & 0x4) && fourCc === "DX10") headerEnd += 20;
  return dds.subarray(headerEnd);
}

function buildDdsWithMipmaps(levels, format) {
  if (!levels.length) throw new Error("没有可写入的 mipmap 层级。");
  const first = levels[0];
  const compressed = format !== "8.8.8.8" && format !== "R8G8B8A8_UNORM";
  const header = Buffer.alloc(128);
  header.write("DDS ", 0, "ascii");
  header.writeUInt32LE(124, 4);
  header.writeUInt32LE(0x1 | 0x2 | 0x4 | 0x1000 | 0x20000 | (compressed ? 0x80000 : 0x8), 8);
  header.writeUInt32LE(first.height, 12);
  header.writeUInt32LE(first.width, 16);
  header.writeUInt32LE(compressed ? calculateBc3Size(first.width, first.height) : first.width * 4, 20);
  header.writeUInt32LE(0, 24);
  header.writeUInt32LE(levels.length, 28);
  header.writeUInt32LE(32, 76);

  if (compressed) {
    header.writeUInt32LE(0x4, 80);
    header.write("DXT5", 84, "ascii");
  } else {
    header.writeUInt32LE(0x40 | 0x1, 80);
    header.writeUInt32LE(32, 88);
    header.writeUInt32LE(0x00ff0000, 92);
    header.writeUInt32LE(0x0000ff00, 96);
    header.writeUInt32LE(0x000000ff, 100);
    header.writeUInt32LE(0xff000000, 104);
  }

  header.writeUInt32LE(0x1000 | 0x400000 | 0x8, 108);
  return Buffer.concat([header, ...levels.map((level) => level.payload)]);
}

function calculateBc3Size(width, height) {
  return Math.ceil(width / 4) * Math.ceil(height / 4) * 16;
}
