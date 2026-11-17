
import assert from 'node:assert/strict';
import {transformLayer,moveLayerAnchor,HANDLES,constrainToPanel,footprint,nextLayerName} from '../../src/features/editor/transform.ts';
import {localPoint,coveragePoint,imageGrabOffset} from '../../src/features/editor/canvasEngine.ts';
import {chartVector,metricVector,coverageMetricFrame,controlPoints,nearestControl,transformMetricLayer} from '../../src/features/editor/metricControls.ts';
const layer={id:'test',name:'test',opacity:1,visible:true,locked:false,placement:{surfaceId:'su7.front-left-door',profileVersion:1,xMm:250,yMm:300,widthMm:200,heightMm:100,rotationDeg:35},source:{kind:'shape',shape:'rectangle',fill:'#ffffff'}};
const close=(a,b)=>assert(Math.abs(a-b)<1e-6,`${a} != ${b}`);
for(let i=0;i<8;i++){
 const [hx,hy]=HANDLES[i],a=35*Math.PI/180,c=Math.cos(a),s=Math.sin(a);
 const rotate=(x,y)=>({x:x*c-y*s,y:x*s+y*c});const moving=rotate(hx*100,hy*50),fixed=rotate(-hx*100,-hy*50),delta=rotate(hx*20,hy*15);
 const start={x:250+moving.x,y:300+moving.y};const result=transformLayer(layer,start,{x:start.x+delta.x,y:start.y+delta.y},i,false,false);
 const opposite=rotate(-hx*result.placement.widthMm/2,-hy*result.placement.heightMm/2);
 close(result.placement.xMm+opposite.x,250+fixed.x);close(result.placement.yMm+opposite.y,300+fixed.y);
 if(!hx)close(result.placement.widthMm,200);if(!hy)close(result.placement.heightMm,100);
}
const moved=transformLayer(layer,{x:100,y:200},{x:112,y:207},null,false,false);close(moved.placement.xMm,262);close(moved.placement.yMm,307);
for(let i=0;i<8;i++)for(const amount of [30,-500,50000]){
 const locked={...layer,placement:{...layer.placement,aspectRatioLocked:true}};
 const [hx,hy]=HANDLES[i],a=35*Math.PI/180,c=Math.cos(a),s=Math.sin(a);
 const rotate=(x,y)=>({x:x*c-y*s,y:x*s+y*c});
 const delta=rotate(hx*amount,hy*amount),result=transformLayer(locked,{x:0,y:0},delta,i,false,false);
 close(result.placement.widthMm/result.placement.heightMm,2);
 assert(result.placement.widthMm>=1&&result.placement.widthMm<=10000);
 assert(result.placement.heightMm>=1&&result.placement.heightMm<=10000);
 const fixed=rotate(-hx*100,-hy*50),opposite=rotate(-hx*result.placement.widthMm/2,-hy*result.placement.heightMm/2);
 close(result.placement.xMm+opposite.x,250+fixed.x);close(result.placement.yMm+opposite.y,300+fixed.y);
}
const grab=localPoint(layer,285,320);
const transferred=moveLayerAnchor(layer,'su7.hood',{x:410,y:520},grab);
const a=35*Math.PI/180,c=Math.cos(a),s=Math.sin(a);
close(transferred.placement.xMm+(grab.x*c-grab.y*s),410);
close(transferred.placement.yMm+(grab.x*s+grab.y*c),520);
assert.equal(transferred.placement.surfaceId,'su7.hood');
// Foreign chart position is intentionally unrelated to the layer's anchor chart.
const fragments=[{partId:'su7.hood',paintUv:[[3000,4000],[3600,4000],[3000,4800]],imageUv:[[.1,.2],[.9,.2],[.1,.8]]}];
let sampledX;
const bitmap={width:256,height:256,getContext:()=>({getImageData:(x)=>{sampledX=x;return {data:[255,255,255,255]};}})};
for(const flipX of [false,true]){
 const flipped={...layer,placement:{...layer.placement,flipX}};
 const point=coveragePoint(flipped,bitmap,fragments,'su7.hood',3150,4200);
 close(point.x,.3);close(point.y,.35);
 const offset=imageGrabOffset(flipped,point);close(offset.x,-40);close(offset.y,-15);
 assert.equal(sampledX,Math.floor((flipX?.7:.3)*256),'flip affects texture alpha sample exactly once, not the drag coordinate');
 assert.equal(coveragePoint(flipped,bitmap,fragments,'su7.front-left-door',3150,4200),null);
}
const inside=(x,y)=>x>=0&&x<=500&&y>=0&&y<=600;
const outside={...layer,placement:{...layer.placement,xMm:650}};
const bounded=constrainToPanel(layer,outside,inside);
assert(bounded.placement.xMm>250&&bounded.placement.xMm<500);
assert(footprint(bounded).every(p=>inside(p.x,p.y)));
const invalid={...layer,placement:{...layer.placement,xMm:-500}};
assert.equal(constrainToPanel(invalid,{...invalid,name:'新名称'},inside).name,'新名称');
assert.equal(constrainToPanel(layer,{...outside,placement:{...outside.placement,crossSurface:true}},inside).placement.xMm,650);
assert.equal(nextLayerName([{...layer,name:'图层2'},{...layer,name:'图层12'},{...layer,name:'自定义'}]),'图层13');

// Independent synthetic coverage: known physical-mm basis followed by chart packing.
// The source fragment contains (.5,.5); foreign/off-centre triangles must not
// establish a competing control frame. Texture flip does not affect geometry.
const add=(a,b)=>({x:a.x+b.x,y:a.y+b.y});
const rotatePoint=(p,angle)=>({x:p.x*Math.cos(angle)-p.y*Math.sin(angle),y:p.x*Math.sin(angle)+p.y*Math.cos(angle)});
const bases=[
 {x:{x:2,y:.4},y:{x:.7,y:1.3}},
 {x:{x:-2,y:.4},y:{x:.7,y:1.3}},
 {x:{x:.03,y:0},y:{x:0,y:.03}},
 {x:{x:7,y:3},y:{x:-4,y:1}},
];
let metricQueries=0;
for(const basis of bases)for(const flipX of [false,true]){
 const subject={...layer,placement:{...layer.placement,flipX}},p=subject.placement,angle=p.rotationDeg*Math.PI/180,center={x:p.xMm,y:p.yMm};
 const imageUv=[[.2,.2],[.9,.2],[.2,.9]];
 const paintUv=imageUv.map(([x,y])=>{
  const mapped=add(center,chartVector(basis,rotatePoint({x:(x-.5)*p.widthMm,y:(y-.5)*p.heightMm},angle)));
  return [mapped.x,mapped.y];
 });
 const fragment={partId:p.surfaceId,imageUv,paintUv};
 const inferred=coverageMetricFrame(subject,[{...fragment,partId:'foreign'},fragment]);
 assert(inferred);
 for(const axis of ['x','y'])for(const coordinate of ['x','y'])close(inferred[axis][coordinate],basis[axis][coordinate]);
 const controls=controlPoints(subject,inferred,22);
 close(Math.hypot(controls[8].x-controls[1].x,controls[8].y-controls[1].y),22);
 for(let i=0;i<9;i++)assert.equal(nearestControl(controls,controls[i],.001),i);
 for(let i=0;i<8;i++)for(const aspectRatioLocked of [false,true])for(const shift of [false,true])for(const amount of [20,-500,50000]){
  const original={...subject,placement:{...p,aspectRatioLocked}},[hx,hy]=HANDLES[i];
  const metricDelta=rotatePoint({x:hx*amount,y:hy*amount*.75},angle),start=controls[i],current=add(start,chartVector(basis,metricDelta));
  const result=transformMetricLayer(original,inferred,start,current,i,false,shift);assert(result);
  const oracle=transformLayer({...original,placement:{...original.placement,xMm:0,yMm:0}},{x:0,y:0},metricDelta,i,false,shift);
  close(result.placement.widthMm,oracle.placement.widthMm);close(result.placement.heightMm,oracle.placement.heightMm);
  const expected=add(center,chartVector(basis,{x:oracle.placement.xMm,y:oracle.placement.yMm}));
  close(result.placement.xMm,expected.x);close(result.placement.yMm,expected.y);
  const after=controlPoints(result,inferred,22),opposite=(i+4)%8;
  close(after[opposite].x,controls[opposite].x);close(after[opposite].y,controls[opposite].y);
  const inverse=metricVector(inferred,chartVector(inferred,metricDelta));close(inverse.x,metricDelta.x);close(inverse.y,metricDelta.y);
  metricQueries++;
 }
 const start=add(center,chartVector(basis,{x:0,y:-100})),current=add(center,chartVector(basis,rotatePoint({x:0,y:-100},Math.PI/4)));
 const turned=transformMetricLayer(subject,inferred,start,current,null,true,false);assert(turned);close(turned.placement.rotationDeg,p.rotationDeg+45);
 const snapped=transformMetricLayer(subject,inferred,start,current,null,true,true);assert(snapped);close(snapped.placement.rotationDeg,75);
 close(turned.placement.xMm,p.xMm);close(turned.placement.yMm,p.yMm);
 assert.equal(coverageMetricFrame(subject,[{...fragment,partId:'foreign'}]),null);
 assert.equal(coverageMetricFrame(subject,[{...fragment,imageUv:[[0,0],[.1,0],[0,.1]]}]),null);
 assert.equal(coverageMetricFrame(subject,[{...fragment,imageUv:[[.5,.5],[.5,.5],[.5,.5]]}]),null);
 assert.equal(coverageMetricFrame(subject,[{...fragment,paintUv:[[NaN,0],[0,1],[1,0]]}]),null);
 assert.equal(coverageMetricFrame({...subject,placement:{...p,xMm:p.xMm+10000}},[fragment]),null);
 assert.equal(transformMetricLayer(subject,{x:{x:1,y:1},y:{x:2,y:2}},start,current,0,false,false),null);
 assert.equal(transformMetricLayer(subject,inferred,start,{x:Infinity,y:1},0,false,false),null);
}
console.log(`Passed: ${metricQueries} metric resize queries preserve physical dimensions and opposite chart anchors; scaled/skewed/reflected coverage, rotation, picking and invalid-frame guards agree.`);
