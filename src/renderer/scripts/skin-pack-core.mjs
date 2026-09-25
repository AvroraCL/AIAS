import { zip } from 'fflate';

export async function buildBrowserPack(scan, packageName) {
  if (!safePackName(packageName)) throw new Error('包名无效。');
  if (scan.issues.some(item => item.level === 'error')) throw new Error('涂装检查未通过，请重新扫描。');
  const files = {};
  files[`${packageName}/`] = new Uint8Array();
  for (const item of scan.included) {
    const bytes = new Uint8Array(await item.file.arrayBuffer());
    if (await sha256(bytes) !== item.sha256) throw new Error(`${item.name} 在检查后发生变化，请重新扫描。`);
    files[`${packageName}/${item.name}`] = [bytes, { level: /\.dds$/i.test(item.name) ? 0 : 6 }];
  }
  return new Promise((resolve, reject) => zip(files, { level: 0 }, (error, data) => error ? reject(error) : resolve(data)));
}

async function sha256(bytes) {
  const digest = new Uint8Array(await crypto.subtle.digest('SHA-256', bytes));
  return [...digest].map(value => value.toString(16).padStart(2, '0')).join('');
}

export function safePackName(name) {
  return Boolean(name) && name !== '.' && name !== '..' && !/[<>:"/\\|?*\x00-\x1f]/.test(name)
    && !/[. ]$/.test(name) && !/^(con|prn|aux|nul|com[1-9]|lpt[1-9])(?:\.|$)/i.test(name);
}

function issue(level, message, file = null, line = null) { return { level, message, file, line }; }

function uncomment(line) {
  let quoted = false;
  for (let i = 0; i < line.length - 1; i++) {
    if (line[i] === '"' && line[i - 1] !== '\\') quoted = !quoted;
    if (!quoted && line.slice(i, i + 2) === '//') return line.slice(0, i);
  }
  return line;
}

function value(line, key) {
  const match = line.match(new RegExp(`^${key}\\s*=\\s*"([^"\\\\\r\n]*)"$`));
  return match?.[1] ?? null;
}

export function parsePackBlk(content, name) {
  const issues = [];
  const references = [];
  const seen = new Set();
  let block = null;
  content.replace(/^\uFEFF/, '').split(/\r?\n/).forEach((raw, index) => {
    const line = uncomment(raw).trim();
    const number = index + 1;
    if (!line) return;
    if (block) {
      if (line === '}') {
        if (!block.from || !block.to) issues.push(issue('error', `${block.kind} 缺少 from 或 to。`, name, block.line));
        else {
          if (seen.has(block.from.toLowerCase())) issues.push(issue('error', `原贴图名 ${block.from} 重复。`, name, block.line));
          seen.add(block.from.toLowerCase());
          references.push({ from: block.from, to: block.to, line: block.line });
        }
        block = null;
      } else if (value(line, 'from:t') !== null) {
        if (block.from !== null) issues.push(issue('error', '重复的 from 字段。', name, number));
        block.from = value(line, 'from:t');
      } else if (value(line, 'to:t') !== null) {
        if (block.to !== null) issues.push(issue('error', '重复的 to 字段。', name, number));
        block.to = value(line, 'to:t');
      } else if (value(line, 'param:t') === null) issues.push(issue('error', '无法解析的映射字段。', name, number));
    } else if (line.endsWith('{')) {
      const kind = line.slice(0, -1).trim();
      if (['replace_tex', 'set_tex'].includes(kind)) block = { kind, line: number, from: null, to: null };
      else issues.push(issue('error', `无法解析的 BLK 规则块 ${kind}。`, name, number));
    } else if (value(line, 'name:t') === null) issues.push(issue('error', '无法解析的 BLK 内容。', name, number));
  });
  if (block) issues.push(issue('error', `${block.kind} 规则块没有结束。`, name, block.line));
  if (!references.length) issues.push(issue('error', 'BLK 没有可打包的贴图映射。', name));
  return { references, issues };
}

function ddsInfo(bytes, size) {
  if (bytes.byteLength < 128) throw new Error('DDS 文件头不完整');
  const v = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  if (v.getUint32(0, true) !== 0x20534444 || v.getUint32(4, true) !== 124) throw new Error('不是有效 DDS 文件');
  const width = v.getUint32(16, true), height = v.getUint32(12, true), mipLevels = v.getUint32(28, true) || 1;
  if (!width || !height || width > 32768 || height > 32768 || mipLevels > 32) throw new Error('DDS 尺寸或 Mipmap 数量无效');
  const fourCC = String.fromCharCode(...bytes.subarray(84, 88));
  let blockBytes = { DXT1: 8, DXT3: 16, DXT5: 16, ATI1: 8, ATI2: 16, BC4U: 8, BC5U: 16 }[fourCC];
  let offset = 128;
  if (fourCC === 'DX10') {
    if (bytes.byteLength < 148) throw new Error('DDS DX10 文件头不完整');
    offset = 148;
    const format = v.getUint32(128, true);
    blockBytes = [71, 72, 80, 81].includes(format) ? 8 : [74, 75, 77, 78, 83, 84, 95, 96, 98, 99].includes(format) ? 16 : null;
  }
  let bits = v.getUint32(88, true);
  if (fourCC === 'DX10' && !blockBytes) {
    const format = v.getUint32(128, true);
    bits = [28, 29, 87, 88, 91, 93].includes(format) ? 32 : [10, 11, 12, 13].includes(format) ? 64 : 0;
  }
  if (!blockBytes && (!bits || bits > 128)) throw new Error('暂不支持的 DDS 像素格式');
  let required = 0;
  for (let level = 0; level < mipLevels; level++) {
    const w = Math.max(1, Math.floor(width / 2 ** level));
    const h = Math.max(1, Math.floor(height / 2 ** level));
    required += blockBytes ? Math.ceil(w / 4) * Math.ceil(h / 4) * blockBytes : w * h * Math.ceil(bits / 8);
  }
  if (size < offset + required) throw new Error('DDS 图像数据不完整');
  return { width, height, mipLevels };
}

function tgaInfo(bytes, size) {
  if (bytes.byteLength < 18) throw new Error('TGA 文件头不完整');
  const h = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  const width = h.getUint16(12, true), height = h.getUint16(14, true), depth = h.getUint8(16);
  const type = h.getUint8(2), mapType = h.getUint8(1), pixelSize = Math.ceil(depth / 8);
  if (!width || !height || !pixelSize || width > 32768 || height > 32768) throw new Error('TGA 尺寸无效');
  if (![1, 2, 3, 9, 10, 11].includes(type)) throw new Error(`暂不支持的 TGA 类型 ${type}`);
  const mapSize = mapType ? h.getUint16(5, true) * Math.ceil(h.getUint8(7) / 8) : 0;
  const offset = 18 + h.getUint8(0) + mapSize;
  if (type < 9) {
    if (size < offset + width * height * pixelSize) throw new Error('TGA 图像数据不完整');
  } else {
    let pos = offset, pixels = 0;
    while (pixels < width * height) {
      if (pos >= bytes.byteLength) throw new Error('TGA RLE 图像数据不完整');
      const packet = bytes[pos++], count = (packet & 127) + 1;
      pos += (packet & 128) ? pixelSize : count * pixelSize;
      pixels += count;
      if (pixels > width * height || pos > bytes.byteLength) throw new Error('TGA RLE 图像数据损坏');
    }
  }
  return { width, height, mipLevels: null };
}

export async function scanPackFiles(files, selectedBlk = null) {
  const names = files.map(file => file.name).sort((a, b) => a.localeCompare(b));
  const blks = names.filter(name => /\.blk$/i.test(name));
  const issues = [];
  const normalized = new Set();
  for (const name of names) {
    if (normalized.has(name.toLowerCase())) issues.push(issue('error', `目录中存在大小写冲突的文件名：${name}。`, name));
    normalized.add(name.toLowerCase());
  }
  const chosen = selectedBlk ? (blks.includes(selectedBlk) ? selectedBlk : null) : blks.length === 1 ? blks[0] : null;
  if (!chosen) issues.push(issue('error', selectedBlk ? '所选 BLK 已不存在，请重新选择。' : blks.length ? '目录中有多份 BLK，请选择本次打包的一份。' : '目录中没有 BLK，请先生成。'));
  const included = [], used = new Set();
  if (chosen) {
    if (!safePackName(chosen)) issues.push(issue('error', 'BLK 文件名不安全。', chosen));
    else {
      const blk = files.find(file => file.name === chosen);
      const blkBytes = new Uint8Array(await blk.arrayBuffer());
      let text = '';
      try { text = new TextDecoder('utf-8', { fatal: true }).decode(blkBytes); }
      catch { issues.push(issue('error', 'BLK 不是 UTF-8/ASCII 文本。', chosen)); }
      const parsed = text ? parsePackBlk(text, chosen) : { references: [], issues: [] };
      issues.push(...parsed.issues);
      included.push({ name: chosen, size: blk.size, width: null, height: null, mipLevels: null, file: blk, sha256: await sha256(blkBytes) });
      used.add(chosen);
      for (const ref of parsed.references) {
        const name = ref.to;
        if (!safePackName(name)) { issues.push(issue('error', `贴图路径 ${name} 不安全或不在当前目录。`, chosen, ref.line)); continue; }
        if (!/\.(dds|tga)$/i.test(name)) { issues.push(issue('error', `贴图 ${name} 不是支持的 DDS/TGA 格式。`, chosen, ref.line)); continue; }
        const file = files.find(file => file.name === name);
        if (!file) {
          issues.push(issue('error', names.some(other => other.toLowerCase() === name.toLowerCase()) ? `贴图 ${name} 的文件名大小写与 BLK 不一致。` : `找不到贴图 ${name}。`, chosen, ref.line));
          continue;
        }
        if (used.has(name)) continue;
        used.add(name);
        try {
          // DDS 只需文件头；RLE TGA 为检测截断需读取完整数据。
          const fullBytes = new Uint8Array(await file.arrayBuffer());
          const bytes = /\.dds$/i.test(name) ? fullBytes.subarray(0, 148) : fullBytes;
          const info = /\.dds$/i.test(name) ? ddsInfo(bytes, file.size) : tgaInfo(bytes, file.size);
          included.push({ name, size: file.size, ...info, file, sha256: await sha256(fullBytes) });
          if (info.mipLevels === 1) issues.push(issue('warning', 'DDS 没有完整 Mipmap 链，远景可能闪烁。', name));
          if (!Number.isInteger(Math.log2(info.width)) || !Number.isInteger(Math.log2(info.height)) || info.width > 4096 || info.height > 4096)
            issues.push(issue('warning', `贴图尺寸 ${info.width}×${info.height} 不属于常见投稿尺寸。`, name));
        } catch (error) { issues.push(issue('error', `${/\.dds$/i.test(name) ? 'DDS' : 'TGA'} 无法解码：${error.message}`, name)); }
      }
    }
  }
  const excluded = names.filter(name => !used.has(name));
  if (excluded.length) issues.push(issue('warning', `另有 ${excluded.length} 个文件未被 BLK 引用，不会收入 ZIP。`));
  return { blks, selectedBlk: chosen, included, excluded, issues, fingerprint: files.map(f => `${f.name}:${f.size}:${f.lastModified}`).join('|') };
}
