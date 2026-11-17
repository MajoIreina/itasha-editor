import assert from 'node:assert/strict';
import fs from 'node:fs';
import { wholeLayout } from '../../src/features/editor/wholeLayout.ts';

const report = JSON.parse(fs.readFileSync(new URL('../../assets/paint/su7-v15-charge-cover/compact-report.json', import.meta.url)));
const dimensions = Object.fromEntries(report.parts.map(p => [p.partId, { widthMm: p.dimensionsMm[0], heightMm: p.dimensionsMm[1] }]));
const surfaces = Object.entries(dimensions).map(([id, size]) => ({ id, ...size }));
const { tiles, reference } = wholeLayout(surfaces);
const tile = name => tiles.find(t => t.surface.id === 'su7.' + name);
assert.equal(tiles.length, surfaces.length, 'every loaded part must remain visible');
assert.equal(new Set(tiles.map(t => t.surface.id)).size, surfaces.length);
for (const a of tiles) {
  assert(Number.isFinite(a.x) && Number.isFinite(a.y));
  for (const b of tiles) {
    if (a === b) continue;
    const overlapX = Math.min(a.x + a.surface.widthMm, b.x + b.surface.widthMm) - Math.max(a.x, b.x);
    const overlapY = Math.min(a.y + a.surface.heightMm, b.y + b.surface.heightMm) - Math.max(a.y, b.y);
    assert(overlapX <= 0 || overlapY <= 0, `${a.surface.id} overlaps ${b.surface.id}`);
  }
}
assert(tile('front-bumper').y < tile('hood').y);
assert(tile('hood').y < reference.y);
assert(tile('trunk').y > reference.y + reference.height);
for (const row of ['front', 'rear']) {
  assert(tile(`${row}-left-door`).x < reference.x);
  assert(tile(`${row}-right-door`).x > reference.x + reference.width);
}
assert(tile('front-left-door').y < tile('rear-left-door').y);
assert(tile('rear-left-fender').x < tile('front-left-door').x);
assert(tile('rear-right-fender').x > tile('front-right-door').x);
const extra = { id: 'other.extra', widthMm: 300, heightMm: 200 };
assert(wholeLayout([...surfaces, extra]).tiles.some(t => t.surface === extra));
console.log(`Passed: all ${tiles.length} parts retained; non-overlapping front-up layout, central cabin, paired doors, outboard one-piece quarters and additional parts.`);
