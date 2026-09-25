export const bakeWorkspaceDefaults = Object.freeze({
  outlinerOpen: true,
  settingsOpen: true,
  projection: 'perspective',
  wireframe: false,
  mapPreview: 'ao',
});

export const bakeDefaults = Object.freeze({
  device: 0,
  deviceLuid: '',
  uvMode: 'preserveValid',
  resolution: 2048,
  samples: 128,
  margin: 16,
  bits: 8,
  distanceRatio: 0.1,
  aoDistanceRatio: 0.01,
  selfOnly: false,
  ao: true,
  normal: true,
  worldNormal: true,
  curvature: true,
  position: true,
  thickness: true,
  uv: false,
  id: true,
  denoise: false,
  workspace: bakeWorkspaceDefaults,
});

export function restoreBakeSettings(value) {
  const source = value && typeof value === 'object' ? value : {};
  const out = { ...bakeDefaults, workspace: { ...bakeWorkspaceDefaults } };
  for (const [key, choices] of [['resolution', [512, 1024, 2048, 4096]], ['samples', [32, 64, 128, 256]], ['bits', [8, 16]]]) if (choices.includes(source[key])) out[key] = source[key];
  if (Number.isInteger(source.device) && source.device >= 0 && source.device < 64) out.device = source.device;
  if (typeof source.deviceLuid === 'string' && source.deviceLuid.length <= 64) out.deviceLuid = source.deviceLuid;
  if (['preserveValid', 'regenerateAll', 'strictSource'].includes(source.uvMode)) out.uvMode = source.uvMode;
  if (Number.isInteger(source.margin) && source.margin >= 0 && source.margin <= 128) out.margin = source.margin;
  if (Number.isFinite(source.distanceRatio) && source.distanceRatio > 0) {
    out.distanceRatio = source.distanceRatio;
    // 旧版共用一个距离：只为显式自定义的旧值保留 AO 行为。旧默认 10%
    // 在密集模型上过暗，迁移为新的局部 AO 默认 1%；厚度距离不变。
    if (source.distanceRatio !== 0.1) out.aoDistanceRatio = source.distanceRatio;
  }
  if (Number.isFinite(source.aoDistanceRatio) && source.aoDistanceRatio > 0) out.aoDistanceRatio = source.aoDistanceRatio;
  for (const key of ['selfOnly', 'ao', 'normal', 'worldNormal', 'curvature', 'position', 'thickness', 'uv', 'id', 'denoise']) if (typeof source[key] === 'boolean') out[key] = source[key];
  const workspace = source.workspace && typeof source.workspace === 'object' ? source.workspace : {};
  for (const key of ['outlinerOpen', 'settingsOpen', 'wireframe']) {
    if (typeof workspace[key] === 'boolean') out.workspace[key] = workspace[key];
  }
  if (workspace.projection === 'perspective' || workspace.projection === 'orthographic') {
    out.workspace.projection = workspace.projection;
  }
  if (['material', 'ao', 'ao_unique', 'normal', 'world_normal', 'world_normal_unique', 'curvature', 'curvature_unique', 'position', 'position_unique', 'thickness', 'thickness_unique', 'id', 'uv', 'uv_unique_mask'].includes(workspace.mapPreview)) out.workspace.mapPreview = workspace.mapPreview;
  return out;
}

export function selectReliablePreviewFiles(files, kind) {
  if (!['ao', 'curvature', 'thickness', 'world_normal', 'position'].includes(kind)) return [];
  const byMaterial = new Map();
  for (const file of files) if (file.kind === kind) byMaterial.set(file.material, file);
  for (const file of files) if (file.kind === `${kind}_unique`) byMaterial.set(file.material, file);
  return [...byMaterial.values()];
}

export function assessUvCoverage(items) {
  const covered = (Array.isArray(items) ? items : []).filter(item => Number(item.coveredPixels) > 0);
  const shared = covered.filter(item => Number(item.sharedPixels) > 0);
  return {
    shared: shared.length,
    noReliablePixels: shared.filter(item => Number(item.sharedPixels) >= Number(item.coveredPixels)).length,
  };
}
