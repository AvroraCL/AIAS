export const bakeDefaults = Object.freeze({ device: 0, resolution: 2048, samples: 128, margin: 16, bits: 8, distanceRatio: 0.1, selfOnly: false, ao: true, uv: true, id: true, output: '' });
export function restoreBakeSettings(value) {
  const source = value && typeof value === 'object' ? value : {};
  const out = { ...bakeDefaults };
  for (const [key, choices] of [['resolution', [512, 1024, 2048, 4096]], ['samples', [32, 64, 128, 256]], ['bits', [8, 16]]]) if (choices.includes(source[key])) out[key] = source[key];
  if (Number.isInteger(source.device) && source.device >= 0 && source.device < 64) out.device = source.device;
  if (Number.isInteger(source.margin) && source.margin >= 0 && source.margin <= 128) out.margin = source.margin;
  if (Number.isFinite(source.distanceRatio) && source.distanceRatio > 0) out.distanceRatio = source.distanceRatio;
  for (const key of ['selfOnly', 'ao', 'uv', 'id']) if (typeof source[key] === 'boolean') out[key] = source[key];
  if (typeof source.output === 'string') out.output = source.output;
  return out;
}
