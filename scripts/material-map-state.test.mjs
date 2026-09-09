import { test } from 'node:test';
import assert from 'node:assert/strict';
import { createPreviewQueue, restoreMapSettings } from '../src/renderer/scripts/material-map-state.mjs';
const tick = () => new Promise(resolve => setTimeout(resolve, 5));
test('old settings restore independent defaults and reject corrupt parameters', () => {
  const settings=restoreMapSettings({normal:{parameters:{bits:12,strength:Infinity,channel:'bad',smoothing:21}}});
  assert.equal(settings.normal.parameters.bits,16);assert.equal(settings.normal.parameters.strength,1);
  assert.equal(settings.normal.parameters.channel,'luminance');assert.equal(settings.normal.parameters.smoothing,20);
  settings.normal.parameters.invert=true;assert.equal(settings.height.parameters.invert,false);
});
test('preview coalesces edits and ignores an obsolete in-flight result', async () => {
  const calls=[],ready=[];let resolve;
  const queue=createPreviewQueue({delay:0,run:input=>{calls.push(input);return new Promise(r=>resolve=r)},ready:value=>ready.push(value),failed:()=>assert.fail('unexpected failure')});
  queue.request('first');await tick();queue.request('middle');queue.request('last');await tick();
  assert.deepEqual(calls,['first']);resolve('old');await tick();assert.deepEqual(ready,[]);assert.deepEqual(calls,['first','last']);
  resolve('new');await tick();assert.deepEqual(ready,['new']);queue.dispose();
});
test('invalidate discards stale errors and settle prevents pending work', async () => {
  let reject;let calls=0;const errors=[];
  const queue=createPreviewQueue({delay:0,run:()=>{calls++;return new Promise((_,r)=>reject=r)},ready:()=>assert.fail('stale'),failed:e=>errors.push(e)});
  queue.request('one');await tick();queue.request('two');await tick();const settled=queue.settle();reject(Error('obsolete'));await settled;await tick();
  assert.equal(calls,1);assert.deepEqual(errors,[]);queue.dispose();
});
