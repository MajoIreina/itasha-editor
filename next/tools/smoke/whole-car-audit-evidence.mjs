import assert from 'node:assert/strict';

const pairKey = pair => [...pair].sort().join('|');
const rowKey = row => JSON.stringify([row.seamId, row.direction, row.edgeFraction, row.case]);

// These are the reviewed SU7 full-audit scope, not a sampling allowance. A
// reduced candidate must fail here even if its remaining representative seeds
// happen to pass. Change this contract only with a new reviewed acceptance run.
const expectedSeams = 730;
const expectedQueries = 5268;

export function validateFullAuditEvidence(map, audit, triangleParts) {
  assert.equal(audit.mode, 'full', 'a full authored-seam metric audit is required');
  const seams = new Map();
  const connectedPairs = new Set();
  for (const connection of map.connections.filter(connection => connection.propagation)) {
    const from = triangleParts.get(JSON.stringify(connection.from.triangle));
    const to = triangleParts.get(JSON.stringify(connection.to.triangle));
    assert(from && to, 'authored seam refers to a missing map triangle');
    if (from === to) continue;
    assert.equal(typeof connection.id, 'string', 'authored seam needs a stable ID');
    assert(!seams.has(connection.id), 'duplicate authored seam ID: ' + connection.id);
    seams.set(connection.id, {from, to});
    connectedPairs.add(pairKey([from, to]));
  }
  assert.equal(seams.size, expectedSeams, 'restore the complete 730-seam SU7 acceptance scope');
  assert.equal(audit.authoredSeams, seams.size, 'audit authored-seam count differs from runtime map');
  assert.equal(audit.physicalPartPairs, connectedPairs.size, 'audit physical part-pair count differs from runtime map');
  assert(Array.isArray(audit.results), 'audit results are missing');
  assert.equal(audit.results.length, audit.queries, 'audit results.length differs from queries');
  assert.equal(audit.queries, expectedQueries, 'restore the complete 5268-query SU7 acceptance audit');

  const rows = new Map();
  const cases = new Set(['small-square', 'wide-rotated', 'tall-rotated', 'large-square']);
  for (const row of audit.results) {
    const label = rowKey(row);
    const seam = seams.get(row.seamId);
    assert(seam, 'audit query refers to an unauthored seam: ' + label);
    assert(row.direction === 'forward' || row.direction === 'reverse', 'unknown audit direction: ' + label);
    const [source, target] = row.direction === 'forward' ? [seam.from, seam.to] : [seam.to, seam.from];
    assert.equal(row.source, source, 'audit source differs from authored seam: ' + label);
    assert.equal(row.target, target, 'audit target differs from authored seam: ' + label);
    assert(Array.isArray(row.pair) && row.pair.length === 2 && pairKey(row.pair) === pairKey([source, target]),
      'audit pair differs from authored seam: ' + label);
    assert([0.2, 0.5, 0.8].includes(row.edgeFraction), 'unknown edge fraction: ' + label);
    assert(cases.has(row.case), 'unknown audit case: ' + label);
    assert(row.case === 'small-square' || row.edgeFraction === 0.5, 'stratified case must use the midpoint: ' + label);
    assert(!rows.has(label), 'duplicate audit query: ' + label);
    rows.set(label, row);
    assert.equal(row.crossingSucceeded, true, 'audited crossing failed: ' + label);
    assert.equal(row.sourceUnchanged, true, 'audited crossing changed source coverage: ' + label);
    assert.equal(row.finiteImageUv, true, 'audited image coordinates are not finite: ' + label);
    assert(Number.isFinite(row.physicalJacobianSingularErrorP95) && row.physicalJacobianSingularErrorP95 >= 0 &&
      row.physicalJacobianSingularErrorP95 <= 0.01, 'audited physical metric P95 exceeds 1%: ' + label);
    if (row.case === 'small-square') assert(Number.isFinite(row.oppositeSideAreaMm2) && row.oppositeSideAreaMm2 >= 0 &&
      row.oppositeSideAreaMm2 <= 0.1, 'small-square crossing jumps to opposite side: ' + label);
    assert.equal(row.exteriorDirectionWitness?.usable, true, 'audit lacks a usable exterior direction witness: ' + label);
    assert.equal(row.exteriorDirectionWitness?.sameDominantHandedness, true, 'audit reverses exterior image handedness: ' + label);
  }
  for (const seamId of seams.keys()) for (const direction of ['forward', 'reverse']) for (const edgeFraction of [0.2, 0.5, 0.8]) {
    const key = rowKey({seamId, direction, edgeFraction, case: 'small-square'});
    assert(rows.has(key), 'missing authored seam/direction/fraction small-square query: ' + key);
  }
  assert(Array.isArray(audit.representativeSeeds), 'representative seeds are missing');
  for (const seed of audit.representativeSeeds) {
    assert.equal(seed.case, 'small-square', 'representative seed must be a small-square query');
    assert.equal(seed.edgeFraction, 0.5, 'representative seed must use the midpoint');
    assert.deepEqual(seed, rows.get(rowKey(seed)), 'representative seed must match an audited result exactly');
  }
  return {connectedPairs, authoredSeams: seams.size, queries: rows.size};
}
