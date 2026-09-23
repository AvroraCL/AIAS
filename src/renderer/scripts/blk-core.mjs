export function defaultBlkName(directory) {
  return directory.replace(/[\\/]+$/, '').split(/[\\/]/).pop() || '';
}

export function blkFileGroup(name) {
  if (/_c\.dds$/i.test(name)) return 'c';
  if (/_n\.dds$/i.test(name)) return 'n';
  return 'other';
}

export function createBlkRules(files) {
  return files.filter(name => /\.dds$/i.test(name)).sort((a, b) => a.localeCompare(b, undefined, { numeric: true })).map(to => ({
    to, from: to.replace(/\.dds$/i, '') + '*', enabled: true,
    command: blkFileGroup(to) === 'n' ? 'replace_tex' : '', camoSkinTex: false
  }));
}

export function validateBlk(name, files, rules) {
  const stem = name.replace(/\.blk$/i, '');
  if (!stem || stem === '.' || stem === '..' || /[<>:"/\\|?*\x00-\x1f]/.test(stem) || /[. ]$/.test(stem) || /^(con|prn|aux|nul|com[1-9]|lpt[1-9])(?:\.|$)/i.test(stem)) return 'BLK 文件名无效。';
  const available = new Set(files.map(file => file.toLowerCase()));
  const active = rules.filter(rule => rule.enabled);
  if (!active.length) return '请至少保留一条贴图规则。';
  const seen = new Set();
  for (const rule of active) {
    if (blkFileGroup(rule.to) === 'n' && rule.command !== 'replace_tex') return `${rule.to} 固定使用 replace_tex。`;
    if (!['replace_tex', 'set_tex'].includes(rule.command)) return `请为 ${rule.to} 选择规则指令。`;
    if (!rule.from || /["{}\r\n\x00-\x1f]/.test(rule.from)) return `请检查 ${rule.to} 的原贴图名。`;
    if (!available.has(rule.to.toLowerCase()) || !/^[^\\/"{}\r\n]+\.dds$/i.test(rule.to)) return `贴图 ${rule.to} 已不存在或文件名无效，请重新扫描。`;
    const key = rule.from.toLowerCase();
    if (seen.has(key)) return `原贴图名 ${rule.from} 重复。`;
    seen.add(key);
  }
  return null;
}

export function renderBlk(rules) {
  const lines = ['name:t="user"'];
  for (const rule of rules.filter(item => item.enabled)) {
    lines.push('', `${rule.command}{`, `  from:t="${rule.from}"`, `  to:t="${rule.to}"`);
    if (rule.command === 'set_tex' && rule.camoSkinTex) lines.push('  param:t="camo_skin_tex"');
    lines.push('}');
  }
  return lines.join('\r\n') + '\r\n';
}

export function diffBlk(before, after) {
  if (before === after) return '文本内容相同。';
  const oldLines = before.replace(/\r\n/g, '\n').trimEnd().split('\n');
  const newLines = after.replace(/\r\n/g, '\n').trimEnd().split('\n');
  return ['原文件：', ...oldLines.map(line => `− ${line}`), '', '新文件：', ...newLines.map(line => `+ ${line}`)].join('\n');
}
