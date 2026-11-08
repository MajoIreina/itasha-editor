/* Canvas gesture geometry. Dimensions stay in document coordinates, handles in CSS pixels. */
window.SU7ArtworkTransform=(()=>{
  const handles=[['nw',-1,-1],['n',0,-1],['ne',1,-1],['e',1,0],['se',1,1],['s',0,1],['sw',-1,1],['w',-1,0]];
  const frames=new WeakMap();
  function local(p,f){const a=f.rotation*Math.PI/180,c=Math.cos(a),s=Math.sin(a),x=p.x-f.x,y=p.y-f.y;return {x:x*c+y*s,y:-x*s+y*c};}
  function world(p,f){const a=f.rotation*Math.PI/180,c=Math.cos(a),s=Math.sin(a);return {x:f.x+p.x*c-p.y*s,y:f.y+p.x*s+p.y*c};}
  function project(v,region,size){const uv=SU7Paint.projection({x:v[0],y:v[1],z:v[2]},region);return {x:uv[0]*size[0],y:uv[1]*size[1]};}
  function frame(l,region,part,size){
    if(!l.wrap)return {x:l.x,y:l.y,w:l.w,h:l.h,rotation:l.rotation};
    let cache=frames.get(l.wrap);if(!cache)frames.set(l.wrap,cache=new Map());const key=[region,part,...size].join(':');if(cache.has(key))return cache.get(key);
    const zero=project([0,0,0],region,size),r=project(l.wrap.right,region,size),d=project(l.wrap.down,region,size);
    const angle=Math.hypot(r.x-zero.x,r.y-zero.y)>1e-6?Math.atan2(r.y-zero.y,r.x-zero.x):Math.atan2(d.y-zero.y,d.x-zero.x)-Math.PI/2;
    const bounds=SU7WrapDrawing.bounds(l.wrap,region,part,size,angle),value=bounds?{...bounds,rotation:angle*180/Math.PI}:null;cache.set(key,value);return value;
  }
  function hit(p,f,scale){
    if(!f)return null;const q=local(p,f),t=9*scale;
    if(Math.hypot(q.x,q.y+f.h/2+28*scale)<=t)return {mode:'rotate'};
    let best=null,dist=Infinity;
    for(const [name,x,y] of handles){const n=Math.hypot(q.x-x*f.w/2,q.y-y*f.h/2);if(n<=t&&n<dist){best={name,x,y,mode:x&&y?'scale':'stretch'};dist=n;}}
    return best;
  }
  function cursor(handle,f){if(!handle)return 'default';if(handle.mode==='rotate')return 'grab';const a=Math.atan2(handle.y,handle.x)+f.rotation*Math.PI/180,index=((Math.round(a/(Math.PI/4))%4)+4)%4;return ['ew-resize','nwse-resize','ns-resize','nesw-resize'][index];}
  function resize(original,f,start,p,handle,centered=false,free=false,anchor=null){
    const a=local(start,f),b=local(p,f),dx=b.x-a.x,dy=b.y-a.y;
    const fixed=anchor?local(anchor,f):{x:0,y:0};
    const extent=(side,length,offset)=>{const n=centered?side*length/2-offset:side*length;return Math.abs(n)<length*.05?side*length/2:n;};
    const ex=handle.x?extent(handle.x,f.w,fixed.x):0,ey=handle.y?extent(handle.y,f.h,fixed.y):0;
    let sx=handle.x?1+dx/ex:1,sy=handle.y?1+dy/ey:1;
    if(handle.mode==='scale'&&!free){let s=1+(dx*ex+dy*ey)/(ex*ex+ey*ey);s=Math.max(Math.max(1/original.w,1/original.h),Math.min(Math.min(30000/original.w,30000/original.h),s));sx=sy=s;}
    else{sx=Math.max(1/original.w,Math.min(30000/original.w,sx));sy=Math.max(1/original.h,Math.min(30000/original.h,sy));}
    const w=original.w*sx,h=original.h*sy;
    if(centered)return {x:original.x,y:original.y,w,h};
    // Fix the opposite edge/corner in the rotated document coordinate system.
    const shift=world({x:handle.x*(w-original.w)/2,y:handle.y*(h-original.h)/2},{x:0,y:0,rotation:f.rotation});
    return {x:original.x+shift.x,y:original.y+shift.y,w,h};
  }
  return {handles,local,world,project,frame,hit,cursor,resize};
})();
