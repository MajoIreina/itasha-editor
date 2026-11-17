
import assert from 'node:assert/strict';
import {transformLayer,moveLayerAnchor,HANDLES,constrainToPanel,footprint,nextLayerName} from '../src/transform.ts';
import {localPoint,coveragePoint,imageGrabOffset} from '../src/canvasEngine.ts';
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
console.log('Passed: all eight handles preserve opposite anchors under rotation; side handles preserve other axis; surface-map movement remains linear.');
