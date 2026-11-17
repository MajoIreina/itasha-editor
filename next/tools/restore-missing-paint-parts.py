"""Restore reviewed paint triangles that v21 dropped, without touching the rest.

Two gaps are repaired:

* ``su7.trunk``  -- v21/v22 removed the whole part because no classified mask
  existed. v20c still carries the reviewed source-UV0 triangles (node/91), so
  they are carried forward unchanged: world coordinates, barycentrics, source
  identity and paint UV are copied verbatim.
* ``su7.front-left-lower-fender`` -- the v22 map only covers 842 of the 1578
  faces in the panel's connected component. v16 holds 1535 faces of the same
  panel; the 594 missing faces that v16 already reviewed are appended with a
  rigid offset as a clearly separated supplement island. The remaining 142
  faces never appeared in any reviewed map and are **not** invented here; they
  stay in ``unresolvedFaces`` for a later registration review.

The script is read-only for every input asset: it writes one unsealed candidate
(the fingerprint is recomputed by ``cargo run -p itasha-surface-map --example
seal_map``). Intermediate files are written to an explicit output directory.

python tools/restore-missing-paint-parts.py --output ../_scratch/restored
"""
from __future__ import annotations

import argparse
import json
import math
import struct
from collections import Counter, defaultdict
from pathlib import Path

NEXT = Path(__file__).resolve().parents[1]
WORKSPACE = NEXT.parents[1]
DEFAULT_BASE = NEXT / 'assets/paint/su7-v22-whole-car/paint-map.json'
DEFAULT_TRUNK = NEXT / 'assets/paint/su7-v20c-psd-registered-packed/paint-map.json'
DEFAULT_SUPPLEMENT = NEXT / 'assets/paint/su7-v16-classified-unfolding/paint-map.json'
DEFAULT_MODEL = WORKSPACE / '3D/Blender/software/body.glb'

LOWER_FENDER = 'su7.front-left-lower-fender'
LOWER_FENDER_MESH = 'body.glb/node/175/primitive/0'
TRUNK = 'su7.trunk'
SUPPLEMENT_GAP_MM = 180.0


def read_glb(path: Path):
    data = path.read_bytes()
    offset = 12
    chunks = []
    while offset < len(data):
        length, kind = struct.unpack_from('<II', data, offset)
        offset += 8
        chunks.append((kind, data[offset:offset + length]))
        offset += length
    document = json.loads(chunks[0][1])
    binary = chunks[1][1]
    components = {5120: ('b', 1), 5121: ('B', 1), 5122: ('h', 2),
                  5123: ('H', 2), 5125: ('I', 4), 5126: ('f', 4)}
    counts = {'SCALAR': 1, 'VEC2': 2, 'VEC3': 3, 'VEC4': 4}

    def accessor(index):
        item = document['accessors'][index]
        view = document['bufferViews'][item['bufferView']]
        fmt, size = components[item['componentType']]
        count = counts[item['type']]
        stride = view.get('byteStride') or size * count
        base = view.get('byteOffset', 0) + item.get('byteOffset', 0)
        return [struct.unpack_from('<' + fmt * count, binary, base + k * stride)
                for k in range(item['count'])]

    return document, binary, accessor


def component_faces(positions, indices, anchor, tolerance=0.05):
    parents = list(range(len(positions)))

    def root(index):
        while parents[index] != index:
            parents[index] = parents[parents[index]]
            index = parents[index]
        return index

    for f in range(0, len(indices), 3):
        a, b, c = indices[f], indices[f + 1], indices[f + 2]
        ra, rb, rc = root(a), root(b), root(c)
        parents[rb] = ra
        parents[root(rc)] = root(ra)
    face_root = [root(indices[f]) for f in range(0, len(indices), 3)]

    def centroid(face):
        return [sum(positions[indices[face * 3 + k]][axis] for k in range(3)) / 3.
                for axis in range(3)]

    anchor_face = next(f for f in range(len(face_root))
                       if all(abs(centroid(f)[axis] - anchor[axis]) < tolerance
                              for axis in range(3)))
    target = face_root[anchor_face]
    return {f for f in range(len(face_root)) if face_root[f] == target}


def triangle_area(triangle):
    a = [triangle[1][i] - triangle[0][i] for i in range(3)]
    b = [triangle[2][i] - triangle[0][i] for i in range(3)]
    c = [a[1] * b[2] - a[2] * b[1],
         a[2] * b[0] - a[0] * b[2],
         a[0] * b[1] - a[1] * b[0]]
    return 0.5 * math.sqrt(sum(v * v for v in c))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--base', type=Path, default=DEFAULT_BASE)
    parser.add_argument('--trunk-source', type=Path, default=DEFAULT_TRUNK)
    parser.add_argument('--supplement-source', type=Path, default=DEFAULT_SUPPLEMENT)
    parser.add_argument('--model', type=Path, default=DEFAULT_MODEL)
    parser.add_argument('--output', type=Path, required=True)
    options = parser.parse_args()
    if options.output.exists():
        raise FileExistsError(f'Refusing to overwrite: {options.output}')
    options.output.mkdir(parents=True)

    base = json.loads(options.base.read_text(encoding='utf-8'))
    trunk_source = json.loads(options.trunk_source.read_text(encoding='utf-8'))
    supplement = json.loads(options.supplement_source.read_text(encoding='utf-8'))
    model_hash_before = None
    import hashlib
    model_hash = hashlib.sha256(options.model.read_bytes()).hexdigest()
    for label, document in (('base', base), ('trunk source', trunk_source),
                            ('supplement source', supplement)):
        if document.get('modelFingerprint') != model_hash:
            raise ValueError(f'{label} map model fingerprint differs from the GLB')
        if label == 'base':
            model_hash_before = document.get('paintFingerprint')

    document, binary, accessor = read_glb(options.model)
    node = document['nodes'][175]
    primitive = document['meshes'][node['mesh']]['primitives'][0]
    positions = accessor(primitive['attributes']['POSITION'])
    indices = [item[0] for item in accessor(primitive['indices'])]
    component = component_faces(positions, indices, (-1.14, 0.50, 0.95))
    print(f'left lower fender component: {len(component)} faces')

    existing_faces = {t['id']['sourceTriangle'] for t in base['triangles']
                      if t['partId'] == LOWER_FENDER
                      and t['id']['sourceMesh'] == LOWER_FENDER_MESH}
    missing = sorted(component - existing_faces)
    available = {t['id']['sourceTriangle']: t for t in supplement['triangles']
                 if t['partId'] == LOWER_FENDER
                 and t['id']['sourceMesh'] == LOWER_FENDER_MESH
                 and t['id']['sourceTriangle'] in set(missing)}
    unresolved = [f for f in missing if f not in available]
    print(f'missing {len(missing)}; recoverable from v16 {len(available)}; unresolved {len(unresolved)}')

    # The supplement keeps v16's intra-chart layout, moved rigidly below the v22
    # island so 2D masks never overlap. Translation only: no scale or rotation.
    base_uv = [p for t in base['triangles'] if t['partId'] == LOWER_FENDER
               for p in t['paintUv']]
    base_floor = max(p[1] for p in base_uv)
    supplement_min_y = min(p[1] for t in available.values() for p in t['paintUv'])
    offset_y = base_floor + SUPPLEMENT_GAP_MM - supplement_min_y
    fragment_base = max((t['id']['fragment'] for t in base['triangles']), default=0)

    added = []
    for order, face in enumerate(sorted(available)):
        original = available[face]
        item = {
            'id': {
                'sourceMesh': original['id']['sourceMesh'],
                'primitive': original['id']['primitive'],
                'sourceTriangle': original['id']['sourceTriangle'],
                'fragment': fragment_base + 1 + order,
            },
            'partId': LOWER_FENDER,
            'regionId': f'{LOWER_FENDER}/restored-v16',
            'chartId': f'{LOWER_FENDER}/restored-v16/000',
            'world': original['world'],
            'sourceBarycentric': original['sourceBarycentric'],
            'paintUv': [[p[0], p[1] + offset_y] for p in original['paintUv']],
        }
        added.append(item)

    trunk = [dict(t, regionId='su7.trunk/exterior')
             for t in trunk_source['triangles'] if t['partId'] == TRUNK]
    for order, item in enumerate(trunk):
        item['id'] = dict(item['id'], fragment=fragment_base + len(added) + 1 + order)
    print(f'trunk triangles restored: {len(trunk)}')

    triangles = base['triangles'] + added + trunk
    ids = Counter((t['id']['sourceMesh'], t['id']['primitive'],
                   t['id']['sourceTriangle'], t['id']['fragment']) for t in triangles)
    duplicates = [key for key, count in ids.items() if count > 1]
    if duplicates:
        raise ValueError(f'duplicate triangle ids: {duplicates[:3]}')

    output = {
        'mappingVersion': base['mappingVersion'],
        'modelFingerprint': base['modelFingerprint'],
        'paintFingerprint': '',
        'triangles': triangles,
        'connections': base['connections'],
    }
    (options.output / 'paint-map-unsealed.json').write_text(
        json.dumps(output, separators=(',', ':')), encoding='utf-8')
    report = {
        'status': 'restored-candidate-awaiting-seal',
        'baseMap': str(options.base.resolve()),
        'basePaintFingerprint': model_hash_before,
        'trunkSource': str(options.trunk_source.resolve()),
        'supplementSource': str(options.supplement_source.resolve()),
        'modelFingerprint': model_hash,
        'trunkTriangles': len(trunk),
        'lowerFender': {
            'componentFaces': len(component),
            'existingFaces': len(existing_faces),
            'missingFaces': len(missing),
            'restoredFromV16': len(added),
            'unresolvedFaces': unresolved,
            'supplementOffsetMm': [0.0, offset_y],
        },
        'warnings': [
            'Trunk triangles keep v20c source-UV0 paint coordinates verbatim; they are unclassified geometry, not an approved classified part.',
            'Restored lower-fender faces keep v16 rigid island placement; they are separated from the v22 island and do not share seams.',
            'Faces absent from every reviewed map stay unresolved and are not invented.',
        ],
    }
    (options.output / 'build-report.json').write_text(
        json.dumps(report, ensure_ascii=False, indent=2), encoding='utf-8')
    print(json.dumps({'triangles': len(triangles),
                      'connections': len(base['connections']),
                      'output': str(options.output.resolve())}, indent=2))


if __name__ == '__main__':
    main()
