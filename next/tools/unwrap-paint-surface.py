"""Offline dedicated paint UV candidates. Run with Blender --background --python.

Keeps source triangle identities and material UV assets untouched. This output is
explicitly a candidate until the semantic part/seam review is completed.
"""
import bpy
import json
import math
import sys
from pathlib import Path
from collections import defaultdict
import numpy as np

args = sys.argv[sys.argv.index('--')+1:]
source, destination = map(Path, args[:2])
if destination.exists():
    raise RuntimeError('Refusing to overwrite existing candidate directory')
destination.mkdir(parents=True)
document = json.loads(source.read_text(encoding='utf-8'))
bpy.ops.object.select_all(action='SELECT')
bpy.ops.object.delete(use_global=False)
groups = {}
for triangle in document['triangles']:
    groups.setdefault(triangle['partId'], []).append(triangle)

def metrics(mesh):
    areas_w, areas_uv, anisotropy = [], [], []
    flips = 0
    uv_layer = mesh.uv_layers.active.data
    for face in mesh.polygons:
        uv = np.array([uv_layer[i].uv[:] for i in face.loop_indices], dtype=np.float64)
        p = np.array([mesh.vertices[i].co[:] for i in face.vertices], dtype=np.float64)
        e, f = p[1]-p[0], p[2]-p[0]
        l = np.linalg.norm(e)
        if l < 1e-12:
            continue
        xx = np.dot(f, e)/l
        yy = np.linalg.norm(np.cross(e, f))/l
        if yy < 1e-12:
            continue
        uv_matrix = np.column_stack((uv[1]-uv[0], uv[2]-uv[0]))
        det = np.linalg.det(uv_matrix)
        singular = np.linalg.svd(uv_matrix @ np.linalg.inv(np.array([[l, xx],[0., yy]])), compute_uv=False)
        areas_w.append(l*yy*.5)
        areas_uv.append(abs(det)*.5)
        anisotropy.append(float(singular[0]/max(singular[1],1e-20)))
        flips += int(det <= 0)
    return {'areaWorldM2':sum(areas_w),'areaUv':sum(areas_uv),'anisotropyP50':float(np.percentile(anisotropy,50)),
            'anisotropyP95':float(np.percentile(anisotropy,95)),'anisotropyMax':max(anisotropy),
            'negativeOrientationTriangles':flips,'triangles':len(mesh.polygons)}

def set_curvature_seams(mesh, angle_limit):
    edge_faces = defaultdict(list)
    for face in mesh.polygons:
        for edge_key in face.edge_keys:
            edge_faces[tuple(sorted(edge_key))].append(face)
    threshold = math.radians(angle_limit)
    for edge in mesh.edges:
        faces = edge_faces.get(tuple(sorted(edge.vertices)), [])
        edge.use_seam = len(faces) != 2 or faces[0].normal.angle(faces[1].normal) > threshold

def chart_count(mesh):
    parents = list(range(len(mesh.polygons)))
    def root(index):
        while parents[index] != index:
            parents[index] = parents[parents[index]]
            index = parents[index]
        return index
    edge_uv = defaultdict(list)
    layer = mesh.uv_layers.active.data
    for face in mesh.polygons:
        loops = list(face.loop_indices)
        for i, loop in enumerate(loops):
            nxt = loops[(i + 1) % len(loops)]
            edge = tuple(sorted((mesh.loops[loop].vertex_index, mesh.loops[nxt].vertex_index)))
            edge_uv[edge].append((face.index, {
                mesh.loops[loop].vertex_index: tuple(layer[loop].uv),
                mesh.loops[nxt].vertex_index: tuple(layer[nxt].uv),
            }))
    for entries in edge_uv.values():
        if len(entries) != 2:
            continue
        (a_face, a), (b_face, b) = entries
        if all(np.linalg.norm(np.asarray(a[v]) - np.asarray(b[v])) <= 1e-6 for v in a):
            parents[root(a_face)] = root(b_face)
    return len({root(i) for i in range(len(parents))})

report = {'status':'candidate-requires-semantic-review','source':str(source),'parts':[],
          'sourceMaterialUvModified':False,'connectionsReviewed':False,
          'cutStrategy':'automatic dihedral seams; candidate score = anisotropyP95 + 0.01 * chartCount'}
layout = {}
for part_id, triangles in groups.items():
    vertices, faces, lookup = [], [], {}
    for triangle in triangles:
        face=[]
        for point in triangle['world']:
            key=tuple(round(float(v),8) for v in point)
            if key not in lookup:
                lookup[key]=len(vertices);vertices.append(point)
            face.append(lookup[key])
        faces.append(face)
    mesh=bpy.data.meshes.new(part_id)
    mesh.from_pydata(vertices,[],faces);mesh.update()
    obj=bpy.data.objects.new(part_id,mesh);bpy.context.collection.objects.link(obj)
    bpy.context.view_layer.objects.active=obj;obj.select_set(True)
    mesh.uv_layers.new(name='PaintUV')
    candidates=[]
    for angle_limit in (20, 30, 40, 55, 70):
        set_curvature_seams(mesh, angle_limit)
        for method in ('ANGLE_BASED','CONFORMAL'):
            bpy.ops.object.mode_set(mode='EDIT');bpy.ops.mesh.select_all(action='SELECT')
            bpy.ops.uv.unwrap(method=method,margin=.003)
            bpy.ops.uv.pack_islands(rotate=True,margin=.003)
            bpy.ops.object.mode_set(mode='OBJECT')
            score=metrics(mesh)
            charts=chart_count(mesh)
            cost=score['anisotropyP95'] + .01*charts
            candidates.append((cost,angle_limit,method,score,charts,[tuple(x.uv) for x in mesh.uv_layers.active.data]))
    _, angle_limit, method, score, charts, uvs=min(candidates,key=lambda c:c[0])
    for loop,uv in zip(mesh.uv_layers.active.data,uvs):loop.uv=uv
    mm=math.sqrt(score['areaWorldM2']/max(score['areaUv'],1e-20))*1000
    repaired=0
    for triangle,face in zip(triangles,mesh.polygons):
        triangle['paintUv']=[[uvs[i][0]*mm,uvs[i][1]*mm] for i in face.loop_indices]
        uv=np.array(triangle['paintUv'])
        p=np.array(triangle['world'],dtype=np.float64)
        e,f=p[1]-p[0],p[2]-p[0]
        world_area=np.linalg.norm(np.cross(e,f))/2
        det=np.linalg.det(np.column_stack((uv[1]-uv[0],uv[2]-uv[0])))
        if not math.isfinite(det) or abs(det)<1e-10 or abs(det)/max(world_area*2e6,1e-30)<.01:
            # Tiny clipped fragments may fail a solver even when their source
            # geometry is valid. Preserve them as explicit intrinsic charts.
            length=np.linalg.norm(e)
            x=mm+100+repaired*10
            triangle['paintUv']=[[x,0.],[x+length*1000,0.],[x+float(np.dot(f,e)/length)*1000,float(world_area*2/length)*1000]]
            repaired+=1
        triangle['chartId']=part_id
    coordinates=[uv for t in triangles for uv in t['paintUv']]
    bounds=[[min(uv[k] for uv in coordinates) for k in range(2)],[max(uv[k] for uv in coordinates) for k in range(2)]]
    for triangle in triangles:
        triangle['paintUv']=[[uv[k]-bounds[0][k] for k in range(2)] for uv in triangle['paintUv']]
    layout[part_id]={'widthMm':bounds[1][0]-bounds[0][0],'heightMm':bounds[1][1]-bounds[0][1]}
    report['parts'].append({'partId':part_id,'selectedSolver':method,'selectedDihedralLimitDegrees':angle_limit,
                            'selectedChartCount':charts,
                            'candidates':[{'method':c[2],'dihedralLimitDegrees':c[1],**c[3],
                                           'chartCount':c[4],'selectionCost':c[0]} for c in candidates],
                            'semanticStatus':'provisional-runtime-partition','intrinsicFallbackTriangles':repaired,'dimensions':layout[part_id]})
    obj.select_set(False)
    print('PAINT_UNWRAP',part_id,method,angle_limit,'charts',charts,'p95',score['anisotropyP95'],flush=True)
document['paintFingerprint']='' # Sealed by Rust, which owns the canonical fingerprint format.
(destination/'paint-map-unsealed.json').write_text(json.dumps(document,separators=(',',':')),encoding='utf-8')
(destination/'unwrap-report.json').write_text(json.dumps(report,ensure_ascii=False,indent=2),encoding='utf-8')
(destination/'chart-layout.json').write_text(json.dumps(layout,indent=2),encoding='utf-8')
bpy.ops.wm.save_as_mainfile(filepath=str(destination/'paint-unfolding-candidate.blend'))
print('PAINT_UNWRAP_COMPLETE',destination,flush=True)
