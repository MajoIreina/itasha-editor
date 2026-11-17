"""Keep recovery within already registered exterior sources; preserve all baseline faces.

python tools/assets/finalize-exterior-candidate.py baseline.json oriented-unsealed.json new-output.json
"""
import json
import sys
import struct
import math
from pathlib import Path
from collections import Counter
baseline, candidate, output = map(Path, sys.argv[1:4])
if output.exists(): raise RuntimeError('Output exists')
old = json.loads(baseline.read_text(encoding='utf-8'))
new = json.loads(candidate.read_text(encoding='utf-8'))
known_sources = {t['id']['sourceMesh'] for t in old['triangles']}
identity = lambda t: tuple(t['id'][k] for k in ('sourceMesh','primitive','sourceTriangle','fragment'))
old_ids = {identity(t) for t in old['triangles']}
removed = Counter(t['id']['sourceMesh'] for t in new['triangles'] if t['id']['sourceMesh'] not in known_sources)
new['triangles'] = [t for t in new['triangles'] if t['id']['sourceMesh'] in known_sources]
assert old_ids <= {identity(t) for t in new['triangles']}, 'Baseline exterior faces must not disappear'
original = {identity(t): t for t in old['triangles']}
for t in new['triangles']:
    if identity(t) in original:
        assert all(t[k] == original[identity(t)][k] for k in ('world','sourceBarycentric','partId','regionId'))
# Very narrow fragments can lose their UV area when converted to float32 at
# multi-metre atlas offsets. Keep them in a reserved, low-coordinate strip.
f32=lambda x: struct.unpack('<f',struct.pack('<f',x))[0]
micro=[]
cursor={}
for t in new['triangles']:
    uv=[[f32(p[0]+20),f32(p[1]+20)] for p in t['paintUv']]
    a,b,c=uv
    determinant=f32(f32(f32(b[0]-a[0])*f32(c[1]-a[1]))-f32(f32(b[1]-a[1])*f32(c[0]-a[0])))
    if abs(determinant)<1e-12:
        w=t['world']
        sub=lambda a,b:[x-y for x,y in zip(a,b)]
        dot=lambda a,b:sum(x*y for x,y in zip(a,b))
        edges=[(i,(i+1)%3) for i in range(3)]
        i,j=max(edges,key=lambda e:dot(sub(w[e[0]],w[e[1]]),sub(w[e[0]],w[e[1]])))
        k=3-i-j
        e,f=sub(w[j],w[i]),sub(w[k],w[i])
        length=math.sqrt(dot(e,e))
        cross=[e[1]*f[2]-e[2]*f[1],e[2]*f[0]-e[0]*f[2],e[0]*f[1]-e[1]*f[0]]
        height=math.sqrt(dot(cross,cross))/length*1000
        part=t['partId'];x=cursor.get(part,0.)
        uv=[None]*3
        uv[i]=[x,0.];uv[j]=[x+length*1000,0.];uv[k]=[x+dot(f,e)/length*1000,height]
        cursor[part]=x+length*1000+1
        micro.append({'id':identity(t),'heightMm':height})
    t['paintUv']=uv
assert all(t['heightMm']<20 for t in micro), 'Micro strip would overlap main charts'
new['paintFingerprint'] = ''
output.write_text(json.dumps(new,separators=(',',':')),encoding='utf-8')
output.with_suffix('.exterior-scope.json').write_text(json.dumps({'baselineTriangles':len(old_ids),'currentTriangles':len(new['triangles']),
    'excludedUnregisteredSourceFragments':dict(removed),'baselineWorldGeometryPreserved':True,'float32MicroFragments':len(micro)},indent=2),encoding='utf-8')
print(f'Preserved {len(old_ids)} baseline triangles, total {len(new["triangles"])}; excluded unregistered inner sources: {dict(removed)}')
