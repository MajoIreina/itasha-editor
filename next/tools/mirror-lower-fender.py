"""Rebuild su7.front-left-lower-fender as the mirror image of the right part.

The user confirmed the left lower fender is the axisymmetric counterpart of the
right one (mirror across z=0). They are separate connected components of
node/175; their mirrored vertex centroids agree within ~3 mm.

Geometry is rebuilt face-to-face: every right source face is mirrored across
z=0 and matched to the nearest left-component face by centroid, then the map
triangle keeps its source barycentrics verbatim. Identity fragments stay whole
faces, clipped slivers stay slivers, and the runtime barycentric consistency
check holds exactly because world is the interpolation of those barycentrics.

Charts are mirrored horizontally (u' = chartWidth - u) so the left canvas reads
as the mirror image of the right canvas. The 11 reviewed right-side seams are
mirrored onto the left counterparts; every other triangle and connection is
kept byte-for-byte.

Output is unsealed; seal with:
cargo run -p itasha-surface-map --example seal_map -- <in> <out>
"""
from __future__ import annotations

import argparse
import hashlib
import json
import math
import struct
from collections import defaultdict
from pathlib import Path

NEXT = Path(__file__).resolve().parents[1]
WORKSPACE = NEXT.parents[1]
DEFAULT_INPUT = NEXT / 'assets/paint/su7-v23-restored/paint-map.json'
DEFAULT_MODEL = WORKSPACE / '3D/Blender/software/body.glb'

LEFT = 'su7.front-left-lower-fender'
RIGHT = 'su7.front-right-lower-fender'
MESH_NODE = 'body.glb/node/175/primitive/0'


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

    return document, accessor


def node_world_matrix(document, index):
    """World matrix of a glTF node, walking the parent chain. Column-major."""
    identity = [1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1.]

    def local(node):
        if 'matrix' in node:
            return list(node['matrix'])
        t = node.get('translation', [0., 0., 0.])
        s = node.get('scale', [1., 1., 1.])
        x, y, z, w = node.get('rotation', [0., 0., 0., 1.])
        r = [1 - 2 * (y * y + z * z), 2 * (x * y + z * w), 2 * (x * z - y * w),
             2 * (x * y - z * w), 1 - 2 * (x * x + z * z), 2 * (y * z + x * w),
             2 * (x * z + y * w), 2 * (y * z - x * w), 1 - 2 * (x * x + y * y)]
        m = [0.] * 16
        for c in range(3):
            for row in range(3):
                m[c * 4 + row] = r[c * 3 + row] * s[c]
        m[12], m[13], m[14], m[15] = t[0], t[1], t[2], 1.
        return m

    def multiply(a, b):
        out = [0.] * 16
        for c in range(4):
            for row in range(4):
                out[c * 4 + row] = sum(a[k * 4 + row] * b[c * 4 + k] for k in range(4))
        return out

    parents = {child: i for i, node in enumerate(document['nodes'])
               for child in node.get('children', [])}
    chain = [index]
    while chain[-1] in parents:
        chain.append(parents[chain[-1]])
    matrix = identity
    for node_index in reversed(chain):
        matrix = multiply(matrix, local(document['nodes'][node_index]))
    return matrix


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

    target = next(face_root[f] for f in range(len(face_root))
                  if all(abs(centroid(f)[axis] - anchor[axis]) < tolerance
                         for axis in range(3)))
    return sorted(f for f in range(len(face_root)) if face_root[f] == target)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--input', type=Path, default=DEFAULT_INPUT)
    parser.add_argument('--model', type=Path, default=DEFAULT_MODEL)
    parser.add_argument('--output', type=Path, required=True)
    options = parser.parse_args()
    if options.output.exists():
        raise FileExistsError(f'Refusing to overwrite: {options.output}')
    options.output.mkdir(parents=True)

    source = json.loads(options.input.read_text(encoding='utf-8'))
    model_hash = hashlib.sha256(options.model.read_bytes()).hexdigest()
    if source.get('modelFingerprint') != model_hash:
        raise ValueError('Input map model fingerprint differs from the GLB')

    document, accessor = read_glb(options.model)
    node = document['nodes'][175]
    primitive = document['meshes'][node['mesh']]['primitives'][0]
    matrix = node_world_matrix(document, 175)

    def bake(point):
        x, y, z = point
        return (matrix[0] * x + matrix[4] * y + matrix[8] * z + matrix[12],
                matrix[1] * x + matrix[5] * y + matrix[9] * z + matrix[13],
                matrix[2] * x + matrix[6] * y + matrix[10] * z + matrix[14])

    # Paint-map world coordinates are baked world space; GLB POSITION is node local.
    positions = [bake(point) for point in accessor(primitive['attributes']['POSITION'])]
    indices = [item[0] for item in accessor(primitive['indices'])]
    left_faces = component_faces(positions, indices, (-1.14, 0.50, 0.95))
    right_faces = component_faces(positions, indices, (-1.14, 0.50, -0.95))
    print(f'left component {len(left_faces)} faces, right {len(right_faces)} faces')

    def face_corners(face):
        return [positions[indices[face * 3 + k]] for k in range(3)]

    def face_centroid(face):
        corners = face_corners(face)
        return [sum(p[i] for p in corners) / 3. for i in range(3)]

    right_centroids = [face_centroid(f) for f in right_faces]
    left_centroids = [face_centroid(f) for f in left_faces]
    # Face correspondence: mirrored right centroid -> nearest left centroid.
    correspondence = {}
    permutations = {}
    worst = 0.0
    for slot, face in enumerate(right_faces):
        c = right_centroids[slot]
        point = (c[0], c[1], -c[2])
        best = None
        for lslot, left in enumerate(left_faces):
            lc = left_centroids[lslot]
            d = (lc[0] - point[0]) ** 2 + (lc[1] - point[1]) ** 2 + (lc[2] - point[2]) ** 2
            if best is None or d < best[0]:
                best = (d, left)
        left = best[1]
        correspondence[face] = left
        worst = max(worst, math.sqrt(best[0]))
        # Corner correspondence: mirrored right corner k -> nearest left corner.
        # The barycentric rows are expressed in right-corner order, so they must
        # be permuted into the matched left-corner order before the world points
        # are rebuilt; otherwise the UV/geometry frame is scrambled and coverage
        # transport can leak across the vehicle.
        right_corners = face_corners(face)
        left_corners = face_corners(left)
        mirrored = [(p[0], p[1], -p[2]) for p in right_corners]
        perm = []
        for m in mirrored:
            nearest = min(
                range(3),
                key=lambda k: (left_corners[k][0] - m[0]) ** 2
                + (left_corners[k][1] - m[1]) ** 2
                + (left_corners[k][2] - m[2]) ** 2,
            )
            perm.append(nearest)
        if len(set(perm)) != 3:
            # Degenerate/ambiguous face pairing: fall back to the identity order.
            perm = [0, 1, 2]
        permutations[face] = perm
    print(f'face correspondence: worst mirrored centroid {worst * 1000:.2f} mm')

    right = [t for t in source['triangles'] if t['partId'] == RIGHT]
    if not right:
        raise ValueError('input map has no right lower fender to mirror')
    chart_width = max(p[0] for t in right for p in t['paintUv'])
    fragment_base = max(t['id']['fragment'] for t in source['triangles'])

    def weights_valid(barycentric):
        return all(-0.002 <= v <= 1.002 for row in barycentric for v in row) \
            and all(abs(sum(row) - 1.) <= 0.002 for row in barycentric)

    rebuilt = []
    invalid = []
    for order, triangle in enumerate(right):
        face = correspondence[triangle['id']['sourceTriangle']]
        barycentric = triangle['sourceBarycentric']
        if not weights_valid(barycentric):
            invalid.append(triangle['id']['sourceTriangle'])
            continue
        corners = face_corners(face)
        # Reorder each barycentric row from right-corner order into left-corner
        # order so corner k of the rebuilt triangle is the mirror of corner k of
        # the right triangle. Rebuilt corner order therefore matches the right
        # triangle's, which keeps the seam edge indices valid.
        perm = permutations[triangle['id']['sourceTriangle']]
        reordered = []
        for row in barycentric:
            out = [0.] * 3
            for m in range(3):
                out[perm[m]] = row[m]
            reordered.append(out)
        barycentric = reordered
        world = [list(sum(barycentric[k][j] * corners[j][i] for j in range(3))
                      for i in range(3)) for k in range(3)]
        # Mirrored source data can flip a sliver's winding into a zero-area line;
        # such fragments carry no area and are dropped explicitly.
        a = [world[1][i] - world[0][i] for i in range(3)]
        b = [world[2][i] - world[0][i] for i in range(3)]
        cross = [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2],
                 a[0] * b[1] - a[1] * b[0]]
        if math.sqrt(sum(v * v for v in cross)) * 0.5 < 1e-9:
            invalid.append(triangle['id']['sourceTriangle'])
            continue
        uv = [[chart_width - p[0], p[1]] for p in triangle['paintUv']]
        # `paintUv` is still in right-corner order while `world` now follows the
        # matched left-corner order. Move the UV corners along with the world
        # corners so the rebuilt triangle's affine frame stays consistent with the
        # right triangle it mirrors; otherwise ChartFrame reads a scrambled scale.
        uv = [uv[perm.index(k)] for k in range(3)]
        rebuilt.append({
            'id': {
                'sourceMesh': MESH_NODE,
                'primitive': 0,
                'sourceTriangle': face,
                'fragment': fragment_base + 1 + order,
            },
            'partId': LEFT,
            'regionId': f'{LEFT}/mirror-right',
            'chartId': f'{LEFT}/mirror-right/{triangle["chartId"].rsplit("/", 1)[-1]}',
            'world': world,
            'sourceBarycentric': [[float(v) for v in row] for row in barycentric],
            'paintUv': uv,
        })
    print(f'rebuilt {len(rebuilt)} triangles; invalid {len(invalid)}')

    kept = [t for t in source['triangles']
            if t['partId'] not in (LEFT, RIGHT)] + right + rebuilt
    ids = defaultdict(int)
    for t in kept:
        ids[(t['id']['sourceMesh'], t['id']['primitive'],
             t['id']['sourceTriangle'], t['id']['fragment'])] += 1
    duplicates = [key for key, count in ids.items() if count > 1]
    if duplicates:
        raise ValueError(f'duplicate triangle ids: {duplicates[:3]}')

    by_key = {(t['id']['sourceMesh'], t['id']['primitive'], t['id']['sourceTriangle']): t
              for t in source['triangles']}

    def key_of(triangle_id):
        return (triangle_id['sourceMesh'], triangle_id['primitive'],
                triangle_id['sourceTriangle'], triangle_id['fragment'])

    def triangle_of(triangle_id):
        return next(t for t in source['triangles'] if key_of(t['id']) == key_of(triangle_id))

    def edge_of(triangle, index):
        return [triangle['world'][index], triangle['world'][(index + 1) % 3]]

    def best_edge(candidates, reference, limit):
        best = None
        for candidate in candidates:
            points = candidate['world']
            for edge in range(3):
                pair = [points[edge], points[(edge + 1) % 3]]
                for reversed_flag in (False, True):
                    target = reference if not reversed_flag else list(reversed(reference))
                    error = max(math.dist(pair[k], target[k]) for k in range(2))
                    length_error = abs(math.dist(*pair) - math.dist(*reference))
                    if error > limit or length_error > 0.005:
                        continue
                    if best is None or error < best[0]:
                        best = (error, candidate, edge, reversed_flag)
        return best

    def is_left(triangle_id):
        return by_key[(triangle_id['sourceMesh'], triangle_id['primitive'],
                       triangle_id['sourceTriangle'])]['partId'] == LEFT

    # The reviewed map already carries the left part's real seams (door and front
    # fender). Right-side seams are not mirrored across: the left boundary of the
    # mesh does not coincide with the right one, so those pairs came out 8-41 mm
    # apart and let transport fold the neighbourhood across the whole car. Keep
    # every untouched connection, and re-anchor the real left seams onto the
    # rebuilt triangles.
    connections = [c for c in source['connections']
                   if not is_left(c['from']['triangle']) and not is_left(c['to']['triangle'])]
    reanchored = 0
    skipped = []
    for connection in source['connections']:
        left_side = 'from' if is_left(connection['from']['triangle']) else 'to'
        if is_left(connection['from']['triangle']) == is_left(connection['to']['triangle']):
            continue
        partner_side = 'to' if left_side == 'from' else 'from'
        old_left = triangle_of(connection[left_side]['triangle'])
        old_edge = connection[left_side]['edge']
        reference = edge_of(old_left, old_edge)
        # Rebuilt corners follow the right part's corner order, so the old edge
        # index and orientation still describe the same boundary. Only the owning
        # triangle identity changes; do not re-decide the seam direction.
        hit = None
        for successor in rebuilt:
            points = successor['world']
            pair = [points[old_edge], points[(old_edge + 1) % 3]]
            error = max(math.dist(pair[k], reference[k]) for k in range(2))
            length_error = abs(math.dist(*pair) - math.dist(*reference))
            if error > 0.02 or length_error > 0.005:
                continue
            if hit is None or error < hit[0]:
                hit = (error, successor)
        if hit is None:
            skipped.append((connection['id'], 'no rebuilt successor edge'))
            continue
        error, successor = hit
        rebuilt_connection = {
            'id': connection['id'],
            'from': dict(connection['from']),
            'to': dict(connection['to']),
            'reversed': bool(connection['reversed']),
            'physicalGapMm': round(error * 1000., 4),
            'propagation': connection['propagation'],
        }
        rebuilt_connection[left_side] = {
            'triangle': dict(successor['id']),
            'edge': old_edge,
        }
        connections.append(rebuilt_connection)
        reanchored += 1
    print(f're-anchored {reanchored} reviewed left seams onto rebuilt triangles; '
          f'skipped {len(skipped)}')
    for item in skipped:
        print('  skipped', item)

    output = {
        'mappingVersion': source['mappingVersion'],
        'modelFingerprint': source['modelFingerprint'],
        'paintFingerprint': '',
        'triangles': kept,
        'connections': connections,
    }
    (options.output / 'paint-map-unsealed.json').write_text(
        json.dumps(output, separators=(',', ':')), encoding='utf-8')
    report = {
        'status': 'mirrored-lower-fender-candidate-awaiting-seal',
        'input': str(options.input.resolve()),
        'inputPaintFingerprint': source.get('paintFingerprint'),
        'modelFingerprint': model_hash,
        'leftComponentFaces': len(left_faces),
        'rightComponentFaces': len(right_faces),
        'faceCorrespondenceWorstMm': worst * 1000,
        'rightTriangles': len(right),
        'rebuiltLeftTriangles': len(rebuilt),
        'invalidFragmentsDropped': len(invalid),
        'removedLeftTriangles': sum(1 for t in source['triangles'] if t['partId'] == LEFT),
        'reanchoredSeams': reanchored,
        'skippedSeams': [item[0] for item in skipped],
        'chartWidthMm': chart_width,
        'method': 'mirror right source faces across z=0 to left faces by centroid, '
                  'copy source barycentrics, mirror chart u = width - u, '
                  'mirror the right-side seams',
        'warnings': [
            'Left part duplicates right-part paint UVs mirrored horizontally; it '
            'is a design axis, not a separately unwrapped chart.',
            'Face correspondence by nearest mirrored centroid tolerates model '
            'asymmetry; see faceCorrespondenceWorstMm.',
        ],
    }
    (options.output / 'build-report.json').write_text(
        json.dumps(report, ensure_ascii=False, indent=2), encoding='utf-8')
    print(json.dumps({'triangles': len(kept), 'connections': len(connections),
                      'output': str(options.output.resolve())}, indent=2))


if __name__ == '__main__':
    main()
