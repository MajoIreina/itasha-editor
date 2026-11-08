window.SU7WrapDrawing=(()=>{
  const projections=new WeakMap(),buffers=new WeakMap(),plans=new WeakMap();
  const directions={left:[0,0,1],right:[0,0,-1],top:[0,1,0],front:[-1,0,0],rear:[1,0,0]};
  function patches(wrap,r,part='all'){
    let views=projections.get(wrap);if(!views)projections.set(wrap,views=new Map());
    // Geometry may be restored after an older project first opens without 3D.
    const geometry=wrap.patches.every(p=>p.xyz?.length===p.faces.length*9),key=r+':'+geometry;
    if(!views.has(key)){
      const result=[],direction=directions[r];
      for(const patch of wrap.patches){
        if(!patch.xyz){if(patch.region===r)result.push(patch);continue;}
        const projected={...patch,region:r,faces:[],xy:[],uv:[],depth:[]};
        for(let i=0;i<patch.faces.length;i++){
          const xyz=patch.xyz.slice(i*9,i*9+9),a=xyz.slice(0,3),b=xyz.slice(3,6).map((v,j)=>v-a[j]),c=xyz.slice(6,9).map((v,j)=>v-a[j]);
          const n=[b[1]*c[2]-b[2]*c[1],b[2]*c[0]-b[0]*c[2],b[0]*c[1]-b[1]*c[0]];
          if(n.reduce((sum,v,j)=>sum+v*direction[j],0)<=1e-12)continue;
          const points=[];for(let j=0;j<9;j+=3)points.push([xyz[j],xyz[j+1],xyz[j+2]]);
          projected.faces.push(patch.faces[i]);projected.uv.push(...patch.uv.slice(i*6,i*6+6));projected.depth.push(points.reduce((value,p)=>value+p.reduce((sum,v,j)=>sum+v*direction[j],0)/3,0));
          for(const point of points)projected.xy.push(...SU7Paint.projection({x:point[0],y:point[1],z:point[2]},r));
        }
        if(projected.faces.length)result.push(projected);
      }
      views.set(key,result);
    }
    return views.get(key).filter(p=>part==='all'||p.part===part);
  }
  function matrix(xy,uv){const [x0,y0,x1,y1,x2,y2]=xy,[u0,v0,u1,v1,u2,v2]=uv,du1=u1-u0,dv1=v1-v0,du2=u2-u0,dv2=v2-v0,det=du1*dv2-du2*dv1;if(Math.abs(det)<1e-12)return null;const a=((x1-x0)*dv2-(x2-x0)*dv1)/det,c=((x2-x0)*du1-(x1-x0)*du2)/det,b=((y1-y0)*dv2-(y2-y0)*dv1)/det,d=((y2-y0)*du1-(y1-y0)*du2)/det;return [a,b,c,d,x0-a*u0-c*v0,y0-b*u0-d*v0];}
  // Wrap values are immutable after their bind-pose geometry is restored. Cache
  // triangle order and affine transforms, never a raster at export resolution.
  function drawPlan(wrap,r,part,size){
    let cache=plans.get(wrap);if(!cache)plans.set(wrap,cache=new Map());
    const key=[r,part,...size,wrap.patches.every(p=>p.xyz?.length===p.faces.length*9)].join(':');
    if(cache.has(key))return cache.get(key);
    const triangles=[];
    for(const p of patches(wrap,r,part))for(let i=0;i<p.xy.length;i+=6){
      const xy=p.xy.slice(i,i+6).map((v,j)=>v*size[j%2]),uv=p.uv.slice(i,i+6),m=matrix(xy,uv);if(!m)continue;
      const path=new Path2D();path.moveTo(xy[0],xy[1]);path.lineTo(xy[2],xy[3]);path.lineTo(xy[4],xy[5]);path.closePath();
      const minU=Math.max(0,Math.min(uv[0],uv[2],uv[4])),maxU=Math.min(1,Math.max(uv[0],uv[2],uv[4])),minV=Math.max(0,Math.min(uv[1],uv[3],uv[5])),maxV=Math.min(1,Math.max(uv[1],uv[3],uv[5]));
      if(minU>=maxU||minV>=maxV)continue;
      triangles.push({path,m,mask:r+':'+p.part,depth:p.depth?.[i/6]??0,minU,maxU,minV,maxV});
    }
    triangles.sort((a,b)=>a.depth-b.depth);cache.set(key,triangles);return triangles;
  }
  function draw(destination,wrap,r,part,im,size,opacity=1,paths={},live=false){
    if(live&&window.SU7WrapRasterizer?.draw(destination,wrap,r,part,im,size,opacity,paths))return;
    let buffer=buffers.get(destination.canvas);if(!buffer)buffers.set(destination.canvas,buffer=document.createElement('canvas'));
    if(buffer.width!==destination.canvas.width||buffer.height!==destination.canvas.height){buffer.width=destination.canvas.width;buffer.height=destination.canvas.height;}
    const c=buffer.getContext('2d');c.resetTransform();c.clearRect(0,0,buffer.width,buffer.height);c.setTransform(destination.getTransform());
    const iw=im.width,ih=im.height;let activeMask;
    for(const t of drawPlan(wrap,r,part,size)){
      const mask=paths[t.mask];if(mask!==activeMask){if(activeMask)c.restore();activeMask=mask;if(mask){c.save();c.clip(mask,'evenodd');}}
      c.save();c.clip(t.path);c.globalAlpha=1;c.transform(...t.m);
      // Only sample the source pixels used by this triangle. Drawing a complete
      // multi-megapixel image thousands of times made curved dragging expensive.
      // A one-pixel border preserves interpolation at the triangle edges.
      const u=Math.max(0,t.minU-1/iw),v=Math.max(0,t.minV-1/ih),w=Math.min(1,t.maxU+1/iw)-u,h=Math.min(1,t.maxV+1/ih)-v;
      c.drawImage(im,u*iw,v*ih,w*iw,h*ih,u,v,w,h);c.restore();
    }
    if(activeMask)c.restore();destination.save();destination.resetTransform();destination.globalAlpha=opacity;destination.drawImage(buffer,0,0);destination.restore();
  }
  function pick(wrap,r,part,point,size){let hit=null,depth=-Infinity;for(const p of patches(wrap,r,part)){for(let i=0;i<p.xy.length;i+=6){const xy=p.xy.slice(i,i+6).map((v,j)=>v*size[j%2]),[a,b,c,d,e,f]=xy,den=(d-f)*(a-e)+(e-c)*(b-f);if(Math.abs(den)<1e-9)continue;const u=((d-f)*(point.x-e)+(e-c)*(point.y-f))/den,v=((f-b)*(point.x-e)+(a-e)*(point.y-f))/den;if(u<0||v<0||u+v>1)continue;const uv=[0,1].map(j=>u*p.uv[i+j]+v*p.uv[i+2+j]+(1-u-v)*p.uv[i+4+j]),z=p.depth?.[i/6]??0;if(z>depth&&uv.every(v=>v>=0&&v<=1)){hit=uv;depth=z;}}}return hit;}
  function bounds(wrap,r,part,size,angle=0){
    let minX=Infinity,minY=Infinity,maxX=-Infinity,maxY=-Infinity;const c=Math.cos(angle),s=Math.sin(angle);
    for(const patch of patches(wrap,r,part)){
      for(let i=0;i<patch.xy.length;i+=6){
        let polygon=[0,2,4].map(j=>[patch.xy[i+j]*size[0],patch.xy[i+j+1]*size[1],patch.uv[i+j],patch.uv[i+j+1]]);
        // A patch stores full triangles. Clip to the image UV square before measuring
        // selection bounds so an adjacent large triangle cannot enlarge the frame.
        for(const [axis,edge,sign] of [[2,0,1],[2,1,-1],[3,0,1],[3,1,-1]]){
          const clipped=[];for(let j=0;j<polygon.length;j++){const a=polygon[j],b=polygon[(j+1)%polygon.length],da=(a[axis]-edge)*sign,db=(b[axis]-edge)*sign;if(da>=0)clipped.push(a);if((da>=0)!==(db>=0)){const t=da/(da-db);clipped.push(a.map((v,k)=>v+(b[k]-v)*t));}}polygon=clipped;if(!polygon.length)break;
        }
        for(const [x,y] of polygon){const u=x*c+y*s,v=-x*s+y*c;minX=Math.min(minX,u);maxX=Math.max(maxX,u);minY=Math.min(minY,v);maxY=Math.max(maxY,v);}
      }
    }
    if(!Number.isFinite(minX)||maxX-minX<1e-6||maxY-minY<1e-6)return null;
    const x=(minX+maxX)/2,y=(minY+maxY)/2;return {x:x*c-y*s,y:x*s+y*c,w:maxX-minX,h:maxY-minY};
  }
  return {matrix,draw,pick,bounds,patches};
})();
