"""Audit source-domain triangle unions with exact rational vertical integration.

All stored barycentrics are first deserialized as IEEE f32, lifted to exact
dyadic rationals, normalized by the complete row sum, and clipped to the source
simplex. A rational segment arrangement partitions x into slabs; interval union
boundaries are linear inside each slab, so their area integrals and both directed
differences are exact. GLB world area is used only for the final units conversion.
This independently checks floating-point GEOS overlay on very thin source masks.
"""
from __future__ import annotations

import argparse
from collections import defaultdict
from fractions import Fraction as F
import json
from pathlib import Path
import runpy
import time

import numpy as np

AUDIT = runpy.run_path(str(Path(__file__).with_name('audit-paint-integrity.py')))


def clipped_polygon(triangle):
    bary = np.asarray(triangle['sourceBarycentric'], dtype=np.float32).astype(float)
    rows = [[F(float(value)) for value in row] for row in bary]
    polygon = [(row[1]/sum(row), row[2]/sum(row)) for row in rows]
    for signed_distance in (lambda p: p[0], lambda p: p[1], lambda p: 1-p[0]-p[1]):
        output = []
        for a, b in zip(polygon, polygon[1:]+polygon[:1]):
            da, db = signed_distance(a), signed_distance(b)
            if da >= 0:
                output.append(a)
            if (da < 0 < db) or (db < 0 < da):
                t = da/(da-db)
                output.append(tuple(x+t*(y-x) for x, y in zip(a, b)))
        polygon = output
        if len(polygon) < 3:
            return ()
    deduplicated = []
    for point in polygon:
        if not deduplicated or point != deduplicated[-1]:
            deduplicated.append(point)
    if deduplicated and deduplicated[-1] == deduplicated[0]:
        deduplicated.pop()
    if len(deduplicated) < 3 or polygon_area(deduplicated) == 0:
        return ()
    return tuple(deduplicated)


def polygon_area(polygon):
    return abs(sum(a[0]*b[1]-a[1]*b[0]
                   for a, b in zip(polygon, polygon[1:]+polygon[:1])))/2


def polygon_lines(polygon):
    result = []
    for a, b in zip(polygon, polygon[1:]+polygon[:1]):
        if a[0] == b[0]:
            continue
        slope = (b[1]-a[1])/(b[0]-a[0])
        result.append((min(a[0], b[0]), max(a[0], b[0]), slope, a[1]-slope*a[0]))
    return result


def integral(line, lo, hi):
    return line[0]*(hi*hi-lo*lo)/2+line[1]*(hi-lo)


def interval_union_area(intervals, mid, lo, hi):
    if not intervals:
        return F(0)
    evaluate = lambda line: line[0]*mid+line[1]
    ordered = sorted(intervals, key=lambda item: (evaluate(item[0]), evaluate(item[1])))
    low, high = ordered[0]
    result = F(0)
    for next_low, next_high in ordered[1:]:
        if evaluate(next_low) <= evaluate(high):
            if evaluate(next_high) > evaluate(high):
                high = next_high
        else:
            result += integral(high, lo, hi)-integral(low, lo, hi)
            low, high = next_low, next_high
    return result+integral(high, lo, hi)-integral(low, lo, hi)


def exact_areas(before, after):
    polygons = [(p, 0) for p in before if p]+[(p, 1) for p in after if p]
    if not polygons:
        return F(0), F(0), F(0), 0
    line_groups = [(polygon_lines(polygon), label) for polygon, label in polygons]
    edges = list(set(line for lines, _ in line_groups for line in lines))
    events = {point[0] for polygon, _ in polygons for point in polygon}
    for i, a in enumerate(edges):
        for b in edges[i+1:]:
            if a[2] == b[2]:
                continue
            left, right = max(a[0], b[0]), min(a[1], b[1])
            if right <= left:
                continue
            x = (b[3]-a[3])/(a[2]-b[2])
            if left < x < right:
                events.add(x)
    ordered = sorted(events)
    areas = [F(0), F(0), F(0)]
    for lo, hi in zip(ordered, ordered[1:]):
        mid = (lo+hi)/2
        intervals = [[], []]
        for lines, label in line_groups:
            active = [(line[2], line[3]) for line in lines if line[0] < mid < line[1]]
            if len(active) < 2:
                continue
            active.sort(key=lambda line: line[0]*mid+line[1])
            intervals[label].append((active[0], active[-1]))
        for i, collection in enumerate((intervals[0], intervals[1], intervals[0]+intervals[1])):
            areas[i] += interval_union_area(collection, mid, lo, hi)
    return *areas, len(ordered)-1


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--model', type=Path, required=True)
    parser.add_argument('--before', type=Path, required=True)
    parser.add_argument('--after', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--face', nargs=2, action='append', metavar=('SOURCE_MESH', 'FACE'))
    parser.add_argument('--differences', type=Path, help='Restrict to source faces listed by an earlier overlay audit')
    args = parser.parse_args()
    began = time.time()
    args.output.mkdir(parents=True, exist_ok=False)
    documents = [json.loads(path.read_text('utf-8')) for path in (args.before, args.after)]
    model_hash = AUDIT['digest'](args.model)
    if any(document['modelFingerprint'] != model_hash for document in documents):
        raise ValueError('Both maps must identify the exact original source model')
    groups = []
    for document in documents:
        by_face = defaultdict(list)
        for triangle in document['triangles']:
            by_face[(triangle['id']['sourceMesh'], triangle['id']['sourceTriangle'])].append(triangle)
        groups.append(by_face)
    keys = set(groups[0]) | set(groups[1])
    if args.face:
        keys &= {(mesh, int(face)) for mesh, face in args.face}
    if args.differences:
        previous = json.loads(args.differences.read_text('utf-8'))
        keys &= {(row['sourceMesh'], row['face']) for row in previous['sourceFaceDifferences']}
    sources = {s['id']: s for s in AUDIT['Glb'](args.model).primitives()}
    differences, summed = [], defaultdict(float)
    for ordinal, face_key in enumerate(sorted(keys)):
        polygons = [[clipped_polygon(t) for t in group.get(face_key, [])] for group in groups]
        before, after, combined, slabs = exact_areas(*polygons)
        lost, gained = combined-after, combined-before
        if lost < 0 or gained < 0:
            raise AssertionError('An exact union cannot have a negative directed difference')
        duplicate = sum((polygon_area(p) for p in polygons[1] if p), F(0))-after
        scale = 2*float(AUDIT['area3'](sources[face_key[0]]['world'][face_key[1]]))
        values = {'lostSourceAreaM2': float(lost)*scale, 'gainedSourceAreaM2': float(gained)*scale,
                  'beforeUnionAreaM2': float(before)*scale, 'afterUnionAreaM2': float(after)*scale,
                  'afterDuplicateAreaM2': float(duplicate)*scale}
        for name, value in values.items():
            summed[name] += value
        if lost > 0 or gained > 0 or duplicate > 0 or args.face:
            differences.append({'sourceMesh': face_key[0], 'face': face_key[1], **values,
                                'rationalSlabs': slabs, 'beforeTriangles': len(polygons[0]), 'afterTriangles': len(polygons[1])})
        if ordinal % 1000 == 0:
            print(f'exact source union {ordinal}/{len(keys)}', flush=True)
    report = {'status': 'exact-rational-normalized-f32-source-domain-union',
              'sourceModelSha256': model_hash,
              'before': {'path': str(args.before.resolve()), 'sha256': AUDIT['digest'](args.before)},
              'after': {'path': str(args.after.resolve()), 'sha256': AUDIT['digest'](args.after)},
              'method': __doc__, 'sourceFacesAudited': len(keys), 'restrictedByFaces': args.face,
              'restrictedByEarlierDifferences': str(args.differences) if args.differences else None,
              **summed, 'elapsedSeconds': time.time()-began,
              'sourceFaceDifferences': sorted(differences, key=lambda row: row['lostSourceAreaM2']+row['gainedSourceAreaM2'], reverse=True)}
    AUDIT['write_json'](args.output/'exact-source-union.json', report)
    print(json.dumps({k: v for k, v in report.items() if k not in ('method', 'sourceFaceDifferences')}, ensure_ascii=True), flush=True)


if __name__ == '__main__':
    main()
