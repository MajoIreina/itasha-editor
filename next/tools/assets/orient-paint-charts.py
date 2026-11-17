"""Reorient intrinsic UV islands using physical front/up axes, without changing source faces.

blender --background --python tools/assets/orient-paint-charts.py -- source.json new-directory
Original material UVs and model files are never written.
"""
import json
import math
import sys
from pathlib import Path
from collections import defaultdict
import numpy as np
from mathutils import Vector
from mathutils.bvhtree import BVHTree

source, output = map(Path, sys.argv[sys.argv.index('--') + 1:][:2])
output.mkdir(parents=True, exist_ok=False)
document = json.loads(source.read_text(encoding='utf-8'))
groups = defaultdict(list)
for triangle in document['triangles']:
    groups[triangle['partId']].append(triangle)
report = {'source': str(source), 'sourceFacesChanged': False, 'parts': []}

def physical_xy(part, world):
    if '-left-' in part or part.endswith('left-sill'):
        return np.column_stack((-world[:, 1], world[:, 0])) * 1000
    if '-right-' in part or part.endswith('right-sill'):
        return np.column_stack((world[:, 1], world[:, 0])) * 1000
    if 'bumper' in part or 'c-ring' in part:
        return np.column_stack((-world[:, 2], -world[:, 1])) * 1000
    return np.column_stack((-world[:, 2], world[:, 0])) * 1000

for part, triangles in groups.items():
    repaired=0
    for triangle in triangles:
        uv=np.array(triangle['paintUv'],dtype=float)
        p=np.array(triangle['world'],dtype=float)
        e,f=p[1]-p[0],p[2]-p[0]
        length=np.linalg.norm(e)
        xx=np.dot(f,e)/length
        yy=np.linalg.norm(np.cross(e,f))/length
        matrix=np.column_stack((uv[1]-uv[0],uv[2]-uv[0]))
        singular=np.linalg.svd(matrix @ np.linalg.inv(np.array([[length,xx],[0.,yy]])),compute_uv=False)
        area_ratio=abs(np.linalg.det(matrix))/max(length*yy*1e6,1e-30)
        if singular[0]/max(singular[1],1e-30)>8 or area_ratio<.01 or area_ratio>100 or np.linalg.det(matrix)<=0:
            # Numerical failures on sliver fragments must not delete source paint.
            # An individual triangle has an exact intrinsic planar representation.
            x=1e6+repaired*10
            triangle['paintUv']=[[x,0.],[x+length*1000,0.],[x+xx*1000,yy*1000]]
            repaired+=1
    parents = list(range(len(triangles)))
    def root(i):
        while parents[i] != i:
            parents[i] = parents[parents[i]]
            i = parents[i]
        return i
    edges = {}
    for i, t in enumerate(triangles):
        # Only join triangles along an actual continuous UV edge. A shared point is not an island.
        corners = [(tuple(round(v, 2) for v in uv), tuple(round(v, 7) for v in world)) for uv,world in zip(t['paintUv'],t['world'])]
        for j in range(3):
            key = tuple(sorted((corners[j], corners[(j + 1) % 3])))
            if key in edges:
                parents[root(i)] = root(edges[key])
            else:
                edges[key] = i
    islands = defaultdict(list)
    for i in range(len(triangles)):
        islands[root(i)].append(i)
    charts = []
    for ids in islands.values():
        uv = np.array([triangles[i]['paintUv'] for i in ids], dtype=float)
        world = np.array([triangles[i]['world'] for i in ids], dtype=float)
        area = np.linalg.norm(np.cross(world[:, 1] - world[:, 0], world[:, 2] - world[:, 0]), axis=1) / 2
        weight = np.repeat(np.maximum(area, 1e-15), 3)
        flat = uv.reshape((-1, 2))
        target = physical_xy(part, world.reshape((-1, 3)))
        a = np.average(flat, axis=0, weights=weight)
        b = np.average(target, axis=0, weights=weight)
        u, v = flat - a, target - b
        dot = np.sum(weight * np.sum(u * v, axis=1))
        cross = np.sum(weight * (u[:, 0] * v[:, 1] - u[:, 1] * v[:, 0]))
        angle = math.atan2(cross, dot)
        c, s = math.cos(angle), math.sin(angle)
        # Rigid orientation only: preserve intrinsic UV length, area and handedness.
        rotated = u @ np.array([[c, s], [-s, c]]) + b
        charts.append({'ids': ids, 'uv': rotated.reshape((-1, 3, 2)), 'area': float(sum(area)), 'angle': angle})
    charts.sort(key=lambda x: -x['area'])
    # Curved pieces may overlap after restoring their physical arrangement.
    # Keep the main shape and move only conflicting smaller islands to an explicit gutter.
    grid=defaultdict(list)
    cell_size=64.
    accepted_bounds=[]
    def cells(face):
        low=np.floor(face.min(axis=0)/cell_size).astype(int)
        high=np.floor(face.max(axis=0)/cell_size).astype(int)
        return ((x,y) for x in range(low[0],high[0]+1) for y in range(low[1],high[1]+1))
    gutter_x = max(float(c['uv'][:, :, 0].max()) for c in charts) + 30
    gutter_y = min(float(c['uv'][:, :, 1].min()) for c in charts)
    gutter_width=max(250.,(gutter_x-min(float(c['uv'][:,:,0].min()) for c in charts))*.2)
    shelf_x,shelf_y,shelf_height=0.,gutter_y,0.
    moved = 0
    moved_area=0.
    def overlap_area(a, b):
        poly = list(a)
        def cross2(u, v): return float(u[0] * v[1] - u[1] * v[0])
        sign = 1 if cross2(b[1] - b[0], b[2] - b[0]) > 0 else -1
        for k in range(3):
            if not poly: return 0
            start, end = b[k], b[(k + 1) % 3]
            result = []
            previous = poly[-1]
            pd = cross2(end - start, previous - start) * sign
            for current in poly:
                cd = cross2(end - start, current - start) * sign
                if (pd >= 0) != (cd >= 0): result.append(previous + (current - previous) * pd / (pd - cd))
                if cd >= 0: result.append(current)
                previous, pd = current, cd
            poly = result
        return abs(sum(cross2(poly[k], poly[(k + 1) % len(poly)]) for k in range(len(poly)))) / 2 if len(poly) > 2 else 0
    accepted_uv = []
    for chart_index, chart in enumerate(charts):
        uv = chart['uv']
        conflict=False
        for face in uv:
            indices={i for key in cells(face) for i in grid.get(key,[])}
            low,high=face.min(axis=0),face.max(axis=0)
            if any(np.all(high>accepted_bounds[i][0]) and np.all(low<accepted_bounds[i][1]) and overlap_area(accepted_uv[i],face)>.001 for i in indices):
                conflict=True
                break
        if conflict:
            low = uv.reshape((-1, 2)).min(axis=0)
            extent=uv.reshape((-1,2)).max(axis=0)-low
            if shelf_x>0 and shelf_x+extent[0]>gutter_width:
                shelf_x=0.;shelf_y+=shelf_height+3.;shelf_height=0.
            uv += np.array([gutter_x+shelf_x, shelf_y]) - low
            shelf_x+=float(extent[0])+3.
            shelf_height=max(shelf_height,float(extent[1]))
            moved += 1
            moved_area+=chart['area']
        for i, face in zip(chart['ids'], uv):
            triangles[i]['paintUv'] = face.tolist()
            triangles[i]['chartId'] = f'{part}/chart/{chart_index}'
        for face in uv:
            index=len(accepted_uv)
            accepted_uv.append(face)
            accepted_bounds.append((face.min(axis=0),face.max(axis=0)))
            for key in cells(face): grid[key].append(index)
    coords = np.array([t['paintUv'] for t in triangles])
    low = coords.reshape((-1, 2)).min(axis=0)
    for t in triangles:
        t['paintUv'] = (np.array(t['paintUv']) - low).tolist()
    report['parts'].append({'partId': part, 'triangles': len(triangles), 'charts': len(charts), 'gutterCharts': moved,'gutterAreaM2':moved_area,'intrinsicFallbackTriangles':repaired,
                            'areaM2': sum(c['area'] for c in charts), 'dimensionsMm': (coords.reshape((-1, 2)).max(axis=0) - low).tolist()})
    print('ORIENT', part, len(charts), moved, flush=True)
document['paintFingerprint'] = ''
(output / 'paint-map-unsealed.json').write_text(json.dumps(document, separators=(',', ':')), encoding='utf-8')
(output / 'orientation-report.json').write_text(json.dumps(report, ensure_ascii=False, indent=2), encoding='utf-8')
