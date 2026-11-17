"""Recover sampled exterior fragments omitted by the old radial-normal filter.

Reads source GLB and a sealed map, subtracts existing barycentric fragments,
then clips missing domains against external visibility. Output must be unwrapped
and audited before the application uses it. No original GLB/UV is overwritten.
"""
import sys
import json
import math
import struct
import hashlib
import runpy
from pathlib import Path
from collections import defaultdict
import numpy as np
from mathutils import Vector

source, mapping, output = map(Path, sys.argv[sys.argv.index('--')+1:][:3])
if output.exists(): raise RuntimeError('Output already exists')
audit_file = output.with_suffix('.source-audit.json')
sys.argv = ['audit-paint-completeness.py', '--', str(source), str(mapping), str(audit_file)]
context = runpy.run_path(str(Path(__file__).with_name('audit-paint-completeness.py')))
document = context['mapping_doc']
source_faces, tree, cameras = context['source_faces'], context['tree'], context['cameras']
by_source = defaultdict(list)
for t in document['triangles']:
    by_source[(t['id']['sourceMesh'], t['id']['sourceTriangle'])].append(t)
# Component identities are inherited only where an existing part already owns
# a substantial and unambiguous amount of that source component.
parents = list(range(len(context['vertices'])))
def root(i):
    while parents[i] != i:
        parents[i] = parents[parents[i]]
        i = parents[i]
    return i
for face in context['faces']:
    first = root(face[0])
    for v in face[1:]: parents[root(v)] = first
owners = defaultdict(lambda: defaultdict(float))
source_index = {}
for i, (mesh, face, name, paint, world) in enumerate(source_faces):
    source_index[(mesh, face)] = i
    component = root(context['faces'][i][0])
    for t in by_source[(mesh, face)]:
        w = np.array(t['world'])
        owners[component][t['partId']] += float(np.linalg.norm(np.cross(w[1]-w[0], w[2]-w[0]))/2)
# External visibility through a grille/seam does not turn an internal panel into
# an exterior body part. Unregistered source meshes remain audit findings.
node_part = {}
mesh_parts=defaultdict(set)
for t in document['triangles']: mesh_parts[t['id']['sourceMesh']].add(t['partId'])
for mesh, parts in mesh_parts.items():
    if len(parts)==1: node_part[mesh]=next(iter(parts))
def target_part(mesh, face):
    existing = by_source[(mesh, face)]
    ids = {t['partId'] for t in existing}
    if len(ids) == 1: return next(iter(ids))
    i = source_index[(mesh, face)]
    amounts = owners[root(context['faces'][i][0])]
    if amounts:
        part, area = max(amounts.items(), key=lambda e:e[1])
        if area > .01 and area/sum(amounts.values()) > .98: return part
    return node_part.get(mesh)

def cross(a, b): return float(a[0]*b[1]-a[1]*b[0])
def split(poly, a, b):
    inside, outside = [], []
    previous = poly[-1]
    pd = cross(b-a, previous-a)
    for current in poly:
        cd = cross(b-a, current-a)
        if (pd>=0)!=(cd>=0):
            point = previous+(current-previous)*pd/(pd-cd)
            inside.append(point); outside.append(point)
        (inside if cd>=0 else outside).append(current)
        previous,pd = current,cd
    return inside,outside
def subtract(poly, clip):
    if cross(clip[1]-clip[0],clip[2]-clip[0]) < 0: clip=clip[[0,2,1]]
    remains=[]
    for e in range(3):
        if len(poly)<3: break
        poly, outside = split(poly, clip[e],clip[(e+1)%3])
        if len(outside)>=3: remains.append(outside)
    return remains

result=[]; unresolved=[]; totals=defaultdict(float)
for audit in context['report']['meshes']:
    mesh = audit['sourceMesh']
    for sample in audit['samples']:
        face = sample['face']
        part = target_part(mesh, face)
        if not part:
            unresolved.append({'sourceMesh':mesh,'face':face,'areaM2':sample['areaM2']-sample['mappedAreaM2']})
            continue
        world = source_faces[source_index[(mesh,face)]][4]
        normal = Vector(np.cross(world[1]-world[0],world[2]-world[0])).normalized()
        polygons = [[np.array([0.,0.]),np.array([1.,0.]),np.array([0.,1.])]]
        for t in by_source[(mesh,face)]:
            clip = np.array(t['sourceBarycentric'])[:,1:]
            polygons = [rest for poly in polygons for rest in subtract(poly,clip)]
        def point(uv): return world[0]*(1-uv[0]-uv[1])+world[1]*uv[0]+world[2]*uv[1]
        visibility_cache={}
        def visible(uv):
            key=tuple(np.round(uv,10))
            if key in visibility_cache: return visibility_cache[key]
            p=Vector(point(uv))
            for camera in cameras:
                direction=p-camera
                distance=direction.length
                if normal.dot(-direction.normalized())<.02: continue
                hit,_,_,_=tree.ray_cast(camera,direction.normalized(),distance+.001)
                if hit is not None and (hit-p).length<.0001:
                    visibility_cache[key]=True; return True
            visibility_cache[key]=False; return False
        def recover(uv,depth=0):
            uv=np.array(uv)
            p=np.array([point(v) for v in uv])
            area=float(np.linalg.norm(np.cross(p[1]-p[0],p[2]-p[0]))/2)
            if area<1e-12: return
            center=uv.mean(axis=0)
            bits=[visible(v) for v in uv]+[visible(center)]
            max_edge=max(np.linalg.norm(p[i]-p[(i+1)%3]) for i in range(3))
            if depth<9 and max_edge>.004 and any(b!=bits[3] for b in bits[:3]):
                m=(uv+np.roll(uv,-1,axis=0))/2
                for child in ([uv[0],m[0],m[2]],[m[0],uv[1],m[1]],[m[2],m[1],uv[2]],[m[0],m[1],m[2]]): recover(child,depth+1)
                return
            if not bits[3]: return
            p=p.astype(np.float32)
            e,f=p[1]-p[0],p[2]-p[0]
            length=float(np.linalg.norm(e))
            area=float(np.linalg.norm(np.cross(e,f))/2)
            if length<1e-7 or area<1e-11: return
            fragment=int.from_bytes(hashlib.sha256(p.tobytes()).digest()[:8],'little')
            # Each recovered triangle starts with an intrinsic, nondegenerate local UV.
            uv_local=[[0.,0.],[length*1000,0.],[float(np.dot(f,e)/length)*1000,2*area/length*1000]]
            result.append({'id':{'sourceMesh':mesh,'primitive':0,'sourceTriangle':face,'fragment':fragment},
                           'partId':part,'regionId':part+'/exterior','chartId':part,'world':p.tolist(),
                           'sourceBarycentric':[[float(1-v[0]-v[1]),float(v[0]),float(v[1])] for v in uv], 'paintUv':uv_local})
            totals[part]+=area
        for poly in polygons:
            for i in range(1,len(poly)-1): recover([poly[0],poly[i],poly[i+1]])
    print('RECOVER',mesh,len(result),flush=True)
document['triangles'].extend(result)
document['paintFingerprint']=''
document['connections']=[]
output.write_text(json.dumps(document,separators=(',',':')),encoding='utf-8')
output.with_suffix('.recovery-report.json').write_text(json.dumps({'recoveredFragments':len(result),'recoveredAreaM2':dict(totals),'unresolvedFaces':unresolved,'status':'visibility-sampled-requires-semantic-review'},indent=2),encoding='utf-8')
