"""Author SU7 exterior paint seams without changing classified geometry or UVs.

The reviewed semantic pairs and spatial gates below are SU7-specific asset data,
not generic propagation rules. Runtime transport remains model-independent.
This tool writes a fresh unsealed candidate directory; seal with surface-map's
Rust validator before integrating it into the vehicle package.
"""
import argparse
from collections import defaultdict, deque
import hashlib
import json
from pathlib import Path
import numpy as np

ROOT = Path(__file__).resolve().parents[2]
SOURCE = ROOT / 'assets/paint/su7-v21-classified-only/paint-map.json'


def identity(value):
    return tuple(value[k] for k in ('sourceMesh', 'primitive', 'sourceTriangle', 'fragment'))


def endpoint_key(endpoint):
    return identity(endpoint['triangle']), int(endpoint['edge'])


def connection_key(connection):
    return tuple(sorted((endpoint_key(connection['from']), endpoint_key(connection['to']))))


def boundary_edges(triangles):
    entries = defaultdict(list)
    for triangle in triangles:
        for edge in range(3):
            points = [tuple(round(v * 1e6) for v in triangle['world'][i])
                      for i in (edge, (edge + 1) % 3)]
            entries[tuple(sorted(points))].append((triangle, edge))
    return [rows[0] for rows in entries.values() if len(rows) == 1]


def geometry(rows):
    world = np.asarray([t['world'] for t, _ in rows], dtype=np.float64)
    indexes = np.arange(len(rows))
    edges = np.asarray([e for _, e in rows])
    a, b, c = (world[indexes, (edges + i) % 3] for i in range(3))
    direction = b - a
    length = np.linalg.norm(direction, axis=1)
    unit = direction / np.maximum(length[:, None], 1e-15)
    inner = c - a - unit * np.sum((c - a) * unit, axis=1)[:, None]
    inner /= np.maximum(np.linalg.norm(inner, axis=1)[:, None], 1e-15)
    return a, b, (a + b) / 2, unit, length, inner


def reviewed_pairs():
    # Coordinates are source SU7 world metres: longitudinal X, up Y, left +Z.
    # Bounds suppress unrelated classified slivers (e.g. rear-bumper at X=-.9).
    pairs = []
    def add(a, b, gap_mm=25., x=(-3., 3.), y=(0., 2.), z=(-1.2, 1.2), note='adjacent outer panel gap',
            tangent_dot=.9, tangent_gap_mm=1.):
        pairs.append(dict(parts=['su7.' + a, 'su7.' + b], maxGapMm=gap_mm,
                          bounds=[list(x), list(y), list(z)], note=note,
                          minTangentAbsDot=tangent_dot,maxTangentGapMm=tangent_gap_mm))
    for side, z in [('left', (.50, 1.2)), ('right', (-1.2, -.50))]:
        add(f'front-{side}-door', f'rear-{side}-door', 25, x=(.10,.35), z=z)
        add(f'front-{side}-door', f'front-{side}-fender', 25, x=(-1.05,-.75), y=(.65,1.10), z=z)
        add(f'front-{side}-door', f'front-{side}-lower-fender', 25, x=(-1.08,-.80), y=(.24,.78), z=z,
            tangent_dot=.8 if side=='right' else .9,
            note='adjacent lower front edge; right 1.7mm curved clipped edges have .839 tangent dot and 1.58mm endpoint gap')
        add(f'rear-{side}-door', f'rear-{side}-fender', 25, x=(.78,1.35), z=z)
        add(f'front-{side}-door', f'rear-{side}-fender', 25, x=(-1.05,-.45), y=(.85,1.12), z=z,
            tangent_gap_mm=11. if side=='left' else 1.,
            note='rear-fender classification includes A-pillar/roof rail; left clipped border preserves up to10.1mm local tangent offset')
        add(f'front-{side}-fender', f'rear-{side}-fender', 25, x=(-1.15,-.70), y=(.85,1.12), z=z,
            note='local A-pillar base meets upper front fender; prevent long-route detour through rear door')
        add(f'front-{side}-fender', f'front-{side}-lower-fender', 25, x=(-1.45,-.90), y=(.65,.80), z=z,
            tangent_gap_mm=16. if side=='left' else 1.,
            note='left classified border segmentation uses nearby 7.385/6.905mm edges with preserved 15.25mm tangent offset; no length-rule relaxation')
        add('front-bumper', f'front-{side}-fender', 25, x=(-2.30,-1.70), y=(.60,.85), z=z)
        add('hood', f'front-{side}-fender', 25, x=(-2.4,-.95), y=(.65,1.08), z=z)
        add('rear-bumper', f'rear-{side}-fender', 25, x=(1.55,2.40), y=(.55,1.05), z=z)
    add('front-bumper', 'hood', 25, x=(-2.5,-2.0), y=(.65,.86))
    add('charge-cover', 'rear-left-fender', 25, x=(1.65,2.05), y=(.80,1.05), z=(.70,1.0))
    # These C rings are separate painted inserts, with a real intervening trim
    # opening. Bridge the measured gap in metric space; never paint the opening.
    add('front-bumper', 'front-c-ring', 65, x=(-2.55,-2.28), y=(.25,.53),
        note='explicit painted insert bridge across real front trim gap; gap remains unpainted')
    add('rear-bumper', 'rear-c-ring', 100, x=(2.20,2.55), y=(.25,.66),
        note='explicit painted insert bridge across real rear trim gap; gap remains unpainted')
    return pairs


def candidates(pair, boundaries, data):
    pa, pb = pair['parts']
    aa, ab, am, au, al, ai = data[pa]
    ba, bb, bm, bu, bl, bi = data[pb]
    bounds = np.asarray(pair['bounds'])
    gate = lambda p: np.all((p >= bounds[:, 0]) & (p <= bounds[:, 1]), axis=1)
    left = np.flatnonzero(gate(am) & (al >= .0005))
    right = np.flatnonzero(gate(bm) & (bl >= .0005))
    limit = pair['maxGapMm'] / 1000.
    result = []
    for start in range(0, len(left), 128):
        selected = left[start:start+128]
        diffs = am[selected, None, :] - bm[None, right, :]
        distances = np.einsum('ijk,ijk->ij', diffs, diffs)
        ax, bx = np.where(distances <= limit**2)
        ax, bx = selected[ax], right[bx]
        if not len(ax):
            continue
        dot = np.sum(au[ax] * bu[bx], axis=1)
        reverse = dot < 0
        b0 = np.where(reverse[:, None], bb[bx], ba[bx])
        b1 = np.where(reverse[:, None], ba[bx], bb[bx])
        d0 = np.linalg.norm(aa[ax] - b0, axis=1)
        d1 = np.linalg.norm(ab[ax] - b1, axis=1)
        maximum = np.maximum(d0,d1)
        tangent_offset = np.sum((bm[bx] - am[ax]) * au[ax], axis=1)
        overlap = np.minimum(al[ax]*.5,tangent_offset+bl[bx]*.5) - np.maximum(-al[ax]*.5,tangent_offset-bl[bx]*.5)
        # Match lengths as required by the asset validator; never split or
        # perturb classified triangles to manufacture a correspondence.
        accepted = ((np.abs(dot) >= pair['minTangentAbsDot']) & (np.abs(al[ax]-bl[bx]) <= .0049)
                    & (maximum <= limit) & (overlap >= -pair['maxTangentGapMm']/1000.))
        for i, j, rev, gap, tangent in zip(ax[accepted],bx[accepted],reverse[accepted],maximum[accepted],dot[accepted]):
            ta,ea=boundaries[pa][int(i)];tb,eb=boundaries[pb][int(j)]
            connection = dict(from_={'triangle':ta['id'],'edge':ea},to={'triangle':tb['id'],'edge':eb},
                              reversed=bool(rev),physicalGapMm=float(gap*1000),propagation=True)
            connection['from']=connection.pop('from_')
            key=connection_key(connection)
            digest=hashlib.sha256(json.dumps(key,separators=(',',':')).encode()).hexdigest()[:16]
            connection['id']='su7-reviewed-whole-car/'+pa[4:]+'--'+pb[4:]+'/'+digest
            score=float(gap + (1-abs(tangent))*.02)
            result.append((score, int(i), int(j), connection,
                           {'midpoint':((am[i]+bm[j])*.5).tolist(),
                            'tangentAbsDot':abs(float(tangent)),
                            'inwardDot':float(np.dot(ai[i],bi[j])),
                            'fromPaintUv':np.mean(ta['paintUv'],axis=0).tolist(),
                            'toPaintUv':np.mean(tb['paintUv'],axis=0).tolist()}))
    # Keep the best local pairing for every source and destination edge. A
    # symmetric nearest union covers long seams without arbitrary global caps.
    result.sort(key=lambda r: (r[0], r[1], r[2]))
    seen_a, seen_b, selected = set(),set(),[]
    for row in result:
        if row[1] not in seen_a or row[2] not in seen_b:
            selected.append(row)
        seen_a.add(row[1]);seen_b.add(row[2])
    return selected, len(result)


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--input',type=Path,default=SOURCE)
    parser.add_argument('--output',type=Path,default=ROOT/'assets/paint/su7-v22-whole-car')
    args=parser.parse_args()
    if args.output.exists():
        raise SystemExit('Output directory already exists; choose a fresh candidate path.')
    doc=json.loads(args.input.read_text(encoding='utf-8'))
    groups=defaultdict(list)
    for t in doc['triangles']:groups[t['partId']].append(t)
    pairs=reviewed_pairs()
    missing={part for p in pairs for part in p['parts']} - set(groups)
    if missing:raise SystemExit('Input is not the reviewed SU7 classification: '+str(sorted(missing)))
    boundaries={part:boundary_edges(rows) for part,rows in groups.items()}
    data={part:geometry(rows) for part,rows in boundaries.items()}
    original_hash=hashlib.sha256(json.dumps(doc['triangles'],separators=(',',':')).encode()).hexdigest()
    ids={identity(t['id']):t for t in doc['triangles']}
    kept=doc['connections'][:]
    known={connection_key(c) for c in kept}
    report={'source':str(args.input.resolve()),'sourcePaintFingerprint':doc['paintFingerprint'],
            'status':'candidate-needs-rust-seal-and-runtime-validation',
            'triangleCount':len(doc['triangles']),'triangleContentSha256':original_hash,
            'triangleWorldUvAndIdentityUnchanged':True,'initialConnections':len(kept), 'pairs':[]}
    for pair in pairs:
        chosen,total=candidates(pair,boundaries,data)
        added=[]
        for _,_,_,c,detail in chosen:
            key=connection_key(c)
            if key in known:continue
            known.add(key);kept.append(c)
            added.append({'id':c['id'],'physicalGapMm':c['physicalGapMm'],**detail})
        summary={**pair,'qualifiedBoundaryPairs':total,'addedConnections':len(added),
                 'gapRangeMm':[min((c['physicalGapMm'] for c in added),default=None),max((c['physicalGapMm'] for c in added),default=None)],'connections':added}
        report['pairs'].append(summary)
        print(' <-> '.join(pair['parts']),len(added),summary['gapRangeMm'],flush=True)
    graph=defaultdict(set)
    for c in kept:
        if not c['propagation']:continue
        a=ids[identity(c['from']['triangle'])]['partId'];b=ids[identity(c['to']['triangle'])]['partId']
        graph[a].add(b);graph[b].add(a)
    components=[];seen=set()
    for part in sorted(groups):
        if part in seen:continue
        queue=deque([part]);seen.add(part);component=[]
        while queue:
            item=queue.popleft();component.append(item)
            for other in sorted(graph[item]-seen):seen.add(other);queue.append(other)
        components.append(sorted(component))
    report['partGraphComponents']=components
    report['unjoinedParts']=[p for p in sorted(groups) if not graph[p]]
    report['intentionalExclusions']=['su7.rear-spoiler: floating aerodynamic attachment, no automatic body bridge',
        'glass/trim gaps are never painted; links transport physical distance between existing painted triangles only',
        'rear-bumper forward classified sliver is excluded from front-body pair authoring',
        'no global nearest-neighbor fallback and no left-to-right shortcut through the vehicle']
    report['finalConnections']=len(kept)
    doc['connections']=kept;doc['paintFingerprint']=''
    assert original_hash==hashlib.sha256(json.dumps(doc['triangles'],separators=(',',':')).encode()).hexdigest()
    args.output.mkdir(parents=True,exist_ok=False)
    (args.output/'paint-map-unsealed.json').write_text(json.dumps(doc,ensure_ascii=False,separators=(',',':')),encoding='utf-8')
    (args.output/'connection-report.json').write_text(json.dumps(report,ensure_ascii=False,indent=2),encoding='utf-8')
    render_witness(doc,report,args.output/'connections-world-review.png')
    print(json.dumps({'output':str(args.output),'connections':len(kept),'components':components},ensure_ascii=False))


def render_witness(doc,report,path):
    from PIL import Image,ImageDraw,ImageFont
    import colorsys
    image=Image.new('RGB',(1800,1800),(246,247,249));draw=ImageDraw.Draw(image)
    font_path=Path('C:/Windows/Fonts/arial.ttf')
    font=ImageFont.truetype(str(font_path),17) if font_path.exists() else ImageFont.load_default()
    title=ImageFont.truetype(str(font_path),23) if font_path.exists() else font
    parts=sorted({t['partId'] for t in doc['triangles']})
    colors={part:tuple(int(v*255) for v in colorsys.hsv_to_rgb(i/len(parts),.28,.80)) for i,part in enumerate(parts)}
    ids={identity(t['id']):t for t in doc['triangles']}
    pair_index={tuple(sorted(p['parts'])):i for i,p in enumerate(report['pairs'])}
    for view_index,view in enumerate(['left','right','top']):
        top=10+view_index*430
        draw.rectangle((15,top,1785,top+415),outline=(180,185,192),width=1)
        draw.text((35,top+12),view.upper()+' WORLD VIEW / colored marks = reviewed paint links; background gaps remain unpainted',font=title,fill=(25,30,40))
        def project(p):
            return (100+(p[0]+2.6)*300,top+235-p[2]*155) if view=='top' else (100+(p[0]+2.6)*300,top+390-p[1]*240)
        visible=[t for t in doc['triangles'] if view=='top' or (sum(p[2] for p in t['world'])>=0)==(view=='left')]
        visible.sort(key=lambda t:sum(p[1 if view=='top' else 2] for p in t['world'])*(1 if view!='right' else -1))
        for t in visible:draw.polygon([project(p) for p in t['world']],fill=colors[t['partId']])
        for c in doc['connections']:
            a=ids[identity(c['from']['triangle'])];b=ids[identity(c['to']['triangle'])]
            pair=tuple(sorted((a['partId'],b['partId'])))
            i=pair_index.get(pair,0);color=tuple(int(v*255) for v in colorsys.hsv_to_rgb(i/max(1,len(pair_index)),.9,.65))
            am=np.mean([a['world'][c['from']['edge']],a['world'][(c['from']['edge']+1)%3]],axis=0)
            bm=np.mean([b['world'][c['to']['edge']],b['world'][(c['to']['edge']+1)%3]],axis=0)
            if view!='top' and ((am[2]+bm[2])>=0)!=(view=='left'):continue
            pa,pb=project(am),project(bm)
            draw.line([pa,pb],fill=color,width=2)
            for x,y in [pa,pb]:draw.ellipse((x-2,y-2,x+2,y+2),fill=color)
    draw.text((35,1315),'Reviewed local adjacency / newly authored links (existing door/hood links retained)',font=title,fill=(25,30,40))
    for i,pair in enumerate(report['pairs']):
        x=35+(i%2)*875;y=1360+(i//2)*31
        short=' <-> '.join(p[4:] for p in pair['parts'])
        gaps=pair['gapRangeMm'];span='none' if gaps[0] is None else f'{gaps[0]:.1f}-{gaps[1]:.1f} mm'
        color=tuple(int(v*255) for v in colorsys.hsv_to_rgb(i/len(report['pairs']),.9,.65))
        draw.rectangle((x,y+4,x+12,y+16),fill=color)
        draw.text((x+22,y),f'{short}: {pair["addedConnections"]} links / {span}',font=font,fill=(25,30,40))
    draw.text((35,1750),'Geometry and UVs unchanged. C-ring bridges cross real trim gaps. Floating rear spoiler is intentionally separate.',font=font,fill=(50,55,60))
    image.save(path)


if __name__=='__main__':main()
