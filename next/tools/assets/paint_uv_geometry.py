"""Exact 2D positive-area intersection with a spatial broad phase.

A 3D triangle BVH does not report coplanar triangle intersections reliably.
"""
import math
from collections import defaultdict

def cross(a,b): return a[0]*b[1]-a[1]*b[0]
def sub(a,b): return (a[0]-b[0],a[1]-b[1])
def overlap_area(a,b):
    poly=list(a)
    sign=1 if cross(sub(b[1],b[0]),sub(b[2],b[0]))>0 else -1
    for i in range(3):
        if not poly: return 0.
        start,end=b[i],b[(i+1)%3]
        direction=sub(end,start)
        result=[]
        previous=poly[-1]
        pd=cross(direction,sub(previous,start))*sign
        for current in poly:
            cd=cross(direction,sub(current,start))*sign
            if (pd>=0)!=(cd>=0):
                k=pd/(pd-cd)
                result.append((previous[0]+(current[0]-previous[0])*k,previous[1]+(current[1]-previous[1])*k))
            if cd>=0: result.append(current)
            previous,pd=current,cd
        poly=result
    return abs(sum(cross(poly[i],poly[(i+1)%len(poly)]) for i in range(len(poly))))/2 if len(poly)>2 else 0.

def positive_overlaps(triangles, minimum_area=.0001):
    grid=defaultdict(list)
    bounds=[]
    cell=64.
    results=[]
    for i,tri in enumerate(triangles):
        low=[min(p[k] for p in tri) for k in range(2)]
        high=[max(p[k] for p in tri) for k in range(2)]
        keys=[(x,y) for x in range(math.floor(low[0]/cell),math.floor(high[0]/cell)+1)
              for y in range(math.floor(low[1]/cell),math.floor(high[1]/cell)+1)]
        for j in {j for key in keys for j in grid[key]}:
            other_low,other_high=bounds[j]
            if any(high[k]<=other_low[k] or low[k]>=other_high[k] for k in range(2)): continue
            area=overlap_area(tri,triangles[j])
            if area>minimum_area: results.append((j,i,area))
        bounds.append((low,high))
        for key in keys: grid[key].append(i)
    return results
