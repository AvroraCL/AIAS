const MAX_SIDE = 512;
const MAX_READ_BYTES = 128 * 1024 * 1024;

function u32(view, offset) { return view.getUint32(offset, true); }

function rgb565(value) {
  return [
    Math.round(((value >> 11) & 31) * 255 / 31),
    Math.round(((value >> 5) & 63) * 255 / 63),
    Math.round((value & 31) * 255 / 31)
  ];
}

function bc3Pixel(bytes, blockOffset, x, y) {
  const a0 = bytes[blockOffset];
  const a1 = bytes[blockOffset + 1];
  const alpha = [a0, a1];
  if (a0 > a1) {
    for (let i = 1; i <= 6; i++) alpha.push(Math.round(((7 - i) * a0 + i * a1) / 7));
  } else {
    for (let i = 1; i <= 4; i++) alpha.push(Math.round(((5 - i) * a0 + i * a1) / 5));
    alpha.push(0, 255);
  }
  const index = (y & 3) * 4 + (x & 3);
  const alphaBit = index * 3;
  const alphaByte = blockOffset + 2 + (alphaBit >> 3);
  const alphaIndex = ((bytes[alphaByte] | ((bytes[alphaByte + 1] || 0) << 8)) >> (alphaBit & 7)) & 7;
  const c0 = bytes[blockOffset + 8] | (bytes[blockOffset + 9] << 8);
  const c1 = bytes[blockOffset + 10] | (bytes[blockOffset + 11] << 8);
  const first = rgb565(c0);
  const second = rgb565(c1);
  const colors = [first, second,
    first.map((value, channel) => Math.round((2 * value + second[channel]) / 3)),
    first.map((value, channel) => Math.round((value + 2 * second[channel]) / 3))
  ];
  const colorBits = (bytes[blockOffset + 12] | (bytes[blockOffset + 13] << 8) |
    (bytes[blockOffset + 14] << 16) | (bytes[blockOffset + 15] << 24)) >>> 0;
  return [...colors[(colorBits >>> (index * 2)) & 3], alpha[alphaIndex]];
}

export async function decodeDdsThumbnail(file) {
  const headerBytes = await file.slice(0, 128).arrayBuffer();
  if (headerBytes.byteLength < 128) throw new Error('DDS 文件头不完整');
  const header = new DataView(headerBytes);
  if (u32(header, 0) !== 0x20534444 || u32(header, 4) !== 124) throw new Error('不是有效 DDS 文件');
  const width = u32(header, 16);
  const height = u32(header, 12);
  if (!width || !height || width > 32768 || height > 32768) throw new Error('DDS 尺寸无效');
  const fourCC = String.fromCharCode(...new Uint8Array(headerBytes, 84, 4));
  const rgba8 = u32(header, 88) === 32 && u32(header, 92) === 0xff &&
    u32(header, 96) === 0xff00 && u32(header, 100) === 0xff0000 &&
    u32(header, 104) === 0xff000000;
  if (fourCC !== 'DXT5' && !rgba8) throw new Error(`暂不支持预览 ${fourCC.trim() || '此 DDS 格式'}`);
  const levels = Math.max(1, Math.min(u32(header, 28) || 1, 16));
  let level = 0;
  while (level + 1 < levels && Math.max(Math.max(1, width >> level), Math.max(1, height >> level)) > MAX_SIDE) level++;
  let offset = 128;
  for (let i = 0; i < level; i++) {
    const w = Math.max(1, width >> i);
    const h = Math.max(1, height >> i);
    offset += rgba8 ? w * h * 4 : Math.ceil(w / 4) * Math.ceil(h / 4) * 16;
  }
  const sourceWidth = Math.max(1, width >> level);
  const sourceHeight = Math.max(1, height >> level);
  const size = rgba8 ? sourceWidth * sourceHeight * 4 : Math.ceil(sourceWidth / 4) * Math.ceil(sourceHeight / 4) * 16;
  if (size > MAX_READ_BYTES) throw new Error('DDS 过大且没有可用的缩小层');
  if (offset + size > file.size) throw new Error('DDS 图像数据不完整');
  const bytes = new Uint8Array(await file.slice(offset, offset + size).arrayBuffer());
  const scale = Math.min(1, MAX_SIDE / Math.max(sourceWidth, sourceHeight));
  const outWidth = Math.max(1, Math.round(sourceWidth * scale));
  const outHeight = Math.max(1, Math.round(sourceHeight * scale));
  const pixels = new Uint8ClampedArray(outWidth * outHeight * 4);
  const blockStride = Math.ceil(sourceWidth / 4);
  for (let y = 0; y < outHeight; y++) {
    const sy = Math.min(sourceHeight - 1, Math.floor((y + 0.5) * sourceHeight / outHeight));
    for (let x = 0; x < outWidth; x++) {
      const sx = Math.min(sourceWidth - 1, Math.floor((x + 0.5) * sourceWidth / outWidth));
      const color = rgba8
        ? bytes.subarray((sy * sourceWidth + sx) * 4, (sy * sourceWidth + sx) * 4 + 4)
        : bc3Pixel(bytes, (Math.floor(sy / 4) * blockStride + Math.floor(sx / 4)) * 16, sx, sy);
      pixels.set(color, (y * outWidth + x) * 4);
    }
  }
  return { width: outWidth, height: outHeight, pixels };
}
