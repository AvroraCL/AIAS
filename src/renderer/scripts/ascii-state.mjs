export const STANDARD = ' .:-=+*#%@';
export const DETAILED = ' .\'`^",:;Il!i~+_-?][}{1)(|\\/tfjrxnuvczXYUJCLQ0OZmwqpdbkhao*#MW&8%B@$';
export const DEFAULTS = { color: false, columns: 120, charset: 'standard', custom: STANDARD, brightness: 0, contrast: 1, invert: false, background: 'black', format: 'png', style: 'ascii', shapeSize: 92, shapeRatio: 100 };
export function restoreAsciiSettings(value = {}) {
  value = value && typeof value === 'object' ? value : {};
  const result = { ...DEFAULTS };
  for (const key of ['color', 'invert']) if (typeof value[key] === 'boolean') result[key] = value[key];
  for (const [key, min, max] of [['columns',40,240],['brightness',-1,1],['contrast',0,3],['shapeSize',20,100],['shapeRatio',50,200]]) {
    if (Number.isFinite(value[key]) && value[key] >= min && value[key] <= max) result[key] = ['columns','shapeSize','shapeRatio'].includes(key) ? Math.round(value[key]) : value[key];
  }
  for (const [key, allowed] of [['charset',['standard','detailed','custom']],['background',['black','white','transparent']],['format',['txt','png']],['style',['ascii','block','dot']]]) if (allowed.includes(value[key])) result[key] = value[key];
  if (typeof value.custom === 'string' && /^[\x20-\x7e]{2,95}$/.test(value.custom)) result.custom = value.custom;
  if (value.version !== 2) result.format = 'png';
  return result;
}
export function characterRamp(settings) {
  const ramp = settings.charset === 'custom' ? settings.custom : settings.charset === 'detailed' ? DETAILED : STANDARD;
  if (!/^[\x20-\x7e]{2,95}$/.test(ramp) || new Set(ramp).size < 2) throw Error('字符集需包含 2–95 个可打印 ASCII 字符，且至少有两个不同字符。');
  return ramp;
}
export function gridSize(width, height, columns, cellWidth, lineHeight) {
  if (![width,height,columns,cellWidth,lineHeight].every(n => Number.isFinite(n) && n > 0)) throw Error('图片尺寸无效。');
  let rows = Math.max(1, Math.round(columns * height / width * cellWidth / lineHeight));
  if (rows > 400) { columns = Math.max(1, Math.round(columns * 400 / rows)); rows = 400; }
  return { columns, rows };
}
// 采样检测解码像素是否带透明通道（立绘/贴纸类素材应保持透明底而非填充底色）。
export function detectTransparency(data) {
  if (!(data instanceof Uint8ClampedArray) || data.length < 4) return false;
  const pixels = data.length / 4;
  const step = Math.max(1, Math.floor(pixels / 4096));
  for (let p = 0; p < pixels; p += step) if (data[p * 4 + 3] < 255) return true;
  return false;
}
export function convertAscii({ pixels, columns, rows, settings }) {
  const graphic = settings.style === 'block' || settings.style === 'dot';
  const ramp = graphic ? ' ' : characterRamp(settings);
  if (columns < 1 || columns > 240 || rows < 1 || rows > 400 || pixels.length !== columns * rows * 4) throw Error('字符网格无效。');
  const colors = new Uint8Array(columns * rows * 3), alphas = new Uint8Array(columns * rows), lights = new Uint8Array(columns * rows), chars = [];
  const transparent = settings.background === 'transparent';
  const bg = settings.background === 'white' ? 255 : 0;
  for (let i = 0; i < columns * rows; i++) {
    const alpha = pixels[i * 4 + 3] / 255;
    const rgb = [0,1,2].map(c => transparent ? pixels[i * 4 + c] : Math.round(pixels[i * 4 + c] * alpha + bg * (1 - alpha)));
    alphas[i] = transparent ? pixels[i * 4 + 3] : 255;
    colors.set(rgb, i * 3);
    let light = (0.2126 * rgb[0] + 0.7152 * rgb[1] + 0.0722 * rgb[2]) / 255;
    light = Math.max(0, Math.min(1, (light - 0.5) * settings.contrast + 0.5 + settings.brightness));
    if (bg === 255) light = 1 - light;
    if (settings.invert) light = 1 - light;
    lights[i] = Math.round(light * 255);
    chars.push(!graphic && transparent && alpha === 0 ? ' ' : ramp[Math.round(light * (ramp.length - 1))]);
  }
  const lines = Array.from({length: rows}, (_, row) => chars.slice(row * columns, (row + 1) * columns).join(''));
  return { columns, rows, chars: chars.join(''), colors, alphas, lights, text: lines.join('\n') };
}
