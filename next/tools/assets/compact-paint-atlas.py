"""Compact recovered charts and resolve intersections at actual float32 precision.

Only paintUv/chartId change. World geometry and source barycentrics are preserved.
Tiny numerical fragments receive separate charts, never deleted geometry.
python tools/assets/compact-paint-atlas.py sealed-map.json new-directory
"""
import json
import sys
import math
from pathlib import Path
from collections import defaultdict
import numpy as np
from paint_uv_geometry import positive_overlaps, overlap_area

source, output = map(Path, sys.argv[1:3])
output.mkdir(parents=True, exist_ok=False)
document = json.loads(source.read_text(encoding='utf-8'))
groups = defaultdict(list)
for t in document['triangles']:
    groups[t['partId']].append(t)
report = {'source': str(source), 'worldAndSourceIdentityUnchanged': True, 'parts': []}

def physical(part, world):
    # The model axes are X=longitudinal, Y=up, Z=lateral. In a top-down
    # front-up atlas, vehicle-front (smaller X) maps toward the top. The
    # upper/window edge on each side faces the centerline.
    if any(key in part for key in ('door', 'rear-fender', 'sill', 'lower-fender')):
        # Rotate the side elevation so its upper/window edge faces inward.
        # Front is negative X, therefore increasing X runs down the canvas.
        inward = world[:, 1] if '-left-' in part or part.endswith('left-sill') else -world[:, 1]
        return np.column_stack((inward, world[:, 0])) * 1000
    if 'front-fender' in part:
        return np.column_stack((-world[:, 2], world[:, 0])) * 1000
    return np.column_stack((-world[:, 2], world[:, 0])) * 1000

def f32(uv):
    return np.asarray(uv, dtype=np.float32).astype(float)

def valid(uv):
    a,b,c = np.asarray(uv, dtype=np.float32)
    u,v = b-a,c-a
    return abs(float(u[0]*v[1]-u[1]*v[0])) >= 1e-12

def intrinsic(t):
    w=np.array(t['world']); edges=[(i,(i+1)%3) for i in range(3)]
    i,j=max(edges,key=lambda e:np.linalg.norm(w[e[0]]-w[e[1]])); k=3-i-j
    e,f=w[j]-w[i],w[k]-w[i]; length=np.linalg.norm(e)
    uv=np.zeros((3,2));uv[j]=[length*1000,0]
    uv[k]=[np.dot(f,e)/length*1000,np.linalg.norm(np.cross(e,f))/length*1000]
    return uv

def orient_chart(uv, ids, target, areas, offset):
    """Rigid 2D fit to model coordinates, including mirror when required."""
    flat=np.asarray(uv,dtype=float).reshape((-1,2))
    destination=target[ids].reshape((-1,2))
    weights=np.repeat(np.maximum(areas[ids],1e-15),3)
    source_center=np.average(flat,axis=0,weights=weights)
    target_center=np.average(destination,axis=0,weights=weights)
    covariance=(flat-source_center).T @ ((destination-target_center)*weights[:,None])
    left,_,right=np.linalg.svd(covariance)
    orthogonal=left @ right
    return f32((flat-source_center) @ orthogonal + target_center-offset).reshape((-1,3,2))

for part, ts in groups.items():
    worlds=np.array([t['world'] for t in ts]); areas=np.linalg.norm(np.cross(worlds[:,1]-worlds[:,0],worlds[:,2]-worlds[:,0]),axis=1)/2
    target=physical(part, worlds.reshape((-1,3))).reshape((-1,3,2))
    offset=target.reshape((-1,2)).min(axis=0)-30
    charts=defaultdict(list); micros=[]; micro_uv={}; precision=[]
    for i,t in enumerate(ts):
        if '/micro/' in t['chartId']:
            micros.append(i);micro_uv[i]=np.array(t['paintUv'],dtype=float)
        else:
            charts[t['chartId']].append(i)
    accepted=[]; bounds=[]; grid=defaultdict(list); uv_by_id={}; detached=[]; kept=0
    def cells(tri):
        low=np.floor(tri.min(axis=0)/64).astype(int); high=np.floor(tri.max(axis=0)/64).astype(int)
        return [(x,y) for x in range(low[0],high[0]+1) for y in range(low[1],high[1]+1)]
    def conflicts(uv):
        for tri in uv:
            low,high=tri.min(axis=0),tri.max(axis=0)
            for j in {j for key in cells(tri) for j in grid[key]}:
                lo,hi=bounds[j]
                if np.all(high>lo) and np.all(low<hi) and overlap_area(tri,accepted[j])>.0001:
                    return True
        return False
    def insert(uv):
        for tri in uv:
            j=len(accepted);accepted.append(tri);bounds.append((tri.min(axis=0),tri.max(axis=0)))
            for key in cells(tri):grid[key].append(j)
    for name, ids in sorted(charts.items(),key=lambda entry:-sum(areas[entry[1]])):
        uv=np.array([ts[i]['paintUv'] for i in ids])
        uv=orient_chart(uv,ids,target,areas,offset)
        bad={i for i,tri in enumerate(uv) if not valid(tri)}
        for i,j,_ in positive_overlaps(uv):
            bad.add(i if areas[ids[i]]<areas[ids[j]] else j)
        for i in bad:
            detached.append(([ids[i]],orient_chart(intrinsic(ts[ids[i]])[None,:,:],[ids[i]],target,areas,offset)))
        good=[i for i in range(len(ids)) if i not in bad]
        if not good:continue
        ids=[ids[i] for i in good];uv=uv[good]
        if conflicts(uv):
            detached.append((ids,uv))
        else:
            kept+=1; insert(uv)
            for i,tri in zip(ids,uv):uv_by_id[i]=tri
    # Height-sorted best-fit shelves use the full part width. Sorting by area in a
    # narrow column previously stretched small supplemental fragments over metres.
    main=np.array(list(uv_by_id.values())).reshape((-1,2))
    low=main.min(axis=0); high=main.max(axis=0)
    # Supplemental strips can be rotated independently; they have explicit chart
    # identities and retain exactly the same reversible triangle mapping.
    for n,(ids,uv) in enumerate(detached):
        extent=np.ptp(uv.reshape((-1,2)),axis=0)
        if extent[1]>extent[0]:
            detached[n]=(ids,uv @ np.array([[0.,1.],[-1.,0.]]))
    width=max(250.,float(high[0]-low[0]),*(float(np.ptp(uv.reshape((-1,2)),axis=0)[0]) for _,uv in detached))
    bottom=high[1]+12.; shelves=[];supplement_area=0.
    detached.sort(key=lambda chart:-float(np.ptp(chart[1].reshape((-1,2)),axis=0)[1]))
    for n,(ids,uv) in enumerate(detached):
        lo=uv.reshape((-1,2)).min(axis=0); extent=np.ptp(uv.reshape((-1,2)),axis=0)
        candidates=[s for s in shelves if s['h']>=extent[1] and s['x']+extent[0]<=width]
        if candidates:
            shelf=min(candidates,key=lambda s:width-s['x']-extent[0])
        else:
            shelf={'x':0.,'y':bottom,'h':float(extent[1])};shelves.append(shelf);bottom+=float(extent[1])+1.
        placed=f32(uv-lo+np.array([low[0]+shelf['x'],shelf['y']]))
        shelf['x']+=float(extent[0])+1.
        for local,(i,tri) in enumerate(zip(ids,placed)):
            if valid(tri):
                uv_by_id[i]=tri;ts[i]['chartId']=f'{part}/supplement/{n}'
                supplement_area+=areas[i]
            else:
                local_uv=f32(uv[local])
                precision.append((i,local_uv if valid(local_uv) else f32(intrinsic(ts[i]))))
    # Low-coordinate strip prevents float32 area collapse for very thin fragments.
    # New chart identities keep these physically separate from their former chart.
    x=0.
    if micros:
        x=max(float(np.asarray(micro_uv[i])[:,0].max()) for i in micros)+10.
    for i in micros:
        # These coordinates were already placed in a reserved low-coordinate
        # strip by the f32 finalizer. Keep the exact values; rotating a sub-ulp
        # sliver can erase its area even when the original coordinates survive.
        placed=f32(micro_uv.get(i,np.array(ts[i]['paintUv'],dtype=float)))
        extent=np.ptp(placed,axis=0)
        assert extent[1]<20 and valid(placed), (part,i,extent)
        uv_by_id[i]=placed;ts[i]['chartId']=f'{part}/micro/{i}'
    for i,local_uv in precision:
        extent=np.ptp(local_uv,axis=0)
        if extent[1]>=20 and extent[0]<20:
            local_uv=local_uv @ np.array([[0.,1.],[-1.,0.]])
            extent=np.ptp(local_uv,axis=0)
        lo=local_uv.min(axis=0)
        placed=f32(local_uv-lo+np.array([x-float(extent[0])-3.,0.]))
        x-=float(extent[0])+3.
        assert valid(placed),(part,i,extent,placed)
        uv_by_id[i]=placed;ts[i]['chartId']=f'{part}/precision/{i}'
        bottom+=float(np.ptp(placed,axis=0)[1])+3.
    assert len(uv_by_id)==len(ts)
    for i,t in enumerate(ts):t['paintUv']=uv_by_id[i].tolist()
    # Validate final values after every translation and float32 conversion.
    overlaps=positive_overlaps([t['paintUv'] for t in ts])
    # A translation can round a formerly shared edge in opposite directions.
    # Preserve such small faces in the exact strip rather than tolerating overlap.
    repairs=0
    while overlaps:
        bad={i if areas[i]<areas[j] else j for i,j,_ in overlaps}
        for i in bad:
            uv=intrinsic(ts[i]);lo=uv.min(axis=0);extent=np.ptp(uv,axis=0)
            if extent[1]<20:
                placed=f32(uv-lo+np.array([x,0.]));x+=float(extent[0])+1.
            else:
                placed=f32(uv-lo+np.array([low[0],bottom]));bottom+=float(extent[1])+1.
            assert valid(placed)
            ts[i]['paintUv']=placed.tolist();ts[i]['chartId']=f'{part}/precision/{i}'
        repairs+=len(bad)
        assert repairs<len(ts)
        overlaps=positive_overlaps([t['paintUv'] for t in ts])
    all_uv=np.array([t['paintUv'] for t in ts]);assert all(valid(uv) for uv in all_uv)
    entry={'partId':part,'triangles':len(ts),'mainCharts':kept,'supplementCharts':len(detached),'microTriangles':len(micros),'precisionStripTriangles':len(precision),
           'supplementAreaM2':float(supplement_area),'areaM2':float(sum(areas)),
           'dimensionsMm':all_uv.reshape((-1,2)).max(axis=0).tolist(),'positiveAreaOverlaps':len(overlaps),'precisionRepairs':repairs}
    report['parts'].append(entry);print('COMPACT',entry,flush=True)
document['paintFingerprint']=''
(output/'paint-map-unsealed.json').write_text(json.dumps(document,separators=(',',':')),encoding='utf-8')
(output/'compact-report.json').write_text(json.dumps(report,indent=2),encoding='utf-8')
