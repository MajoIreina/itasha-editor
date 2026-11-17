"""Add reviewed original-GLB hinge relations without changing retained paint masks.

The reviewed input selects source face pairs only. This tool independently proves
their exact GLB index/POSITION edge identity, exterior-side bend, stable retained
anchors, and both masks' common support in a one-micrometre physical source-edge
band. The band measures a propagation entrance; it never adds paint coverage.
"""
from __future__ import annotations

import argparse
from collections import Counter, defaultdict
import copy
from fractions import Fraction as F
import json
import math
from pathlib import Path
import runpy

import numpy as np

AUDIT = runpy.run_path(str(Path(__file__).with_name('audit-paint-integrity.py')))
SUPPORT_DISTANCE_MM = .001


def union_intervals(intervals):
    result = []
    for lo, hi in sorted(intervals):
        if hi <= lo:
            continue
        if not result or lo > result[-1][1]:
            result.append([lo, hi])
        else:
            result[-1][1] = max(result[-1][1], hi)
    return result


def clip_exact(polygon, signed_distance):
    if not polygon:
        return []
    output = []
    a, da = polygon[-1], signed_distance(polygon[-1])
    for b in polygon:
        db = signed_distance(b)
        if (da < 0) != (db < 0):
            t = da/(da-db)
            output.append(tuple(x+t*(y-x) for x, y in zip(a, b)))
        if db >= 0:
            output.append(b)
        a, da = b, db
    return output


def area2_exact(polygon):
    return abs(sum((a[1]*b[2]-a[2]*b[1]
                    for a, b in zip(polygon, polygon[1:]+polygon[:1])), F(0))) if len(polygon) > 2 else F(0)


def physical_mask_support(triangles, source_world, ordered_edge):
    """Clip exact normalized f32 source domains to a strictly inward edge band."""
    start, end = ordered_edge
    corners = [next(i for i, p in enumerate(source_world) if np.array_equal(p, endpoint))
               for endpoint in (start, end)]
    opposite = next(i for i in range(3) if i not in corners)
    a, b = [tuple(F(float(v)) for v in source_world[i]) for i in corners]
    c = tuple(F(float(v)) for v in source_world[opposite])
    direction = tuple(y-x for x, y in zip(a, b))
    offset = tuple(y-x for x, y in zip(a, c))
    squared = sum(v*v for v in direction)
    dot = sum(x*y for x, y in zip(offset, direction))
    gain = dot/squared
    altitude_squared = sum(v*v for v in offset)-dot*dot/squared
    altitude_mm = math.sqrt(float(altitude_squared))*1000.
    inward_limit = F(math.nextafter(SUPPORT_DISTANCE_MM/altitude_mm, 0.))
    # Prove the conservative binary bound against the exact squared distance,
    # rather than assuming the f64 square root rounded in a helpful direction.
    while inward_limit*inward_limit*altitude_squared > F(1, 10**12):
        inward_limit = F(math.nextafter(float(inward_limit), 0.))
    parameter = lambda row: row[corners[1]]+row[opposite]*gain
    intervals, records = [], []
    for triangle in triangles:
        raw = np.asarray(triangle['sourceBarycentric'], dtype=np.float32).astype(float)
        polygon = []
        for row in raw:
            rational = tuple(F(float(v)) for v in row)
            polygon.append(tuple(v/sum(rational) for v in rational))
        for coordinate in range(3):
            polygon = clip_exact(polygon, lambda p, coordinate=coordinate: p[coordinate])
        polygon = clip_exact(polygon, parameter)
        polygon = clip_exact(polygon, lambda p: 1-parameter(p))
        polygon = clip_exact(polygon, lambda p: inward_limit-p[opposite])
        area2 = area2_exact(polygon)
        if area2 == 0:
            continue
        lo, hi = min(parameter(p) for p in polygon), max(parameter(p) for p in polygon)
        if hi <= lo:
            continue
        intervals.append([lo, hi])
        records.append({'triangle': triangle['id'], 'bandSourceArea2Exact': str(area2),
                        'intervalParameterExact': [str(lo), str(hi)]})
    intervals = union_intervals(intervals)
    length_mm = float(np.linalg.norm(end-start)*1000.)
    return {'supportIntervalsParameterExact': [[str(lo), str(hi)] for lo, hi in intervals],
            'supportIntervalsMm': [[float(lo)*length_mm, float(hi)*length_mm] for lo, hi in intervals],
            'originalSourceAltitudeMm': altitude_mm,
            'inwardOppositeBarycentricLimitExact': str(inward_limit),
            'inwardDistanceSquaredProof': str(inward_limit*inward_limit*altitude_squared),
            'positiveAreaStripFragments': records}


def original_from_anchor(triangle):
    """Match the runtime's normalized-source affine reconstruction and guards."""
    bary = np.asarray(triangle['sourceBarycentric'], dtype=np.float32).astype(float)
    bary /= bary.sum(axis=1)[:, None]
    source = bary[:, 1:]
    matrix = np.column_stack((source[1]-source[0], source[2]-source[0]))
    determinant = float(np.linalg.det(matrix))
    world = np.asarray(triangle['world'], dtype=np.float32).astype(float)
    edges = [np.linalg.norm(world[j]-world[i]) for i, j in ((0, 1), (1, 2), (2, 0))]
    height = float(np.linalg.norm(np.cross(world[1]-world[0], world[2]-world[0]))/max(edges))
    if abs(determinant) <= 1.e-12 or not np.isfinite(height) or height <= 2.e-6:
        return None
    weights = (np.eye(3)[:, 1:]-source[0]) @ np.linalg.inv(matrix).T
    reconstructed = world[0]+weights[:, 0, None]*(world[1]-world[0])+weights[:, 1, None]*(world[2]-world[0])
    return reconstructed, abs(determinant), height


def stable_anchor(triangles, source_world):
    candidates = []
    for triangle in triangles:
        reconstruction = original_from_anchor(triangle)
        if reconstruction is None:
            continue
        world, determinant, height = reconstruction
        error = float(np.linalg.norm(world-source_world, axis=1).max())
        if np.isfinite(error) and error <= 5.e-6:
            candidates.append((error, -determinant, triangle, height))
    if not candidates:
        raise ValueError('No retained anchor reconstructs its original source face within 5 micrometres')
    error, negative_determinant, triangle, height = min(candidates, key=lambda c: c[:2])
    return triangle, {'sourceBarycentricDeterminant': -negative_determinant,
                      'retainedWorldHeightMm': height*1000.,
                      'originalFaceReconstructionMaxErrorMm': error*1000.}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--model', type=Path, required=True)
    parser.add_argument('--map', type=Path, required=True)
    parser.add_argument('--reviewed-pairs', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=False)
    document = json.loads(args.map.read_text('utf-8'))
    reviewed = json.loads(args.reviewed_pairs.read_text('utf-8'))
    model_hash = AUDIT['digest'](args.model)
    if document['modelFingerprint'] != model_hash or reviewed['modelSha256'] != model_hash:
        raise ValueError('Both the map and reviewed pairs must identify the exact source GLB')
    if any(c.get('sourceHinge') for c in document['connections']):
        raise ValueError('Input already contains source hinges; refuse duplicate additions')
    sources = {s['id']: s for s in AUDIT['Glb'](args.model).primitives()}
    by_face = defaultdict(list)
    for triangle in document['triangles']:
        identity = triangle['id']
        by_face[(triangle['partId'], identity['sourceMesh'], identity['primitive'], identity['sourceTriangle'])].append(triangle)
    connections, evidence = [], []
    seen = set()
    for selected in reviewed['sourceFacePairs']:
        a, b = tuple(selected['sourceA']), tuple(selected['sourceB'])
        pair = tuple(sorted((a, b)))
        if pair in seen:
            continue
        seen.add(pair)
        part = selected['partId']
        if a[:2] != b[:2] or a == b:
            raise ValueError('Reviewed hinge must join distinct faces of one original primitive')
        source = sources[a[0]]
        indices_a, indices_b = source['indices'][a[2]], source['indices'][b[2]]
        world_a, world_b = source['world'][a[2]], source['world'][b[2]]
        common_indices = set(map(int, indices_a)) & set(map(int, indices_b))
        same_positions = [(i, j) for i, pa in enumerate(world_a) for j, pb in enumerate(world_b)
                          if np.array_equal(pa, pb)]
        if len(same_positions) != 2:
            raise ValueError(f'Reviewed pair lacks exactly two identical source POSITION endpoints: {a}, {b}')
        corners_a, corners_b = [p[0] for p in same_positions], [p[1] for p in same_positions]
        edge_a = next(i for i in range(3) if {i, (i+1) % 3} == set(corners_a))
        edge_b = next(i for i in range(3) if {i, (i+1) % 3} == set(corners_b))
        hinge = world_a[[edge_a, (edge_a+1) % 3]]
        reversed_edge = np.array_equal(world_b[edge_b], hinge[1])
        if not np.array_equal(world_b[[edge_b, (edge_b+1) % 3]][::-1] if reversed_edge else world_b[[edge_b, (edge_b+1) % 3]], hinge):
            raise ValueError('Original edge orientation mismatch')
        direction = (hinge[1]-hinge[0])/np.linalg.norm(hinge[1]-hinge[0])
        def inward(point):
            delta = point-hinge[0]
            v = delta-direction*np.dot(delta, direction)
            return v/np.linalg.norm(v)
        interior_cosine = float(inward(world_a[(edge_a+2) % 3]) @ inward(world_b[(edge_b+2) % 3]))
        if not interior_cosine < 0.:
            raise ValueError('Original edge does not separate opposite exterior-side face interiors')
        members_a, members_b = by_face[(part, *a)], by_face[(part, *b)]
        if not members_a or not members_b:
            raise ValueError('Both source faces must retain positive-domain fragments of the same reviewed part')
        support_a = physical_mask_support(members_a, world_a, hinge)
        support_b = physical_mask_support(members_b, world_b, hinge)
        intervals_a, intervals_b = [[[F(x), F(y)] for x, y in side['supportIntervalsParameterExact']]
                                    for side in (support_a, support_b)]
        support_parameters = union_intervals([[max(x, u), min(y, v)]
                                      for x, y in intervals_a
                                      for u, v in intervals_b
                                      if min(y, v) > max(x, u)])
        length_mm = float(np.linalg.norm(hinge[1]-hinge[0])*1000.)
        # Round inwards: serialized f32 ranges must never expand the measured
        # support by half an ULP at either end.
        serialized = []
        for lo, hi in support_parameters:
            flo, fhi = np.float32(float(lo)), np.float32(float(hi))
            if F(float(flo)) < lo:
                flo = np.nextafter(flo, np.float32(np.inf))
            if F(float(fhi)) > hi:
                fhi = np.nextafter(fhi, np.float32(-np.inf))
            if fhi <= flo:
                raise ValueError(f'Positive exact support interval cannot fit an inward f32 range: {a}, {b}, {lo}, {hi}')
            serialized.append([float(flo), float(fhi)])
        if not serialized:
            raise ValueError(f'No positive common one-micrometre support in reviewed pair {a}, {b}')
        anchor_a, anchor_report_a = stable_anchor(members_a, world_a)
        anchor_b, anchor_report_b = stable_anchor(members_b, world_b)
        connection_id = f'original-source-hinge-v25/{part}/{a[0].split("/node/")[1].replace("/", "-")}/{a[2]}-{b[2]}'
        connection = {'id': connection_id,
                      'from': {'triangle': anchor_a['id'], 'edge': 0},
                      'to': {'triangle': anchor_b['id'], 'edge': 0},
                      'reversed': bool(reversed_edge), 'physicalGapMm': 0., 'propagation': True,
                      'sourceHinge': {'fromSourceEdge': edge_a, 'toSourceEdge': edge_b,
                                      'world': hinge.astype(np.float32).tolist(), 'supportIntervals': serialized}}
        connections.append(connection)
        evidence.append({'connectionId': connection_id, 'sourceA': a, 'sourceB': b, 'partId': part,
                         'classification': 'shared-original-index-edge' if len(common_indices) == 2 else 'exact-POSITION-edge-across-duplicate-indices',
                         'sourceIndicesA': indices_a.tolist(), 'sourceIndicesB': indices_b.tolist(),
                         'originalWorldEdge': hinge.tolist(), 'originalEdgeLengthMm': length_mm,
                         'sourceFaceInteriorSideCosine': interior_cosine,
                         'sideA': support_a, 'sideB': support_b,
                         'commonSupportIntervalsParameterExact': [[str(lo), str(hi)] for lo, hi in support_parameters],
                         'commonSupportIntervalsMm': [[float(lo)*length_mm, float(hi)*length_mm] for lo, hi in support_parameters],
                         'serializedSupportIntervals': serialized,
                         'fromAnchor': {'triangle': anchor_a['id'], **anchor_report_a},
                         'toAnchor': {'triangle': anchor_b['id'], **anchor_report_b}})
    candidate = copy.deepcopy(document)
    candidate['paintFingerprint'] = ''
    candidate['connections'].extend(connections)
    if candidate['triangles'] != document['triangles'] or candidate['connections'][:len(document['connections'])] != document['connections']:
        raise AssertionError('Original geometry, identities, charts, and seams must remain unchanged')
    AUDIT['write_json'](args.output/'paint-map-unsealed.json', candidate)
    report = {'status': 'source-hinges-awaiting-seal-and-runtime-integration-audit',
              'inputMap': str(args.map.resolve()), 'inputMapSha256': AUDIT['digest'](args.map),
              'inputPaintFingerprint': document['paintFingerprint'], 'sourceModelSha256': model_hash,
              'reviewedPairsPath': str(args.reviewed_pairs.resolve()), 'reviewedPairsSha256': AUDIT['digest'](args.reviewed_pairs),
              'trianglesUnchanged': True, 'oldConnectionsUnchanged': True,
              'oldConnections': len(document['connections']), 'newSourceHinges': len(connections),
              'classifications': dict(Counter(row['classification'] for row in evidence)),
              'supportDistanceMm': SUPPORT_DISTANCE_MM,
              'method': 'Exact GLB source identity and normalized stored-f32 source-domain Fractions, clipped to the original source simplex, original edge parameter [0,1], and a strictly inward <=1 micrometre band. The physical band limit is proved against exact rational squared distance. Only exact-positive strip areas contribute; every positive common projected interval is retained and rounded inwards to f32, otherwise generation fails. Runtime clips projected output to existing masks. The band does not fill a geometric or paint-mask gap.',
              'activeGeometryBridges': 0, 'pairs': evidence}
    AUDIT['write_json'](args.output/'source-hinge-report.json', report)
    print(json.dumps({k: v for k, v in report.items() if k not in ('pairs', 'method')}, ensure_ascii=True), flush=True)


if __name__ == '__main__':
    main()
