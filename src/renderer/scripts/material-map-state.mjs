export const mapDefaults = Object.freeze({ channel: 'luminance', smoothing: 0, contrast: 1,
  invert: false, strength: 1, convention: 'opengl', boundary: 'clamp', bits: 16, alsoHeight: false });

export function restoreMapSettings(value = {}) {
  const result = {};
  for (const kind of ['normal', 'height']) {
    const saved = value?.[kind] || {};
    const p = { ...mapDefaults, ...(saved.parameters || {}) };
    for (const [key, min, max] of [['smoothing', 0, 20], ['contrast', 0, 4], ['strength', 0, 10]]) {
      p[key] = Number.isFinite(Number(p[key])) ? Math.min(max, Math.max(min, Number(p[key]))) : mapDefaults[key];
    }
    p.smoothing = Math.round(p.smoothing);
    for (const [key, values] of Object.entries({ channel: ['luminance', 'r', 'g', 'b', 'alpha'], convention: ['opengl', 'directx'], boundary: ['clamp', 'wrap'] })) {
      if (!values.includes(p[key])) p[key] = mapDefaults[key];
    }
    p.bits = Number(p.bits) === 8 ? 8 : 16;
    p.invert = p.invert === true; p.alsoHeight = p.alsoHeight === true;
    result[kind] = { parameters: p, outputPath: typeof saved.outputPath === 'string' ? saved.outputPath : '' };
  }
  return result;
}

// Latest-request scheduler: only one expensive preview in flight, with stale
// completions/errors discarded even while the next request is still debouncing.
export function createPreviewQueue({ run, ready, failed, delay = 150 }) {
  let revision = 0, timer, pending = null, active = null, disposed = false;
  async function pump() {
    if (active || !pending || disposed) return;
    const request = pending; pending = null;
    active = Promise.resolve().then(() => run(request.input));
    try {
      const value = await active;
      if (!disposed && request.id === revision) await ready(value);
    } catch (error) {
      if (!disposed && request.id === revision) failed(error);
    } finally { active = null; pump(); }
  }
  return {
    request(input) {
      const id = ++revision; clearTimeout(timer); pending = null;
      timer = setTimeout(() => { pending = { id, input }; pump(); }, delay);
    },
    invalidate() { ++revision; clearTimeout(timer); pending = null; },
    async settle() { this.invalidate(); if (active) await active.catch(() => {}); },
    dispose() { disposed = true; this.invalidate(); },
  };
}
