function trimDirectory(path) {
  const value = String(path || '');
  if (value === '/' || /^[A-Za-z]:[\\/]$/.test(value)) return value;
  return value.replace(/[\\/]+$/, '');
}

export function inputDirectory(input, inputIsDirectory = false) {
  const path = trimDirectory(input);
  if (!path) return '';
  if (inputIsDirectory) return path;
  const cut = Math.max(path.lastIndexOf('\\'), path.lastIndexOf('/'));
  if (cut < 0) return '';
  if (cut === 0 || /^[A-Za-z]:$/.test(path.slice(0, cut))) return path.slice(0, cut + 1);
  return path.slice(0, cut);
}

export function resolveOutputDirectory(stored, input, settings = {}, inputIsDirectory = false) {
  if (settings.outputStrategy === 'fixed') return trimDirectory(settings.defaultOutputDir);
  if (settings.outputStrategy === 'input') return inputDirectory(input, inputIsDirectory);
  return trimDirectory(stored);
}
