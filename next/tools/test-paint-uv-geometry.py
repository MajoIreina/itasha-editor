from paint_uv_geometry import overlap_area, positive_overlaps

a=[(0.,0.),(10.,0.),(0.,10.)]
b=[(1.,1.),(11.,1.),(1.,11.)]
assert abs(overlap_area(a,b)-32.)<1e-9
assert abs(overlap_area(a,list(reversed(b)))-32.)<1e-9
assert overlap_area(a,[(10.,0.),(10.,10.),(0.,10.)])==0
assert overlap_area(a,[(20.,20.),(30.,20.),(20.,30.)])==0
assert positive_overlaps([a,b])==[(0,1,32.)]
assert positive_overlaps([a,[(10.,0.),(10.,10.),(0.,10.)]])==[]
print('Passed: coplanar positive overlap, reversed winding, shared edge and disjoint triangles.')
