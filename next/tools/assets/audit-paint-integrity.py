"""Read-only SU7 paint completeness audit against independent source geometry.

Geometry stage (normal Python, numpy + shapely):
  python tools/assets/audit-paint-integrity.py --model BODY --map MAP
    --baseline PORTABLE_GLB --sdk SDK_JSON --output NEW_DIRECTORY
Visibility stage (Blender's Python, numpy + mathutils):
  blender -b --factory-startup --python tools/assets/audit-paint-integrity.py --
    --visibility --model BODY --output EXISTING_GEOMETRY_DIRECTORY

Neither stage changes input files. Existing output files are never overwritten.
Coverage uses the union of fragment polygons in each source triangle's
barycentric domain, rather than summed mapped areas or the map's connection graph.
Visibility witnesses are independent BVH rays into the actual assembled GLBs.
They are sample evidence, not semantic approval or a proof of no omissions.
"""
from __future__ import annotations

import argparse
from collections import Counter, defaultdict
from fractions import Fraction
import hashlib
import itertools
import json
import math
from pathlib import Path
import struct
import sys
import time

import numpy as np


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def write_json(path, document):
    with path.open('x', encoding='utf-8') as stream:
        json.dump(document, stream, ensure_ascii=False, separators=(',', ':'))


def area3(points):
    return np.linalg.norm(np.cross(points[..., 1, :] - points[..., 0, :],
                                   points[..., 2, :] - points[..., 0, :]), axis=-1) / 2


def source_polygon(triangle):
    """Normalize all homogeneous barycentric rows before source-domain union.

Near-collinear dyadic rows use an exact rational determinant, so a source edge
cannot acquire area merely from JSON/f32 row-sum or world-coordinate rounding.
"""
    from shapely.geometry import Polygon
    bary=np.asarray(triangle['sourceBarycentric'],dtype=float)
    determinant=float(np.linalg.det(bary))
    if abs(determinant)<1.e-10:
        b=[[Fraction(float(v)) for v in row] for row in bary]
        exact=(b[0][0]*(b[1][1]*b[2][2]-b[1][2]*b[2][1])
               -b[0][1]*(b[1][0]*b[2][2]-b[1][2]*b[2][0])
               +b[0][2]*(b[1][0]*b[2][1]-b[1][1]*b[2][0]))
        if exact==0:
            return Polygon()
    normalized=bary/bary.sum(axis=1)[:,None]
    for opposite in range(3):
        mask=bary[:,opposite]==0.
        a,b=[i for i in range(3) if i!=opposite]
        normalized[mask,b]=1.-normalized[mask,a]
    return Polygon(normalized[:,1:])


class Glb:
    def __init__(self, path):
        self.path = Path(path)
        raw = self.path.read_bytes()
        if struct.unpack_from('<III', raw) != (0x46546c67, 2, len(raw)):
            raise ValueError(f'Invalid GLB: {path}')
        self.document, self.binary = None, None
        offset = 12
        while offset < len(raw):
            length, kind = struct.unpack_from('<II', raw, offset)
            chunk = raw[offset + 8:offset + 8 + length]
            if kind == 0x4e4f534a:
                self.document = json.loads(chunk)
            elif kind == 0x004e4942:
                self.binary = chunk
            offset += 8 + length
        if self.document is None or self.binary is None:
            raise ValueError('JSON/BIN chunks are required')

    def accessor(self, index):
        a = self.document['accessors'][index]
        if 'sparse' in a or a.get('normalized'):
            raise ValueError('Unsupported accessor; do not silently approximate')
        v = self.document['bufferViews'][a['bufferView']]
        dtype = np.dtype({5120: 'i1', 5121: 'u1', 5122: '<i2',
                          5123: '<u2', 5125: '<u4', 5126: '<f4'}[a['componentType']])
        width = {'SCALAR': 1, 'VEC2': 2, 'VEC3': 3, 'VEC4': 4}[a['type']]
        return np.ndarray((a['count'], width), dtype=dtype, buffer=self.binary,
                          offset=v.get('byteOffset', 0) + a.get('byteOffset', 0),
                          strides=(v.get('byteStride', dtype.itemsize * width),
                                   dtype.itemsize)).copy()

    def primitives(self):
        doc = self.document

        def visit(index, parent):
            node = doc['nodes'][index]
            if 'matrix' in node:
                local = np.asarray(node['matrix'], dtype=float).reshape(4, 4).T
            else:
                x, y, z, w = node.get('rotation', [0, 0, 0, 1])
                local = np.eye(4)
                local[:3, :3] = np.array([
                    [1-2*(y*y+z*z), 2*(x*y-z*w), 2*(x*z+y*w)],
                    [2*(x*y+z*w), 1-2*(x*x+z*z), 2*(y*z-x*w)],
                    [2*(x*z-y*w), 2*(y*z+x*w), 1-2*(x*x+y*y)]]) @ np.diag(node.get('scale', [1, 1, 1]))
                local[:3, 3] = node.get('translation', [0, 0, 0])
            matrix = parent @ local
            if 'mesh' in node:
                for pi, primitive in enumerate(doc['meshes'][node['mesh']]['primitives']):
                    if primitive.get('mode', 4) != 4:
                        raise ValueError('Non-triangle primitive')
                    positions = self.accessor(primitive['attributes']['POSITION'])
                    world = positions @ matrix[:3, :3].T + matrix[:3, 3]
                    indices = self.accessor(primitive['indices']).reshape(-1, 3).astype(np.int64)
                    # Exactly the runtime loader's winding rule; sourceTriangle order stays intact.
                    if np.linalg.det(matrix) < 0:
                        indices = indices[:, [0, 2, 1]]
                    material = doc['materials'][primitive.get('material', 0)]
                    yield {'id': f'{self.path.name}/node/{index}/primitive/{pi}',
                           'node': index, 'name': node.get('name', ''), 'primitive': pi,
                           'material': material.get('name', ''),
                           'paint': bool(material.get('extras', {}).get('native_paint_switch')),
                           'positions': world, 'indices': indices, 'world': world[indices]}
            for child in node.get('children', []):
                yield from visit(child, matrix)

        for root in doc['scenes'][doc.get('scene', 0)]['nodes']:
            yield from visit(root, np.eye(4))


def components(indices):
    parent = list(range(int(indices.max()) + 1))

    def root(i):
        while parent[i] != i:
            parent[i] = parent[parent[i]]
            i = parent[i]
        return i

    for a, b, c in indices:
        ra, rb, rc = root(int(a)), root(int(b)), root(int(c))
        parent[rb] = ra
        parent[root(rc)] = root(ra)
    groups = defaultdict(list)
    for face, (a, _, _) in enumerate(indices):
        groups[root(int(a))].append(face)
    return sorted(groups.values(), key=len, reverse=True)


def triangle_keys(world, scale=1e5):
    result = Counter()
    for triangle in world:
        result[tuple(sorted(tuple(int(round(v * scale)) for v in p) for p in triangle))] += 1
    return result


def geometry(args):
    from shapely.geometry import Polygon
    from shapely.ops import unary_union

    args.output.mkdir(parents=True, exist_ok=False)
    began = time.time()
    model = Glb(args.model)
    primitives = list(model.primitives())
    sources = {p['id']: p for p in primitives}
    mapping = json.loads(args.map.read_text(encoding='utf-8'))
    body_hash = digest(args.model)
    if mapping['modelFingerprint'] != body_hash:
        raise ValueError('Model/map fingerprint mismatch')
    grouped, by_part = defaultdict(list), defaultdict(list)
    errors, outliers = [], []
    source_error_max = 0.
    invalid_barycentric = 0
    for triangle in mapping['triangles']:
        identity = triangle['id']
        key = (identity['sourceMesh'], identity['sourceTriangle'])
        source = sources.get(key[0])
        if source is None or identity['primitive'] != source['primitive'] or not 0 <= key[1] < len(source['indices']):
            errors.append(identity)
            continue
        bary = np.asarray(triangle['sourceBarycentric'], dtype=float)
        expected = bary @ source['world'][key[1]]
        error = float(np.linalg.norm(expected - triangle['world'], axis=1).max())
        source_error_max = max(source_error_max, error)
        if error > .0001:
            outliers.append({'id': identity, 'maxErrorMm': error * 1000})
        if not np.all(np.isfinite(bary)) or np.max(np.abs(bary.sum(axis=1) - 1)) > .002 or bary.min() < -.002 or bary.max() > 1.002:
            invalid_barycentric += 1
        grouped[key].append(triangle)
        by_part[triangle['partId']].append(triangle)

    simplex = Polygon([(0., 0.), (1., 0.), (0., 1.)])
    fractions, overlap_fractions, probes, uncovered_faces = {}, {}, [], []
    area_by_key = {}
    for primitive in primitives:
        if not primitive['paint']:
            continue
        areas = area3(primitive['world'])
        for face, source_area in enumerate(areas):
            key = (primitive['id'], face)
            area_by_key[key] = float(source_area)
            fragments = grouped.get(key, [])
            polygons = [source_polygon(t) for t in fragments]
            valid = [p for p in polygons if p.is_valid and not p.is_empty and p.area > 0]
            merged = unary_union(valid).intersection(simplex) if valid else Polygon()
            fraction = min(1., max(0., merged.area * 2))
            fractions[key] = fraction
            overlap_fractions[key] = max(0., sum(p.intersection(simplex).area for p in valid) * 2 - fraction)
            if source_area < 1e-12 or fraction >= 1 - 1e-7:
                continue
            missing = simplex.difference(merged)
            polygons_missing = [missing] if missing.geom_type == 'Polygon' else list(missing.geoms)
            record = {'sourceMesh': key[0], 'face': face, 'name': primitive['name'],
                      'sourceAreaM2': float(source_area), 'coveredFraction': fraction,
                      'uncoveredAreaM2': float(source_area * (1-fraction))}
            uncovered_faces.append(record)
            corners = primitive['world'][face]
            normal = np.cross(corners[1]-corners[0], corners[2]-corners[0])
            normal /= max(float(np.linalg.norm(normal)), 1e-30)
            for polygon in polygons_missing:
                if polygon.area * source_area * 2 < 1e-12:
                    continue
                center = np.asarray(polygon.representative_point().coords[0])
                # Several interior probes make edge omissions visible as well as face centers.
                ring = np.asarray(polygon.exterior.coords[:-1])
                picks = [center]
                for vertex in ring[::max(1, math.ceil(len(ring)/3))]:
                    point = center * .55 + vertex * .45
                    if polygon.covers(__import__('shapely').geometry.Point(point)):
                        picks.append(point)
                weights = np.array([[1-p[0]-p[1], p[0], p[1]] for p in picks])
                probes.append({'sourceMesh': key[0], 'face': face, 'name': primitive['name'],
                               'uncoveredAreaM2': float(polygon.area * source_area * 2),
                               'points': (weights @ corners).tolist(), 'normal': normal.tolist()})
        print(f'coverage {primitive["name"]}: {len(areas)} source faces', flush=True)

    component_report, mesh_report = [], []
    for primitive in primitives:
        if not primitive['paint']:
            continue
        keys = [(primitive['id'], face) for face in range(len(primitive['indices']))]
        mesh_report.append({'sourceMesh': primitive['id'], 'name': primitive['name'],
                            'sourceFaces': len(keys), 'sourceAreaM2': sum(area_by_key[k] for k in keys),
                            'coveredAreaM2': sum(area_by_key[k]*fractions[k] for k in keys),
                            'overlapAreaM2': sum(area_by_key[k]*overlap_fractions[k] for k in keys),
                            'uncoveredFaces': sum(fractions[k] < 1-1e-7 for k in keys)})
        for ordinal, faces in enumerate(components(primitive['indices'])):
            corners = primitive['world'][faces]
            counts = Counter(t['partId'] for face in faces for t in grouped.get((primitive['id'], face), []))
            keys = [(primitive['id'], f) for f in faces]
            component_report.append({'sourceMesh': primitive['id'], 'name': primitive['name'],
                                     'component': ordinal, 'faces': len(faces), 'faceIds': faces,
                                     'bounds': [corners.reshape(-1, 3).min(axis=0).tolist(), corners.reshape(-1, 3).max(axis=0).tolist()],
                                     'sourceAreaM2': sum(area_by_key[k] for k in keys),
                                     'coveredAreaM2': sum(area_by_key[k]*fractions[k] for k in keys),
                                     'coveredFaces': sum(fractions[k] > 1e-7 for k in keys),
                                     'fullyCoveredFaces': sum(fractions[k] >= 1-1e-7 for k in keys),
                                     'overlapAreaM2': sum(area_by_key[k]*overlap_fractions[k] for k in keys),
                                     'mapParts': dict(counts)})

    part_report = []
    for part, triangles in sorted(by_part.items()):
        covered_by_source = defaultdict(list)
        for t in triangles:
            covered_by_source[(t['id']['sourceMesh'], t['id']['sourceTriangle'])].append(source_polygon(t))
        area = 0.
        for key, polys in covered_by_source.items():
            area += unary_union([p for p in polys if p.is_valid]).intersection(simplex).area * 2 * float(area3(sources[key[0]]['world'][key[1]]))
        part_report.append({'partId': part, 'mapTriangles': len(triangles),
                            'uniqueSourceFaces': len(covered_by_source), 'unionCoveredAreaM2': area,
                            'summedMapWorldAreaM2': float(area3(np.asarray([t['world'] for t in triangles])).sum())})

    sdk = json.loads(args.sdk.read_text(encoding='utf-8'))
    raw = list(Glb(args.baseline).primitives())
    raw_by_name = {p['name']: p for p in raw}
    sdk_candidates = []
    software_by_name = {p['name']: p for p in primitives}
    for candidate in raw:
        assignment = None
        for fragment, config in sdk['ResourceMapping'].items():
            if fragment in candidate['name']:
                assignment = config['materialNmae']
        if not assignment or not (assignment.startswith('carBody_MT_') or assignment == 'carPaint_sameColor_MT'):
            continue
        software = software_by_name.get(candidate['name'])
        entry = {'name': candidate['name'], 'baselineFaces': len(candidate['world']),
                 'sdkMaterial': assignment, 'presentInSoftware': software is not None,
                 'baselineAreaM2': float(area3(candidate['world']).sum())}
        if software is not None:
            raw_keys, software_keys = triangle_keys(candidate['world']), triangle_keys(software['world'])
            entry.update({'softwarePaintTagged': software['paint'],
                          'softwareFaces': len(software['world']),
                          'softwareAreaM2': float(area3(software['world']).sum()),
                          'baselineOnlyTrianglesAt10MicrometreGrid': sum((raw_keys-software_keys).values()),
                          'softwareOnlyTrianglesAt10MicrometreGrid': sum((software_keys-raw_keys).values()),
                          'boundsMaxErrorMm': float(np.max(np.abs(np.array([candidate['world'].min(axis=(0, 1)), candidate['world'].max(axis=(0, 1))]) - np.array([software['world'].min(axis=(0, 1)), software['world'].max(axis=(0, 1))])))*1000)})
        sdk_candidates.append(entry)

    report = {'status': 'source-coverage-audit-requires-visibility-and-semantic-review',
              'inputs': {str(p): {'sha256': digest(p), 'bytes': p.stat().st_size} for p in (args.model, args.map, args.baseline, args.sdk)},
              'paintFingerprint': mapping['paintFingerprint'],
              'modelFingerprintMatches': True,
              'sourcePaintMeshes': len(mesh_report), 'sourcePaintFaces': sum(m['sourceFaces'] for m in mesh_report),
              'sourcePaintAreaM2': sum(m['sourceAreaM2'] for m in mesh_report),
              'unionMappedPaintAreaM2': sum(m['coveredAreaM2'] for m in mesh_report),
              'duplicateMappedAreaM2': sum(m['overlapAreaM2'] for m in mesh_report),
              'partCount': len(part_report), 'mapTriangles': len(mapping['triangles']),
              'invalidSourceReferences': errors, 'invalidBarycentricTriangles': invalid_barycentric,
              'barycentricWorldMaxErrorMm': source_error_max*1000,
              'barycentricWorldErrorsAbovePointOneMm': outliers,
              'parts': part_report, 'sourceMeshes': mesh_report,
              'baselineSdkPaintCandidates': sdk_candidates,
              'coverageMethod': 'Normalize every complete homogeneous barycentric row by its sum, preserve exact zero source coordinates, prove suspected zero-domain fragments by exact dyadic rational 3x3 determinant, then polygon union clipped to the fixed (b1,b2) source simplex; source GLB world triangle area converts ratios to m2.',
              'baselineComparisonLimit': '10 micrometre coordinate grid is a reproducible signature, not a distance tolerance. Grid boundary mismatches require nearest-geometry follow-up.',
              'elapsedSeconds': time.time()-began}
    write_json(args.output/'geometry.json', report)
    write_json(args.output/'components.json', component_report)
    write_json(args.output/'uncovered-source-faces.json', uncovered_faces)
    write_json(args.output/'uncovered-probes.json', probes)
    print(json.dumps({k: report[k] for k in ('sourcePaintFaces', 'sourcePaintAreaM2', 'unionMappedPaintAreaM2', 'duplicateMappedAreaM2', 'partCount', 'mapTriangles', 'barycentricWorldMaxErrorMm')}), flush=True)


def compare_source_footprints(args):
    """Independent source-domain symmetric difference; sums cannot hide losses."""
    from shapely.geometry import Polygon
    from shapely.ops import unary_union
    current=json.loads(args.map.read_text('utf-8'))
    previous=json.loads(args.compare_map.read_text('utf-8'))
    if current['modelFingerprint']!=previous['modelFingerprint'] or current['modelFingerprint']!=digest(args.model):
        raise ValueError('Source footprint comparison requires the exact same model')
    sources={p['id']:p for p in Glb(args.model).primitives()}
    simplex=Polygon([(0.,0.),(1.,0.),(0.,1.)])
    maps=[]
    for document in (previous,current):
        groups=defaultdict(list)
        for t in document['triangles']:
            polygon=source_polygon(t)
            if polygon.is_valid and polygon.area>0:
                groups[(t['id']['sourceMesh'],t['id']['sourceTriangle'])].append(polygon)
        maps.append({key:unary_union(polygons).intersection(simplex) for key,polygons in groups.items()})
    differences=[]
    for key in sorted(set(maps[0])|set(maps[1])):
        before,after=[m.get(key,Polygon()) for m in maps]
        scale=2*float(area3(sources[key[0]]['world'][key[1]]))
        lost=before.difference(after).area*scale;gained=after.difference(before).area*scale
        if lost>0. or gained>0.:
            differences.append({'sourceMesh':key[0],'face':key[1],'lostAreaM2':lost,'gainedAreaM2':gained})
    report={'status':'exact-source-domain-symmetric-difference-not-visibility-proof',
            'sourceDomainNormalization':'Complete homogeneous rows divided by row sum, exact source zero coordinates retained; exact dyadic rational determinant rejects only proved collinear fragments before the fixed (b1,b2) polygon union.',
            'before':{'path':str(args.compare_map.resolve()),'sha256':digest(args.compare_map),'paintFingerprint':previous['paintFingerprint']},
            'after':{'path':str(args.map.resolve()),'sha256':digest(args.map),'paintFingerprint':current['paintFingerprint']},
            'sourceModelSha256':digest(args.model),'lostSourceAreaM2':sum(d['lostAreaM2'] for d in differences),
            'gainedSourceAreaM2':sum(d['gainedAreaM2'] for d in differences),
            'changedSourceFaces':len(differences),'sourceFaceDifferences':sorted(differences,key=lambda d:d['lostAreaM2']+d['gainedAreaM2'],reverse=True),
            'method':'Per original source triangle, polygon union clipped to barycentric simplex, both directed differences converted using actual GLB world area. Every positive difference contributes to the totals; no reporting area cutoff.'}
    write_json(args.output/'source-footprint-difference.json',report)
    print(json.dumps({k:v for k,v in report.items() if k not in ('sourceFaceDifferences','method','before','after')}),flush=True)


def visibility(args):
    from mathutils import Vector
    from mathutils.bvhtree import BVHTree

    began = time.time()
    probes = json.loads((args.output/'uncovered-probes.json').read_text('utf-8'))
    config = json.loads((args.model.parent/'config.json').read_text('utf-8'))
    modules = [args.model, args.model.parent/config['wheel'][config.get('defaults', {}).get('wheel', 0)]['asset']]
    vertices, faces, identities = [], [], []
    for module in modules:
        for primitive in Glb(module).primitives():
            start = len(vertices)
            vertices.extend(primitive['positions'].tolist())
            faces.extend((primitive['indices']+start).tolist())
            identities.extend((primitive['id'], face) for face in range(len(primitive['indices'])))
    tree = BVHTree.FromPolygons(vertices, faces, all_triangles=True, epsilon=0.)
    cameras = [Vector((7*math.cos(i*math.tau/36), height, 7*math.sin(i*math.tau/36)))
               for height in (.25, .85, 1.6, 3.5) for i in range(36)]
    cameras.append(Vector((0, 8, 0)))
    witnesses, hidden = [], 0
    by_mesh = defaultdict(lambda: {'faces': set(), 'visibleUncoveredRegionAreaM2': 0.})
    for number, probe in enumerate(probes):
        normal = Vector(probe['normal'])
        found = None
        for point_index, coordinates in enumerate(probe['points']):
            point = Vector(coordinates)
            for camera_index, camera in enumerate(cameras):
                ray = point-camera
                distance = ray.length
                direction = ray.normalized()
                if normal.dot(-direction) < .02:
                    continue
                hit, _, hit_index, _ = tree.ray_cast(camera, direction, distance+.0001)
                if hit is not None and identities[hit_index] == (probe['sourceMesh'], probe['face']) and (hit-point).length < .0001:
                    found = {'probe': number, 'sourceMesh': probe['sourceMesh'], 'face': probe['face'],
                             'name': probe['name'], 'uncoveredRegionAreaM2': probe['uncoveredAreaM2'],
                             'point': coordinates, 'camera': list(camera),
                             'pointIndex': point_index, 'cameraIndex': camera_index,
                             'hitErrorMm': (hit-point).length*1000}
                    break
            if found:
                break
        if found:
            witnesses.append(found)
            group = by_mesh[probe['sourceMesh']]
            group['faces'].add(probe['face'])
            group['visibleUncoveredRegionAreaM2'] += probe['uncoveredAreaM2']
        else:
            hidden += 1
        if number % 5000 == 0:
            print(f'visibility {number}/{len(probes)}; witnesses {len(witnesses)}', flush=True)
    report = {'status': 'sampled-uncovered-region-visibility-not-semantic-approval',
              'modules': {str(p): digest(p) for p in modules}, 'cameraCount': len(cameras),
              'uncoveredRegions': len(probes), 'visibleUncoveredRegions': len(witnesses),
              'notObservedRegions': hidden, 'elapsedSeconds': time.time()-began,
              'meshes': [{'sourceMesh': key, 'visibleUncoveredFaces': len(v['faces']),
                          'visibleUncoveredRegionAreaM2': v['visibleUncoveredRegionAreaM2']}
                         for key, v in by_mesh.items()],
              'areaLimit': 'Area counts the entire uncovered polygon containing a visible witness, not the exact visible subarea. Hidden rays do not prove exclusion or semantic irrelevance.',
              'witnesses': witnesses}
    write_json(args.output/'visibility.json', report)
    print(json.dumps({k:v for k,v in report.items() if k != 'witnesses'}), flush=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--model', type=Path, required=True)
    parser.add_argument('--map', type=Path)
    parser.add_argument('--baseline', type=Path)
    parser.add_argument('--sdk', type=Path)
    parser.add_argument('--compare-map', type=Path)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--visibility', action='store_true')
    argv = sys.argv[sys.argv.index('--')+1:] if '--' in sys.argv else None
    args = parser.parse_args(argv)
    if args.visibility:
        visibility(args)
    else:
        if not all((args.map, args.baseline, args.sdk)):
            parser.error('geometry requires --map --baseline --sdk')
        geometry(args)
        if args.compare_map:
            compare_source_footprints(args)


if __name__ == '__main__':
    main()
