export const bakeWorkspaceDefaults = Object.freeze({
  outlinerOpen: true,
  settingsOpen: true,
  projection: 'perspective',
  grid: true,
  axes: true,
  wireframe: false,
});

export const bakeDefaults = Object.freeze({
  device: 0,
  resolution: 2048,
  samples: 128,
  margin: 16,
  bits: 8,
  distanceRatio: 0.1,
  selfOnly: false,
  ao: true,
  uv: true,
  id: true,
  denoise: false,
  workspace: bakeWorkspaceDefaults,
});

export function restoreBakeSettings(value) {
  const source = value && typeof value === 'object' ? value : {};
  const out = { ...bakeDefaults, workspace: { ...bakeWorkspaceDefaults } };
  for (const [key, choices] of [['resolution', [512, 1024, 2048, 4096]], ['samples', [32, 64, 128, 256]], ['bits', [8, 16]]]) if (choices.includes(source[key])) out[key] = source[key];
  if (Number.isInteger(source.device) && source.device >= 0 && source.device < 64) out.device = source.device;
  if (Number.isInteger(source.margin) && source.margin >= 0 && source.margin <= 128) out.margin = source.margin;
  if (Number.isFinite(source.distanceRatio) && source.distanceRatio > 0) out.distanceRatio = source.distanceRatio;
  for (const key of ['selfOnly', 'ao', 'uv', 'id', 'denoise']) if (typeof source[key] === 'boolean') out[key] = source[key];
  const workspace = source.workspace && typeof source.workspace === 'object' ? source.workspace : {};
  for (const key of ['outlinerOpen', 'settingsOpen', 'grid', 'axes', 'wireframe']) {
    if (typeof workspace[key] === 'boolean') out.workspace[key] = workspace[key];
  }
  if (workspace.projection === 'perspective' || workspace.projection === 'orthographic') {
    out.workspace.projection = workspace.projection;
  }
  return out;
}
