"""Read-only exterior-view audit of source paint faces absent from the editable map.

blender --background --python tools/assets/audit-paint-completeness.py -- body.glb paint-map.json output.json
This is a camera sample audit, not semantic approval of every triangle.
"""
import json
import math
import struct
import sys
from pathlib import Path
from collections import defaultdict
import numpy as np
from mathutils import Matrix, Vector, Quaternion
from mathutils.bvhtree import BVHTree

source, mapping, output = map(Path, sys.argv[sys.argv.index('--') + 1:][:3])
raw = source.read_bytes()
length = struct.unpack_from('<I', raw, 12)[0]
document = json.loads(raw[20:20 + length])
bin_start = 20 + length + 8
dtypes = {5121: 'u1', 5123: '<u2', 5125: '<u4', 5126: '<f4'}
sizes = {'SCALAR': 1, 'VEC2': 2, 'VEC3': 3, 'VEC4': 4}
def accessor(index):
    a = document['accessors'][index]
    v = document['bufferViews'][a['bufferView']]
    size = sizes[a['type']]
    offset = bin_start + v.get('byteOffset', 0) + a.get('byteOffset', 0)
    dtype = np.dtype(dtypes[a['componentType']])
    return np.ndarray((a['count'], size), dtype=dtype, buffer=raw, offset=offset,
                      strides=(v.get('byteStride', dtype.itemsize * size), dtype.itemsize)).copy()
vertices, faces, source_faces = [], [], []
module_name = 'body.glb'
def visit(index, parent):
    node = document['nodes'][index]
    if 'matrix' in node:
        local = Matrix(np.array(node['matrix']).reshape((4, 4)).T.tolist())
    else:
        rotation = node.get('rotation', [0, 0, 0, 1])
        local = Matrix.LocRotScale(Vector(node.get('translation', [0, 0, 0])), Quaternion((rotation[3], *rotation[:3])), Vector(node.get('scale', [1, 1, 1])))
    matrix = parent @ local
    if 'mesh' in node:
        for pi, primitive in enumerate(document['meshes'][node['mesh']]['primitives']):
            positions = accessor(primitive['attributes']['POSITION'])
            transform = np.array(matrix)
            world = positions @ transform[:3, :3].T + transform[:3, 3]
            indices = accessor(primitive['indices']).reshape((-1, 3))
            if matrix.determinant() < 0: indices = indices[:, [0, 2, 1]]
            offset = len(vertices)
            vertices.extend(world.tolist())
            faces.extend((indices + offset).tolist())
            paint = module_name == 'body.glb' and document['materials'][primitive.get('material', 0)].get('extras', {}).get('native_paint_switch', False)
            for i, f in enumerate(indices):
                source_faces.append((f'{module_name}/node/{index}/primitive/{pi}', i, node.get('name', ''), paint, world[f]))
    for child in node.get('children', []): visit(child, matrix)
for index in document['scenes'][document.get('scene', 0)]['nodes']: visit(index, Matrix.Identity(4))
# Wheel modules occlude the wheel-well backing in the actual assembled vehicle.
config_path = source.parent / 'config.json'
occluder_modules = []
if config_path.is_file():
    config = json.loads(config_path.read_text(encoding='utf-8'))
    module_name = config['wheel'][config.get('defaults', {}).get('wheel', 0)]['asset']
    raw = (source.parent / module_name).read_bytes()
    length = struct.unpack_from('<I', raw, 12)[0]
    document = json.loads(raw[20:20+length])
    bin_start = 20+length+8
    for index in document['scenes'][document.get('scene', 0)]['nodes']: visit(index, Matrix.Identity(4))
    occluder_modules.append(module_name)
tree = BVHTree.FromPolygons(vertices, faces, all_triangles=True, epsilon=0)
mapping_doc = json.loads(mapping.read_text(encoding='utf-8'))
mapped = defaultdict(float)
for t in mapping_doc['triangles']:
    p = np.array(t['world'])
    mapped[(t['id']['sourceMesh'], t['id']['sourceTriangle'])] += np.linalg.norm(np.cross(p[1]-p[0], p[2]-p[0])) / 2
cameras = [Vector((6*math.cos(i*math.tau/24), y, 6*math.sin(i*math.tau/24))) for y in [.45, 1.4, 3.5] for i in range(24)]
cameras.append(Vector((0, 8, 0)))
missing = defaultdict(list)
for mesh, face, name, paint, points in source_faces:
    if not paint: continue
    e, f = points[1]-points[0], points[2]-points[0]
    normal = np.cross(e, f)
    area = float(np.linalg.norm(normal)/2)
    if area < 1e-10 or mapped[(mesh, face)] >= area * .98: continue
    normal = Vector(normal).normalized()
    center = Vector(points.mean(axis=0))
    for camera in cameras:
        direction = center - camera
        distance = direction.length
        if normal.dot(-direction.normalized()) < .02: continue
        hit, _, _, _ = tree.ray_cast(camera, direction.normalized(), distance + .001)
        if hit is not None and (hit-center).length < .0005:
            missing[mesh].append({'face': face, 'name': name, 'areaM2': area, 'mappedAreaM2': mapped[(mesh, face)], 'center': list(center), 'normal': list(normal)})
            break
report = {'source': str(source), 'map': str(mapping), 'occluderModules': occluder_modules, 'status': 'sampled-exterior-audit-not-semantic-approval',
          'cameraCount': len(cameras), 'meshes': [{'sourceMesh': mesh, 'faces': len(items), 'unmappedVisibleAreaM2': sum(max(0, t['areaM2']-t['mappedAreaM2']) for t in items), 'samples': items} for mesh, items in missing.items()]}
output.parent.mkdir(parents=True, exist_ok=True)
output.write_text(json.dumps(report, ensure_ascii=False, indent=2), encoding='utf-8')
print(json.dumps([{k:v for k,v in m.items() if k != 'samples'} for m in report['meshes']]), flush=True)
