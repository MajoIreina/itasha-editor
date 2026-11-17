import assert from 'node:assert/strict';
import {parsePaintMap} from '../smoke/paint-map-json.mjs';

const exactIds = parsePaintMap('{"triangles":[{"id":{"fragment":18446640630113940372}},{"id":{"fragment":18446640630113940373}}],"connections":[{"from":{"triangle":{"fragment":18446640630113940372}}}]}');
assert.notEqual(exactIds.triangles[0].id.fragment, exactIds.triangles[1].id.fragment);
assert.equal(exactIds.triangles[0].id.fragment, '18446640630113940372');
assert.equal(exactIds.connections[0].from.triangle.fragment, exactIds.triangles[0].id.fragment);
import {validateFullAuditEvidence} from '../smoke/whole-car-audit-evidence.mjs';

// Deliberately synthetic audit data: this exercises the evidence gate only.
// It does not certify any vehicle geometry or replace the real full audit.
const from = {sourceMesh: 'fixture', primitive: 0, sourceTriangle: 0, fragment: 0};
const to = {...from, sourceTriangle: 1};
const triangleParts = new Map([[JSON.stringify(from), 'a'], [JSON.stringify(to), 'b']]);
const map = {connections: Array.from({length: 730}, (_, i) => ({
  id: 'seam-' + i, propagation: true, from: {triangle: from}, to: {triangle: to},
}))};
const results = [];
for (const [index, seam] of map.connections.entries()) for (const direction of ['forward', 'reverse']) {
  for (const edgeFraction of [0.2, 0.5, 0.8]) {
    const cases = edgeFraction === 0.5 && index < 148
      ? ['small-square', 'wide-rotated', 'tall-rotated', 'large-square'] : ['small-square'];
    for (const variant of cases) results.push({seamId: seam.id, direction, edgeFraction, case: variant,
      pair: ['a', 'b'], source: direction === 'forward' ? 'a' : 'b', target: direction === 'forward' ? 'b' : 'a',
      crossingSucceeded: true, sourceUnchanged: true, finiteImageUv: true,
      physicalJacobianSingularErrorP95: 0.001, oppositeSideAreaMm2: 0,
      exteriorDirectionWitness: {usable: true, sameDominantHandedness: true}});
  }
}
const fixture = {mode: 'full', authoredSeams: 730, physicalPartPairs: 1, queries: 5268, results,
  representativeSeeds: results.filter(row => row.case === 'small-square' && row.edgeFraction === 0.5).map(row => structuredClone(row))};
assert.equal(validateFullAuditEvidence(map, fixture, triangleParts).queries, 5268);
const rejected = (name, mutate) => {
  const candidate = structuredClone(fixture);
  mutate(candidate);
  assert.throws(() => validateFullAuditEvidence(map, candidate, triangleParts), undefined, name);
};
rejected('reduced audit mode', audit => {audit.mode = 'stratified';});
rejected('reduced authored scope', audit => {audit.authoredSeams = 24;});
rejected('truncated result array', audit => {audit.results.pop();});
rejected('reported query count', audit => {audit.queries--;});
rejected('duplicate replaces missing fraction', audit => {audit.results[1] = structuredClone(audit.results[0]);});
rejected('missing small-square hidden by an extra case', audit => {audit.results[1].case = 'wide-rotated';});
rejected('unauthored seam', audit => {audit.results[0].seamId = 'unknown';});
rejected('wrong seam direction', audit => {audit.results[0].source = 'b';});
rejected('crossing failure outside selected seeds', audit => {audit.results[0].crossingSucceeded = false;});
rejected('changed source outside selected seeds', audit => {audit.results[0].sourceUnchanged = false;});
rejected('nonfinite image outside selected seeds', audit => {audit.results[0].finiteImageUv = false;});
rejected('metric error outside selected seeds', audit => {audit.results[0].physicalJacobianSingularErrorP95 = 0.010001;});
rejected('nonfinite metric error', audit => {audit.results[0].physicalJacobianSingularErrorP95 = NaN;});
rejected('opposite-side jump', audit => {audit.results[0].oppositeSideAreaMm2 = 0.100001;});
rejected('unusable exterior direction', audit => {audit.results[0].exteriorDirectionWitness.usable = false;});
rejected('flipped exterior handedness', audit => {audit.results[0].exteriorDirectionWitness.sameDominantHandedness = false;});
rejected('representative seed detached from evidence', audit => {audit.representativeSeeds[0].sourceCenter = [1, 2];});
const reducedMap = {...map, connections: map.connections.slice(0, 24)};
assert.throws(() => validateFullAuditEvidence(reducedMap, fixture, triangleParts));
console.log('Full-audit evidence gate: complete synthetic scope accepted; 18 corrupted/reduced cases rejected.');
