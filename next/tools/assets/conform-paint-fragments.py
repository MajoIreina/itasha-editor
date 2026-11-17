"""Build a new offline conforming map using source-face/index-edge identities.

Never modifies the input map or source GLB. Interior point coincidence is tested
only inside one source triangle. Cross-face cuts use the GLB's actual indices;
spatially nearby disconnected sheets are never welded. Explicit seam edges are
protected from subdivision. New repair UV charts are packed outside
the preserved charts so coverage cannot select an unrelated panel.
"""
from __future__ import annotations

import argparse
from collections import Counter, defaultdict
import copy
from fractions import Fraction
import json
from pathlib import Path
import runpy
import time

import numpy as np
import shapely
from shapely.geometry import Polygon
from shapely.ops import unary_union
from shapely.strtree import STRtree

AUDIT = runpy.run_path(str(Path(__file__).with_name('audit-paint-integrity.py')))
TRIANGULATION_FALLBACKS=[]


def key(triangle_id):
    return (triangle_id['sourceMesh'], triangle_id['primitive'], triangle_id['sourceTriangle'], triangle_id['fragment'])


def source_determinant(bary):
    """Homogeneous source area, with exact-zero proof for near-linear f32 rows.

The first barycentric coordinate cannot be discarded before normalizing rows:
f32 row-sum drift otherwise turns a true source edge into a thin fake polygon.
"""
    determinant=float(np.linalg.det(bary))
    if abs(determinant)<1.e-10:
        b=[[Fraction(float(v)) for v in row] for row in bary]
        determinant=float(b[0][0]*(b[1][1]*b[2][2]-b[1][2]*b[2][1])
                          -b[0][1]*(b[1][0]*b[2][2]-b[1][2]*b[2][0])
                          +b[0][2]*(b[1][0]*b[2][1]-b[1][1]*b[2][0]))
    return determinant/float(np.prod(bary.sum(axis=1)))


def source_triangles(geometry):
    """Triangulate without discarding positive-area sub-threshold polygons."""
    if geometry.geom_type!='Polygon':
        for member in getattr(geometry,'geoms',[]):
            yield from source_triangles(member)
        return
    coordinates=list(geometry.exterior.coords[:-1])
    if not geometry.interiors and len(coordinates)==3:
        yield geometry
        return
    try:
        yield from shapely.constrained_delaunay_triangles(geometry).geoms
        return
    except shapely.errors.GEOSException:
        TRIANGULATION_FALLBACKS.append({'sourceGeometry':shapely.to_geojson(geometry),
                                       'reason':'GEOS-convex-corner-failure-exact-orientation-ear-clipping'})
    if geometry.interiors:
        # Every boundary vertex participates in this Delaunay tessellation;
        # clipping its convex cells leaves hole-free simple polygons.
        from shapely.ops import triangulate
        for cell in triangulate(geometry):
            clipped=cell.intersection(geometry)
            if clipped.equals(geometry):
                raise ValueError('Unable to decompose source polygon holes without changing its domain')
            yield from source_triangles(clipped)
        return
    points=[]
    for point in coordinates:
        if not points or point!=points[-1]:
            points.append(point)
    if len(points)>1 and points[0]==points[-1]:
        points.pop()
    rational=[[Fraction(float(v)) for v in p] for p in points]
    def orientation(a,b,c):
        x,y,z=rational[a],rational[b],rational[c]
        return (y[0]-x[0])*(z[1]-x[1])-(y[1]-x[1])*(z[0]-x[0])
    area=sum(p[0]*q[1]-p[1]*q[0] for p,q in zip(rational,rational[1:]+rational[:1]))
    if area==0:
        return
    indices=list(range(len(points)))
    if area<0:
        indices.reverse()
    while len(indices)>3:
        for i,b in enumerate(indices):
            a,c=indices[i-1],indices[(i+1)%len(indices)]
            if orientation(a,b,c)<=0:
                continue
            if any(orientation(a,b,p)>=0 and orientation(b,c,p)>=0 and orientation(c,a,p)>=0
                   for p in indices if p not in (a,b,c)):
                continue
            yield Polygon([points[a],points[b],points[c]])
            indices.pop(i)
            break
        else:
            raise ValueError('Exact source polygon ear clipping found no valid ear')
    if len(indices)==3 and orientation(*indices)!=0:
        yield Polygon([points[i] for i in indices])


def pack_repairs(triangles):
    groups = defaultdict(list)
    bounds = {}
    for index, triangle in enumerate(triangles):
        if triangle['regionId'].endswith('/reviewed-source-v25') and triangle['partId'] != 'su7.front-left-lower-fender':
            groups[(triangle['partId'], triangle['chartId'])].append(index)
        else:
            p = np.asarray(triangle['paintUv']); part = triangle['partId']
            if part not in bounds:
                bounds[part] = [p.min(axis=0),p.max(axis=0)]
            else:
                bounds[part] = [np.minimum(bounds[part][0],p.min(axis=0)),np.maximum(bounds[part][1],p.max(axis=0))]
    jobs = []
    for (part, old_chart), indices in sorted(groups.items()):
        # Sparse repairs can span the entire source UV sheet. Pack their local
        # connected patches separately so a few edge fragments do not create a
        # several-metre blank 2D chart. This grouping never changes geometry.
        parents=list(range(len(indices)))
        def root(i):
            while parents[i] != i:
                parents[i]=parents[parents[i]];i=parents[i]
            return i
        vertices={}
        for local,index in enumerate(indices):
            for point in triangles[index]['paintUv']:
                vertex=tuple(np.round(np.asarray(point)*100.).astype(np.int64))
                if vertex in vertices:
                    parents[root(local)]=root(vertices[vertex])
                else:
                    vertices[vertex]=local
        connected=defaultdict(list)
        for local,index in enumerate(indices):
            connected[root(local)].append(index)
        for component,component_indices in enumerate(connected.values()):
            polygons=[Polygon(triangles[i]['paintUv']) for i in component_indices]
            tree=STRtree(polygons);colors={}
            for i,polygon in enumerate(polygons):
                forbidden={colors[int(j)] for j in tree.query(polygon) if int(j)<i and polygon.intersection(polygons[int(j)]).area>1.e-5}
                colors[i]=next(c for c in range(len(forbidden)+1) if c not in forbidden)
            for color in sorted(set(colors.values())):
                subgroup=[component_indices[i] for i in range(len(component_indices)) if colors[i]==color]
                points=np.concatenate([np.asarray(triangles[i]['paintUv']) for i in subgroup])
                low,high=points.min(axis=0),points.max(axis=0)
                jobs.append((part,old_chart,component,color,subgroup,low,high))
    report=[];cursors={}
    for part,old_chart,component,color,subgroup,low,high in sorted(jobs,key=lambda j:(j[0],-float(j[6][1]-j[5][1]),-float(j[6][0]-j[5][0]),j[1],j[2],j[3])):
        width,height=high-low
        if part not in cursors:
            old=bounds.get(part,[np.zeros(2),np.zeros(2)])
            max_patch_width=max(float(j[6][0]-j[5][0]) for j in jobs if j[0]==part)
            cursors[part]=[0.,float(old[1][1]+10.),0.,float(max(1500.,old[1][0]-old[0][0],max_patch_width))]
        cursor=cursors[part]
        if cursor[0]>0. and cursor[0]+width>cursor[3]:
            cursor[0]=0.;cursor[1]+=cursor[2]+10.;cursor[2]=0.
        offset=np.array([cursor[0]-low[0],cursor[1]-low[1]])
        chart=f'{old_chart}/repair-v25/{component:03d}-{color:03d}'
        for i in subgroup:
            triangles[i]['paintUv']=(np.asarray(triangles[i]['paintUv'])+offset).astype(np.float32).tolist()
            triangles[i]['chartId']=chart
        cursor[0]+=float(width)+10.;cursor[2]=max(cursor[2],float(height))
        report.append({'partId':part,'oldChart':old_chart,'chartId':chart,'triangles':len(subgroup),
                       'translationMm':offset.tolist(),'boundsMm':[low.tolist(),high.tolist()]})
    return report


def source_chart_fits(triangles,sources):
    groups=defaultdict(list)
    for t in triangles:groups[t['chartId']].append(t)
    fits={};report=[]
    for chart,members in groups.items():
        x=np.concatenate([np.asarray(t['sourceBarycentric'])@sources[t['id']['sourceMesh']]['uv'][sources[t['id']['sourceMesh']]['indices'][t['id']['sourceTriangle']]] for t in members])
        y=np.concatenate([t['paintUv'] for t in members]);x=np.c_[x,np.ones(len(x))]
        affine=np.linalg.lstsq(x,y,rcond=None)[0];error=float(np.linalg.norm(x@affine-y,axis=1).max())
        condition=float(np.linalg.cond(x))
        method='global-source-UV0-affine';mode='uv0';source_face=None
        if condition>1.e8:
            # The original packer deliberately enlarged thirteen isolated,
            # submicrometre-height source fragments to 0.1 mm. They cannot use
            # a physical world or UV0 affine without enormous normal scale.
            # Their exact barycentric chart is valid only on its owning source
            # face, never as a transform for another fragment/sheet.
            faces={(t['id']['sourceMesh'],t['id']['sourceTriangle']) for t in members}
            if '/source-uv0-precision/' not in chart or len(members)!=1 or len(faces)!=1:
                raise ValueError(f'Unstable non-precision source chart: {chart}')
            bx=np.asarray(members[0]['sourceBarycentric'],dtype=float)
            affine=np.linalg.solve(bx,y);error=float(np.linalg.norm(bx@affine-y,axis=1).max())
            mode='owned-source-barycentric';source_face=next(iter(faces))
            method='original-owned-barycentric-precision-chart-with-author-0.1mm-height'
        if error>.01:
            raise ValueError(f'Chart is not a stable global source UV0 transform: {chart}/{error}/{condition}')
        fits[chart]={'affine':affine,'mode':mode,'sourceFace':source_face}
        report.append({'chartId':chart,'fitMaxErrorMm':error,'conditionNumber':condition,'triangles':len(members),'method':method})
    return fits,report


def chart_uv(fit,bary,source,face):
    if fit['mode']=='owned-source-barycentric':
        if fit['sourceFace']!=(source['id'],face):
            raise ValueError('Precision chart used outside its owning source face')
        return bary@fit['affine']
    uv0=source['uv'][source['indices'][face]]
    return np.c_[bary@uv0,np.ones(len(bary))]@fit['affine']


def reviewed_source_bridges(face_key, members, source, reviewed=None):
    """Fill two explicitly reviewed submicrometre cracks within one source face.

    The original paint union is retained. Both supporting triangles must carry
    the reviewed part, lie on opposite sides, and reference this same GLB face.
    No world-proximity connection between different source faces is created.
    """
    if reviewed is None:
        reviewed={}
    part=reviewed.get(face_key)
    if part is None:
        return {},[]
    if not source['paint'] or any(t['partId']!=part for t in members):
        raise ValueError(f'Reviewed source bridge has an unexpected source material/part: {face_key}')
    world=source['world'][face_key[1]]
    normal=np.cross(world[1]-world[0],world[2]-world[0]);normal/=np.linalg.norm(normal)
    simplex=Polygon([(0.,0.),(1.,0.),(0.,1.)])
    old_union=unary_union([AUDIT['source_polygon'](t).intersection(simplex) for t in members])
    jobs=defaultdict(list);records=[]
    for master in members:
        bary=np.asarray(master['sourceBarycentric'],float);bary/=bary.sum(axis=1)[:,None]
        points=bary@world
        for edge in range(3):
            direction=points[(edge+1)%3]-points[edge];length=float(np.linalg.norm(direction))
            if length==0.:
                continue
            direction/=length
            side=float(np.cross(direction,points.mean(axis=0)-points[edge])@normal)
            for support in members:
                if support is master:
                    continue
                other=np.asarray(support['sourceBarycentric'],float);other/=other.sum(axis=1)[:,None]
                other_world=other@world
                if side*float(np.cross(direction,other_world.mean(axis=0)-points[edge])@normal)>=0.:
                    continue
                for other_edge in range(3):
                    indices=[other_edge,(other_edge+1)%3]
                    endpoints=other_world[indices]
                    parameters=(endpoints-points[edge])@direction/length
                    projected=points[edge]+parameters[:,None]*length*direction
                    distances=np.linalg.norm(endpoints-projected,axis=1)
                    span=abs(float(parameters[1]-parameters[0]))
                    if parameters.min() < -1.e-10 or parameters.max()>1.+1.e-10 or distances.max()>2.e-7 or span>=.9 or span==0.:
                        continue
                    p0=bary[edge]+parameters[0]*(bary[(edge+1)%3]-bary[edge])
                    p1=bary[edge]+parameters[1]*(bary[(edge+1)%3]-bary[edge])
                    polygon=shapely.make_valid(Polygon([other[other_edge,1:],other[(other_edge+1)%3,1:],p1[1:],p0[1:]])).intersection(simplex)
                    addition=polygon.difference(old_union)
                    if addition.area==0.:
                        continue
                    jobs[master['chartId']].append(addition)
                    records.append({'sourceMesh':face_key[0],'face':face_key[1],'partId':part,
                                    'masterTriangle':master['id'],'masterEdge':edge,
                                    'supportTriangle':support['id'],'supportEdge':other_edge,
                                    'maximumWidthMm':float(distances.max()*1000.),
                                    'addedSourceGeometry':shapely.to_geojson(addition),
                                    'addedSourceAreaM2':float(addition.area*2.*AUDIT['area3'](world)),
                                    'reason':'explicitly-reviewed-same-source-face-paint-supported-numerical-crack'})
    return {chart:unary_union(polygons) for chart,polygons in jobs.items()},records


def consolidate(triangles, connections, sources,chart_fits,reviewed_bridges):
    """Remove overlapping/sliver fragmentation in each true source face.

Explicit seam source faces retain their reviewed anchors. Other groups retain
their own affine paint chart; no nearest-neighbour identity is introduced.
"""
    protected_ids = {key(c[s]['triangle']) for c in connections for s in ('from','to')}
    protected = {(c[s]['triangle']['sourceMesh'],c[s]['triangle']['sourceTriangle'])
                 for c in connections for s in ('from','to')}
    groups = defaultdict(list);exact_source_lines=[];bridge_records=[]
    for triangle in triangles:
        bary=np.asarray(triangle['sourceBarycentric'],dtype=float)
        if source_determinant(bary)==0. and key(triangle['id']) not in protected_ids:
            exact_source_lines.append({'triangle':triangle['id'],'sourceAreaM2':0.,
                                       'originalSourceBarycentric':bary.tolist(),
                                       'rowNormalizedSourceBarycentric':(bary/bary.sum(axis=1)[:,None]).tolist()})
            continue
        groups[(triangle['id']['sourceMesh'],triangle['id']['sourceTriangle'])].append(triangle)
    fragment = max(t['id']['fragment'] for t in triangles)+1
    result, changes, failures, precision_losses = [], [], [], []
    simplex = Polygon([(0.,0.),(1.,0.),(0.,1.)])
    for face_key, members in groups.items():
        if len(members)==1:
            result.extend(members);continue
        anchors=[t for t in members if key(t['id']) in protected_ids]
        bridges,reviewed_records=reviewed_source_bridges(face_key,members,sources[face_key[0]],reviewed_bridges)
        bridge_records.extend(reviewed_records)
        charts = defaultdict(list)
        for triangle in members:
            if key(triangle['id']) in protected_ids:
                continue
            charts[(triangle['partId'],triangle['chartId'])].append(triangle)
        prepared=[]
        for chart_key, chart_members in charts.items():
            affine=chart_fits[chart_key[1]]
            polygon=unary_union([AUDIT['source_polygon'](t).intersection(simplex) for t in chart_members]+[bridges.get(chart_key[1],Polygon())])
            prepared.append((polygon.area,chart_members,polygon,affine))
        else:
            assigned=unary_union([AUDIT['source_polygon'](t).intersection(simplex) for t in anchors])
            made=list(anchors);trimmed=0.
            source_world=sources[face_key[0]]['world'][face_key[1]]
            for _,chart_members,polygon,affine in sorted(prepared,key=lambda item:item[0],reverse=True):
                before=polygon.area;polygon=polygon.difference(assigned);trimmed+=before-polygon.area
                assigned=assigned.union(polygon)
                for piece in source_triangles(polygon):
                    if piece.area == 0.:
                        continue
                    xy=np.asarray(piece.exterior.coords[:3]);bary=np.c_[1-xy.sum(axis=1),xy]
                    world=(bary@source_world).astype(np.float32)
                    uv=chart_uv(affine,bary,sources[face_key[0]],face_key[1]).astype(np.float32)
                    world64=world.astype(float)
                    world_cross=np.cross(world64[1]-world64[0],world64[2]-world64[0])
                    if not np.any(world_cross != 0.):
                        precision_losses.append({'sourceMesh':face_key[0],'face':face_key[1],
                                                 'chartId':chart_members[0]['chartId'],
                                                 'sourceBarycentric':bary.tolist(),
                                                 'sourceAreaM2':piece.area*2*float(AUDIT['area3'](source_world)),
                                                 'worldF32':world.tolist(),
                                                 'reason':'positive-source-polygon-exactly-zero-world-cross-after-f32-endpoints-promoted-to-f64'})
                        continue
                    created=dict(chart_members[0]);created['id']=dict(created['id'],fragment=fragment);fragment+=1
                    created['world']=world.tolist();created['sourceBarycentric']=bary.astype(np.float32).tolist();created['paintUv']=uv.tolist()
                    made.append(created)
            result.extend(made)
            changes.append({'sourceMesh':face_key[0],'face':face_key[1],'oldTriangles':len(members),'newTriangles':len(made),
                            'trimmedRepeatedAreaM2':trimmed*2*float(AUDIT['area3'](source_world)),
                            'protectedAnchorTriangles':len(anchors)})
            continue
        result.extend(members)
    return result,{'changedFaces':changes,'nonAffineFacesPreserved':failures,'protectedSeamSourceFaces':len(protected),
                   'positivePrecisionLosses':precision_losses,'positivePrecisionLossAreaM2':sum(p['sourceAreaM2'] for p in precision_losses),
                   'removedExactSourceLines':exact_source_lines,'removedExactSourceAreaM2':0.,
                   'reviewedSourceBridges':bridge_records,
                   'exactOrientationTriangulationFallbacks':TRIANGULATION_FALLBACKS,
                   'sourceDomainNormalization':'Full homogeneous barycentric rows divided by row sums; near-linear source-zero classification proved by exact dyadic rational 3x3 determinant.'}


def cluster_parameters(values, edge_length):
    # Distinct source parameters may bound a real positive-area sliver.
    return sorted(set(values))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--model', type=Path, required=True)
    parser.add_argument('--map', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--repack-only',action='store_true')
    parser.add_argument('--reviewed-source-bridges',type=Path,
                        help='Optional independently reviewed source-domain addition manifest; absent means no bridges')
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=False)
    began = time.time()
    document = json.loads(args.map.read_text('utf-8'))
    if AUDIT['digest'](args.model) != document['modelFingerprint']:
        raise ValueError('Source model fingerprint mismatch')
    reviewed_bridges={}
    if args.reviewed_source_bridges:
        bridge_configuration=json.loads(args.reviewed_source_bridges.read_text('utf-8'))
        if bridge_configuration['modelFingerprint']!=document['modelFingerprint'] or bridge_configuration['maximumWidthMm']!=.0002:
            raise ValueError('Reviewed source bridge model/width mismatch')
        reviewed_bridges={(r['sourceMesh'],r['sourceFace']):r['partId'] for r in bridge_configuration['sourceFaces']}
    original_input_count = len(document['triangles'])
    triangles = copy.deepcopy(document['triangles'])
    sources=None;chart_fits=None;chart_fit_report=None
    if not args.repack_only:
        glb=AUDIT['Glb'](args.model);sources={s['id']:s for s in glb.primitives()}
        for source in sources.values():
            primitive=glb.document['meshes'][glb.document['nodes'][source['node']]['mesh']]['primitives'][source['primitive']]
            source['uv']=glb.accessor(primitive['attributes']['TEXCOORD_0']) if 'TEXCOORD_0' in primitive['attributes'] else None
        chart_fits,chart_fit_report=source_chart_fits(triangles,sources)
    packing = pack_repairs(triangles)
    if chart_fits is not None:
        for p in packing:
            fit=copy.deepcopy(chart_fits[p['oldChart']]);fit['affine'][-1]+=np.asarray(p['translationMm']);chart_fits[p['chartId']]=fit
    if args.repack_only:
        micro_charts=[];source_cache={s['id']:s for s in AUDIT['Glb'](args.model).primitives()};micro_cursor=defaultdict(float)
        source_degenerate=[];retained=[]
        connection_ids={key(c[s]['triangle']) for c in document['connections'] for s in ('from','to')}
        for triangle in triangles:
            source=source_cache[triangle['id']['sourceMesh']]
            source_world=source['world'][triangle['id']['sourceTriangle']]
            bary=np.asarray(triangle['sourceBarycentric'],dtype=float)
            source_area=abs(source_determinant(bary))*float(AUDIT['area3'](source_world))
            if source_area==0. and key(triangle['id']) not in connection_ids:
                source_degenerate.append({'triangle':triangle['id'],'sourceAreaM2':source_area,
                                          'originalSourceBarycentric':bary.tolist(),
                                          'rowNormalizedSourceBarycentric':(bary/bary.sum(axis=1)[:,None]).tolist(),
                                          'reason':'exact-zero-rational-homogeneous-source-barycentric-area-despite-f32-world-noise'})
                continue
            retained.append(triangle)
            uv=np.asarray(triangle['paintUv'],dtype=np.float32);q=uv[1:]-uv[0]
            cross=q[0,0]*q[1,1]-q[0,1]*q[1,0]
            uv64=uv.astype(float);uv_edges=uv64[1:]-uv64[0]
            determinant=float(np.linalg.det(uv_edges))
            # Match the actual Vec2 audit/runtime evaluation order as well as
            # the correctly rounded mean; they differ by an ULP on thin UVs.
            centroids=[uv64.mean(axis=0).astype(np.float32).astype(float),
                       ((uv[0]+uv[1]+uv[2])/np.float32(3.)).astype(float)]
            centroid_bary=[]
            for centroid in centroids:
                weights=np.linalg.solve(uv_edges.T,centroid-uv64[0]) if abs(determinant)>=1.e-12 else np.array([-1.,-1.])
                centroid_bary.append(np.r_[1.-weights.sum(),weights])
            if abs(determinant)<1.e-12 or min(b.min() for b in centroid_bary)<-1.e-5:
                if key(triangle['id']) in connection_ids:
                    raise ValueError('UV repacking collapsed an explicit seam anchor')
                normalized=bary/bary.sum(axis=1)[:,None]
                world=normalized@source_world
                area=source_area
                pairs=[(float(np.linalg.norm(world[b]-world[a])),a,b) for a,b in ((0,1),(1,2),(2,0))]
                length,a,b=max(pairs)
                tangent=(world[b]-world[a])/length
                normal=np.cross(source_world[1]-source_world[0],source_world[2]-source_world[0]);normal/=np.linalg.norm(normal)
                up=np.cross(normal,tangent)
                local=np.c_[(world-world[a])@tangent,(world-world[a])@up]*1000.
                c=3-a-b
                physical_height=2.*area/length*1000.
                # Packed UV is a locator. Its minimum representable height is
                # explicit; physical image dimensions still use world metric.
                locator_height=max(physical_height,1.e-5,1.e-10/(length*1000.))
                physical_length=length*1000.
                # Tiny positive source triangles can be shorter than one ULP
                # at the cumulative packing offset. Keep a representable UV
                # locator; neither world nor source-domain geometry changes.
                packing_ulp=float(np.spacing(np.float32(micro_cursor[triangle['partId']]+physical_length+1.)))
                locator_length=max(physical_length,1.e-5,16.*packing_ulp)
                local[:,0]*=locator_length/physical_length
                local[a]=[0.,0.];local[b]=[locator_length,0.]
                local[c,1]=np.copysign(locator_height,source_determinant(bary)*float(np.linalg.det(np.eye(3)[[a,b,c]])))
                low,high=local.min(axis=0),local.max(axis=0)
                offset=np.array([-micro_cursor[triangle['partId']]-high[0]-.01,-high[1]-.01])
                triangle['paintUv']=(local+offset).astype(np.float32).tolist()
                triangle['chartId']=f'{triangle["partId"]}/source-tangent-micro-v25/{len(micro_charts):03d}'
                micro_cursor[triangle['partId']]+=float(high[0]-low[0])+1.
                uv=np.asarray(triangle['paintUv'],dtype=np.float32);q=uv[1:]-uv[0]
                if abs(float(q[0,0]*q[1,1]-q[0,1]*q[1,0]))<1.e-12:
                    raise ValueError(f'Actual source tangent micro-chart is degenerate: {triangle["id"]}')
                micro_charts.append({'triangle':triangle['id'],'sourceAreaM2':area,'chartId':triangle['chartId'],
                                     'sourceMetricMethod':'source world orthonormal tangent with explicit minimum f32 locator height; world metric remains authoritative',
                                     'physicalSourceLengthMm':physical_length,'packedLocatorLengthMm':locator_length,
                                     'packingOffsetUlpMm':packing_ulp,
                                     'physicalSourceHeightMm':physical_height,'packedLocatorHeightMm':locator_height,
                                     'tangent':tangent.tolist(),'up':up.tolist(),'translationMm':offset.tolist(),
                                     'reason':'regular packing lost submicrometre source height or a representable centroid at runtime f32'})
        triangles=retained
        candidate=dict(document,paintFingerprint='',triangles=triangles)
        AUDIT['write_json'](args.output/'paint-map-unsealed.json',candidate)
        report={'status':'uv-repacked-awaiting-seal-and-identity-audit','inputMap':str(args.map.resolve()),
                'inputMapSha256':AUDIT['digest'](args.map),'modelFingerprint':document['modelFingerprint'],
                'triangles':len(triangles),'packedRepairCharts':packing,'sourceTangentMicroCharts':micro_charts,
                'microSourceAreaM2':sum(t['sourceAreaM2'] for t in micro_charts),
                'removedZeroAreaSourceFragments':source_degenerate,
                'removedZeroAreaSourceM2':sum(t['sourceAreaM2'] for t in source_degenerate),
                'elapsedSeconds':time.time()-began}
        original_by_id={key(t['id']):t for t in document['triangles']}
        report['retainedGeometryAndIdentitiesUnchanged']=all({k:v for k,v in t.items() if k not in ('chartId','paintUv')}=={k:v for k,v in original_by_id[key(t['id'])].items() if k not in ('chartId','paintUv')} for t in triangles)
        AUDIT['write_json'](args.output/'uv-packing-report.json',report)
        print(json.dumps({k:v for k,v in report.items() if k not in ('packedRepairCharts','sourceTangentMicroCharts','removedZeroAreaSourceFragments')}),flush=True)
        return
    triangles, consolidation = consolidate(triangles,document['connections'],sources,chart_fits,reviewed_bridges)
    consolidation['reviewedSourceBridgeConfiguration']=({'path':str(args.reviewed_source_bridges.resolve()),
                                                        'sha256':AUDIT['digest'](args.reviewed_source_bridges),
                                                        'sourceFaces':len(reviewed_bridges)}
                                                       if args.reviewed_source_bridges else None)
    original_triangles = copy.deepcopy(triangles)
    protected_edges = {(key(c[s]['triangle']),c[s]['edge']) for c in document['connections'] for s in ('from','to')}
    by_face, edge_values, face_edges = defaultdict(list), defaultdict(list), {}
    # Registry keys are actual source accessor vertex indices, never world proximity.
    for index, triangle in enumerate(triangles):
        by_face[(triangle['id']['sourceMesh'], triangle['id']['sourceTriangle'])].append(index)
    for face_key, indices in by_face.items():
        source = sources[face_key[0]]
        vertex_ids = source['indices'][face_key[1]]
        source_world = source['world'][face_key[1]]
        edges = []
        for opposite in range(3):
            a, b = [i for i in range(3) if i != opposite]
            if vertex_ids[a] > vertex_ids[b]:
                a, b = b, a
            edge_key = (face_key[0], int(vertex_ids[a]), int(vertex_ids[b]))
            edges.append((opposite, a, b, edge_key))
            edge_values[edge_key].extend([0., 1.])
        face_edges[face_key] = edges
        for index in indices:
            bary = np.asarray(triangles[index]['sourceBarycentric'], dtype=float)
            bary /= bary.sum(axis=1)[:, None]
            for row in bary:
                world = row @ source_world
                for opposite, a, b, edge_key in edges:
                    direction = source_world[b]-source_world[a]
                    squared = float(direction@direction)
                    if squared == 0.:
                        continue
                    t = float((world-source_world[a])@direction/squared)
                    distance = np.linalg.norm(source_world[a]+t*direction-world)
                    if row[opposite]==0.:
                        t = float(row[b]/(row[a]+row[b]))
                        row[:] = 0.
                        row[a], row[b] = 1-t, t
                        edge_values[edge_key].append(t)
                        break
            triangles[index]['sourceBarycentric'] = bary.tolist()
    for edge_key, values in edge_values.items():
        source = sources[edge_key[0]]
        length = float(np.linalg.norm(source['positions'][edge_key[2]]-source['positions'][edge_key[1]]))
        edge_values[edge_key] = cluster_parameters(values, length)
    face_points = {}
    for face_key, indices in by_face.items():
        points = []
        for opposite, a, b, edge_key in face_edges[face_key]:
            for t in edge_values[edge_key]:
                row = np.zeros(3)
                row[a], row[b] = 1-t, t
                points.append(row)
        for index in indices:
            points.extend(np.asarray(triangles[index]['sourceBarycentric']))
        # Cluster only within an identified original face. Canonical source-edge
        # points are inserted first so both source faces choose identical cuts.
        source_world = sources[face_key[0]]['world'][face_key[1]]
        unique = []
        for row in points:
            if not unique or not any(np.array_equal(existing,row) for existing in unique):
                unique.append(row)
        face_points[face_key] = np.asarray(unique)
        for index in indices:
            bary = np.asarray(triangles[index]['sourceBarycentric'])
            for i, row in enumerate(bary):
                distances = np.linalg.norm((face_points[face_key]-row)@source_world, axis=1)
                bary_distances=np.max(np.abs(face_points[face_key]-row),axis=1)
                nearest=int(bary_distances.argmin())
                if bary_distances[nearest]==0.:
                    bary[i] = face_points[face_key][nearest]
            triangles[index]['sourceBarycentric'] = bary.tolist()

    def cuts(index, edge):
        triangle = triangles[index]
        face_key = (triangle['id']['sourceMesh'], triangle['id']['sourceTriangle'])
        world = sources[face_key[0]]['world'][face_key[1]]
        bary = np.asarray(triangle['sourceBarycentric'])
        a, b = bary[edge], bary[(edge+1)%3]
        if (key(triangle['id']),edge) in protected_edges:
            return [(0.,a),(1.,b)]
        direction = (b-a)[1:]
        squared = float(direction@direction)
        if squared == 0.:
            return [(0., a), (1., b)]
        candidates = face_points[face_key]
        ts = (candidates-a)[:,1:]@direction/squared
        errors = np.linalg.norm((candidates-a)[:,1:]-ts[:, None]*direction, axis=1)
        length = np.sqrt(squared)
        selected = [(float(t), candidate.copy()) for t,candidate in zip(ts,candidates)
                    if 0. < t < 1. and source_determinant(np.array([a,b,candidate]))==0.]
        ordered = [(0., a)] + sorted(selected, key=lambda item:item[0]) + [(1., b)]
        unique = [ordered[0]]
        for item in ordered[1:-1]:
            if not np.array_equal(item[1],unique[-1][1]):
                unique.append(item)
        unique.append(ordered[-1])
        return unique

    print(f'conforming {len(triangles)} triangles; protected {len(protected_edges)} original explicit seam edges', flush=True)
    result, descendants, split_counts = [], {}, Counter()
    fragment = max(t['id']['fragment'] for t in triangles)+1
    max_shift = 0.
    collapsed = []
    for index, triangle in enumerate(triangles):
        source = sources[triangle['id']['sourceMesh']]
        source_world = source['world'][triangle['id']['sourceTriangle']]
        bary = np.asarray(triangle['sourceBarycentric'])
        source_uv0=source['uv'][source['indices'][triangle['id']['sourceTriangle']]]
        # Use the well-conditioned whole-chart source UV0 fit. Inverting the
        # barycentric matrix of a microscopic fragment can amplify its normal
        # direction and generate a spurious UV overlap despite tiny residuals.
        uv_transform=chart_fits[triangle['chartId']]
        boundary, edge_ranges = [], []
        for edge in range(3):
            edge_cuts = cuts(index, edge)
            start = len(boundary)
            boundary.extend(row for _, row in edge_cuts[:-1])
            edge_ranges.append((edge_cuts, start))
        boundary = np.asarray(boundary)
        canonical = (bary@source_world).astype(np.float32)
        max_shift = max(max_shift, float(np.linalg.norm(canonical-np.asarray(original_triangles[index]['world']), axis=1).max()))
        pieces = [boundary] if len(boundary) == 3 else [np.array([boundary.mean(axis=0), boundary[i], boundary[(i+1)%len(boundary)]]) for i in range(len(boundary))]
        boundary_ranges = [(edge,edge_cuts[i][0],edge_cuts[i+1][0])
                           for edge,(edge_cuts,_) in enumerate(edge_ranges) for i in range(len(edge_cuts)-1)]
        edge_descendants = [[] for _ in range(3)]
        first_result_index = len(result)
        for piece_index,piece in enumerate(pieces):
            world = (piece@source_world).astype(np.float32)
            uv = chart_uv(uv_transform,piece,source,triangle['id']['sourceTriangle']).astype(np.float32)
            world64=world.astype(float)
            cross = np.cross(world64[1]-world64[0],world64[2]-world64[0])
            uv_edges = uv[1:]-uv[0]
            uv_cross = uv_edges[0,0]*uv_edges[1,1]-uv_edges[0,1]*uv_edges[1,0]
            if not np.any(cross != 0.):
                source_area = float(AUDIT['area3'](piece@source_world))
                collapsed.append({'oldTriangle': triangle['id'], 'sourceAreaM2': source_area,
                                  'sourceBarycentricOriginal':piece.tolist(),
                                  'sourceBarycentricStored':piece.astype(np.float32).tolist(),
                                  'worldF32':world.tolist(),
                                  'reason':'positive-source-piece-exactly-zero-world-cross-after-f32-endpoints-promoted-to-f64'})
                continue
            created = dict(triangle)
            created['id'] = dict(triangle['id'], fragment=fragment)
            fragment += 1
            created['world'] = world.tolist()
            created['sourceBarycentric'] = piece.astype(np.float32).tolist()
            created['paintUv'] = uv.tolist()
            result.append(created)
            # The fan construction knows its exact parent edge; no geometric
            # proximity search may accidentally call a short radial edge a seam.
            if len(boundary) == 3:
                for edge in range(3):
                    edge_descendants[edge].append({'triangle':created['id'],'edge':edge,'range':[0.,1.]})
            else:
                edge,t0,t1 = boundary_ranges[piece_index]
                edge_descendants[edge].append({'triangle':created['id'],'edge':1,'range':[t0,t1]})
        descendants[key(triangle['id'])] = edge_descendants
        if len(result)-first_result_index != 1:
            split_counts[triangle['partId']] += 1
        if index % 25000 == 0:
            print(f'triangulated {index}/{len(triangles)} -> {len(result)}', flush=True)
    new_by_id = {key(t['id']):t for t in result}
    connections, seam_issues = [], []
    for connection in document['connections']:
        a = descendants[key(connection['from']['triangle'])][connection['from']['edge']]
        b = descendants[key(connection['to']['triangle'])][connection['to']['edge']]
        paired = set()
        for left in a:
            left_range = np.asarray(left['range'])
            candidates = []
            for ordinal, right in enumerate(b):
                right_range = np.asarray(right['range'])
                if connection['reversed']:
                    right_range = 1-right_range
                error = float(np.max(np.abs(np.sort(left_range)-np.sort(right_range))))
                candidates.append((error, ordinal, right, right_range))
            if not candidates or min(c[0] for c in candidates) > .001:
                seam_issues.append({'id': connection['id'], 'reason': 'no-conforming-parameter-partner', 'range': left['range']})
                continue
            _, ordinal, right, right_range = min(candidates, key=lambda c:c[0])
            if ordinal in paired:
                seam_issues.append({'id': connection['id'], 'reason': 'duplicate-partner-range'})
                continue
            paired.add(ordinal)
            reversed_edge = (left_range[1]>left_range[0]) != (right_range[1]>right_range[0])
            new = dict(connection, id=f'{connection["id"]}/conform/{len(paired):03d}')
            new['from'] = {k:left[k] for k in ('triangle', 'edge')}
            new['to'] = {k:right[k] for k in ('triangle', 'edge')}
            new['reversed'] = bool(reversed_edge)
            points_a = np.asarray(new_by_id[key(left['triangle'])]['world'])[[left['edge'], (left['edge']+1)%3]]
            points_b = np.asarray(new_by_id[key(right['triangle'])]['world'])[[right['edge'], (right['edge']+1)%3]]
            new['physicalGapMm'] = float(np.linalg.norm(points_a-(points_b[::-1] if reversed_edge else points_b), axis=1).max()*1000)
            connections.append(new)
        if len(paired) != len(b):
            seam_issues.append({'id': connection['id'], 'reason': 'unpaired-to-segments', 'count': len(b)-len(paired)})
    candidate = dict(document, paintFingerprint='', triangles=result, connections=connections)
    AUDIT['write_json'](args.output/'paint-map-unsealed.json', candidate)
    report = {'status':'candidate-requires-union-topology-and-coverage-validation',
              'inputMap':str(args.map.resolve()), 'inputMapSha256': AUDIT['digest'](args.map),
              'modelFingerprint':document['modelFingerprint'], 'oldTriangles':original_input_count, 'consolidatedTriangles':len(triangles),'newTriangles':len(result),
              'splitOldTrianglesByPart':dict(split_counts), 'maxCornerShiftMm':max_shift*1000,
              'sourceEdgePolicy':'Exact opposite-barycentric zero; distinct source parameters are never snapped or averaged.',
              'withinFacePolicy':'Exact duplicate homogeneous source rows only; collinearity is proved by an exact dyadic rational determinant.',
              'collapsedPieces':collapsed, 'collapsedSourceAreaM2':sum(p['sourceAreaM2'] for p in collapsed),
              'oldConnections':len(document['connections']), 'newConnections':len(connections),
              'seamIssues':seam_issues, 'packedRepairCharts':packing, 'consolidation':consolidation,
              'globalSourceUVChartFits':chart_fit_report,
              'protectedExplicitSeamEdges':len(protected_edges),'elapsedSeconds':time.time()-began,
              'limits':['Cross-face subdivision uses original source index edges only.',
                        'Fresh fragment identities require compatibility review.',
                        'Sampled visibility and map sealing alone do not establish exterior completeness.']}
    AUDIT['write_json'](args.output/'conform-report.json', report)
    print(json.dumps({k:v for k,v in report.items() if k not in ('packedRepairCharts','collapsedPieces','seamIssues','consolidation','globalSourceUVChartFits')}), flush=True)
    if seam_issues:
        raise ValueError(f'{len(seam_issues)} explicit seam issues need review')


if __name__ == '__main__':
    main()
