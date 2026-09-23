import test from 'node:test';
import assert from 'node:assert/strict';
import { blkFileGroup, bulkFromFill, createBlkRules, defaultBlkName, renderBlk, validateBlk } from './blk-core.mjs';

test('groups DDS by suffix and fixes N files to replace_tex', () => {
  assert.equal(blkFileGroup('tank_C.DDS'), 'c');
  assert.equal(blkFileGroup('tank_n.dds'), 'n');
  assert.equal(blkFileGroup('tank_mask.dds'), 'other');
  const files = ['tank_c.dds', 'tank_n.dds', 'tank_mask.dds'];
  const rules = createBlkRules(files);
  assert.equal(rules.find(rule => rule.to === 'tank_n.dds').command, 'replace_tex');
  assert.equal(rules.find(rule => rule.to === 'tank_c.dds').command, '');
  rules.find(rule => rule.to === 'tank_c.dds').command = 'set_tex';
  rules.find(rule => rule.to === 'tank_mask.dds').command = 'replace_tex';
  assert.equal(validateBlk('tank', files, rules), null);
  rules.find(rule => rule.to === 'tank_n.dds').command = 'set_tex';
  assert.match(validateBlk('tank', files, rules), /固定使用/);
});

test('matches the two-rule aircraft example and CRLF layout', () => {
  const files = ['mig_21_bis_finland_n.dds', 'mig_21_bis_finland_c.dds'];
  const rules = createBlkRules(files);
  assert.equal(rules[0].command, '');
  rules[0].command = 'set_tex';
  rules[1].command = 'replace_tex';
  const expected = [
    'name:t="user"', '',
    'set_tex{', '  from:t="mig_21_bis_finland_c*"', '  to:t="mig_21_bis_finland_c.dds"', '}', '',
    'replace_tex{', '  from:t="mig_21_bis_finland_n*"', '  to:t="mig_21_bis_finland_n.dds"', '}', ''
  ].join('\r\n');
  assert.equal(renderBlk(rules), expected);
  assert.equal(validateBlk('mig_21_bis_finland', files, rules), null);
});

test('includes extra DDS, optional set_tex param, and validates stale or duplicate mappings', () => {
  const files = ['body_c.dds', 'body_n.dds', 'jet_flame.dds'];
  const rules = createBlkRules(files);
  rules.forEach(rule => { rule.command = 'replace_tex'; });
  rules[0].command = 'set_tex';
  rules[0].camoSkinTex = true;
  assert.match(renderBlk(rules), /param:t="camo_skin_tex"/);
  assert.match(renderBlk(rules), /jet_flame\.dds/);
  rules.splice(1, 0, { ...rules[0], from: 'legacy_body_c*', command: 'replace_tex' });
  assert.equal(validateBlk('vehicle', files, rules), null);
  assert.equal((renderBlk(rules).match(/to:t="body_c\.dds"/g) || []).length, 2);
  rules[1].from = rules[0].from.toUpperCase();
  assert.match(validateBlk('vehicle', files, rules), /重复/);
  rules[1].from = 'body_n*';
  assert.match(validateBlk('vehicle', files.slice(1), rules), /不存在/);
  assert.match(validateBlk('../vehicle', files, rules), /文件名无效/);
  assert.equal(defaultBlkName('F:\\Game\\UserSkins\\vehicle\\'), 'vehicle');
});

test('bulkFromFill resets names by file name, stem, or clears them', () => {
  const rules = createBlkRules(['f_16xl_c.dds', 'f_16xl_n.dds', 'cockpit_glass.dds']);
  rules.forEach(rule => { rule.from = 'manual_name'; });
  const byName = bulkFromFill(rules, 'name');
  assert.deepEqual(byName.map(rule => rule.from), ['cockpit_glass*', 'f_16xl_c*', 'f_16xl_n*']);
  const byStem = bulkFromFill(rules, 'stem');
  assert.deepEqual(byStem.map(rule => rule.from), ['cockpit_glass*', 'f_16xl*', 'f_16xl*']);
  const cleared = bulkFromFill(rules, 'clear');
  assert.ok(cleared.every(rule => rule.from === ''));
  // 纯函数：不改动原数组
  assert.ok(rules.every(rule => rule.from === 'manual_name'));
});
