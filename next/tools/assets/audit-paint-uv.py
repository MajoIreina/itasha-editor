"""Blender offline inspection of dedicated UVs. Does not modify source or map.

blender --background --python tools/assets/audit-paint-uv.py -- map.json output-directory
"""
import json
import sys
import struct
from pathlib import Path
from collections import defaultdict
from mathutils import Vector
from mathutils.bvhtree import BVHTree
from mathutils.kdtree import KDTree
sys.path.insert(0, str(Path(__file__).resolve().parent))
from paint_uv_geometry import positive_overlaps

source, output = map(Path, sys.argv[sys.argv.index('--')+1:][:2])
output.mkdir(parents=True, exist_ok=False)
document = json.loads(source.read_text(encoding='utf-8'))
# Rust serializes shortest decimals that round-trip to f32. Reconstruct those
# actual runtime coordinates before testing: interpreting them as f64 instead
# introduces different rounding on shared edges and reports artificial slivers.
for triangle in document['triangles']:
    triangle['paintUv'] = [[struct.unpack('<f', struct.pack('<f', v))[0] for v in p] for p in triangle['paintUv']]
groups = defaultdict(list)
for triangle in document['triangles']:
    groups[triangle['partId']].append(triangle)

def cross(a,b):
    return a[0]*b[1]-a[1]*b[0]

def overlap_area(a,b):
    # Strict positive area only. Shared seam vertices/edges are legal.
    poly = [Vector(p) for p in a]
    orientation = 1 if cross(Vector(b[1])-Vector(b[0]),Vector(b[2])-Vector(b[0])) > 0 else -1
    for i in range(3):
        start,end=Vector(b[i]),Vector(b[(i+1)%3])
        if not poly:return 0
        result=[]
        previous=poly[-1]
        pd=cross(end-start,previous-start)*orientation
        for current in poly:
            cd=cross(end-start,current-start)*orientation
            if (pd>=0)!=(cd>=0):result.append(previous.lerp(current,pd/(pd-cd)))
            if cd>=0:result.append(current)
            previous,pd=current,cd
        poly=result
    return abs(sum(cross(poly[i],poly[(i+1)%len(poly)]) for i in range(len(poly))))/2 if len(poly)>2 else 0

report={'source':str(source),'status':'algorithmic-audit-not-semantic-approval','parts':[]}
boundaries={}
for part,triangles in groups.items():
    vertices=[(p[0],p[1],0) for t in triangles for p in t['paintUv']]
    overlaps=[{'a':a,'b':b,'areaMm2':area} for a,b,area in positive_overlaps([t['paintUv'] for t in triangles])]
    edges=defaultdict(list)
    for i,t in enumerate(triangles):
        for edge in range(3):
            a,b=t['world'][edge],t['world'][(edge+1)%3]
            key=tuple(sorted((tuple(round(v,6) for v in a),tuple(round(v,6) for v in b))))
            edges[key].append((i,edge))
    boundaries[part]=[entry[0] for entry in edges.values() if len(entry)==1]
    parents=list(range(len(triangles)))
    def root(index):
        while parents[index]!=index:
            parents[index]=parents[parents[index]]
            index=parents[index]
        return index
    for entries in edges.values():
        if len(entries)>1:
            first=root(entries[0][0])
            for ti,_ in entries[1:]:parents[root(ti)]=first
    world_points=[p for t in triangles for p in t['world']]
    uv_points=[p for t in triangles for p in t['paintUv']]
    world_bounds=[[min(p[i] for p in world_points) for i in range(3)],[max(p[i] for p in world_points) for i in range(3)]]
    uv_bounds=[[min(p[i] for p in uv_points) for i in range(2)],[max(p[i] for p in uv_points) for i in range(2)]]
    world_area=sum((Vector(t['world'][1])-Vector(t['world'][0])).cross(Vector(t['world'][2])-Vector(t['world'][0])).length/2 for t in triangles)
    uv_area=sum(abs(cross(Vector(t['paintUv'][1])-Vector(t['paintUv'][0]),Vector(t['paintUv'][2])-Vector(t['paintUv'][0])))/2 for t in triangles)
    report['parts'].append({'partId':part,'triangles':len(triangles),'positiveAreaOverlapCount':len(overlaps),'overlapSamples':overlaps[:20],
        'boundaryEdges':len(boundaries[part]),'nonmanifoldEdges':sum(len(v)>2 for v in edges.values()),'connectedComponents':len({root(i) for i in range(len(triangles))}),
        'worldBoundsM':world_bounds,'paintUvBoundsMm':uv_bounds,'worldSurfaceAreaM2':world_area,'paintUvAreaMm2':uv_area,
        'uvToWorldAreaScale':uv_area/max(world_area*1_000_000,1e-12)})
    print('UV_AUDIT',part,len(overlaps),flush=True)

pairs=[('front-left-door','rear-left-door'),('front-right-door','rear-right-door'),('hood','front-left-fender'),('hood','front-right-fender')]
connections=[]
for name_a,name_b in pairs:
    part_a,part_b='su7.'+name_a,'su7.'+name_b
    a,b=groups[part_a],groups[part_b]
    b_edges=boundaries[part_b]
    tree=KDTree(len(b_edges))
    for i,(ti,e) in enumerate(b_edges):
        t=b[ti];mid=(Vector(t['world'][e])+Vector(t['world'][(e+1)%3]))/2
        tree.insert(mid,i)
    tree.balance()
    candidates=[]
    for ai,ae in boundaries[part_a]:
        ta=a[ai];aw=[Vector(ta['world'][ae]),Vector(ta['world'][(ae+1)%3])]
        da=aw[1]-aw[0]
        if da.length<0.003:continue
        for _,bi,distance in tree.find_range((aw[0]+aw[1])/2,0.015):
            bt,be=b_edges[bi];tb=b[bt];bw=[Vector(tb['world'][be]),Vector(tb['world'][(be+1)%3])]
            db=bw[1]-bw[0]
            if db.length<0.003 or abs(da.length-db.length)>0.004 or abs(da.normalized().dot(db.normalized()))<0.97:continue
            reversed_pair=(aw[0]-bw[1]).length+(aw[1]-bw[0]).length < (aw[0]-bw[0]).length+(aw[1]-bw[1]).length
            if reversed_pair:bw.reverse()
            gap=((aw[0]-bw[0]).length+(aw[1]-bw[1]).length)/2
            if max((aw[0]-bw[0]).length,(aw[1]-bw[1]).length)>0.018:continue
            candidates.append((gap,ai,ae,bt,be,reversed_pair))
    used_a,used_b=set(),set()
    for gap,ai,ae,bi,be,reverse in sorted(candidates):
        if (ai,ae) in used_a or (bi,be) in used_b:continue
        used_a.add((ai,ae));used_b.add((bi,be))
        connections.append({'id':f'{name_a}--{name_b}--{ai}-{ae}-{bi}-{be}','from':{'triangle':a[ai]['id'],'edge':ae},'to':{'triangle':b[bi]['id'],'edge':be},'reversed':reverse,'physicalGapMm':gap*1000,'propagation':False})
    print('SEAM_CANDIDATES',name_a,name_b,len(used_a),flush=True)
report['candidateConnections']=len(connections)
(output/'uv-audit.json').write_text(json.dumps(report,ensure_ascii=False,indent=2),encoding='utf-8')
(output/'seam-candidates.json').write_text(json.dumps({'status':'requires-review','connections':connections},ensure_ascii=False,indent=2),encoding='utf-8')

# Source-space review views keep screen UV packing out of semantic decisions.
palette=['#248fe4','#ec9828','#d95978','#70ba78','#b48ee5','#65caca']
ids={str(t['id']):t for triangles in groups.values() for t in triangles}
for view in ['left','right','top']:
    svg=['<svg xmlns="http://www.w3.org/2000/svg" width="1600" height="1000" viewBox="0 0 1600 1000"><rect width="1600" height="1000" fill="#181818"/>']
    project=lambda p: ((p[0]+3)*245,700-p[1]*370) if view!='top' else ((p[2]+1.6)*450,(p[0]+3)*145)
    for n,(part,triangles) in enumerate(groups.items()):
        for t in triangles:
            center=sum(p[2] for p in t['world'])/3
            if view=='left' and center<0 or view=='right' and center>0:continue
            p=[project(p) for p in t['world']]
            svg.append('<polygon points="'+' '.join(f'{x:.2f},{y:.2f}' for x,y in p)+f'" fill="{palette[n%len(palette)]}" fill-opacity="0.7"/>')
    for c in connections:
        t=ids[str(c['from']['triangle'])];e=c['from']['edge'];a,b=t['world'][e],t['world'][(e+1)%3]
        if view=='left' and (a[2]+b[2])/2<0 or view=='right' and (a[2]+b[2])/2>0:continue
        pa,pb=project(a),project(b)
        svg.append(f'<path d="M {pa[0]},{pa[1]} L {pb[0]},{pb[1]}" stroke="#fff" stroke-width="3"/>')
    svg.append(f'<text x="25" y="30" fill="white" font-size="22">{view}: provisional paint regions / white = proposed seam pairs</text></svg>')
    (output/f'semantics-{view}.svg').write_text(''.join(svg),encoding='utf-8')
