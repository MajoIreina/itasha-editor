import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
export async function checkOrientationTransport({ipc,reportDir,checks}) {
 const project=(await ipc('read_view')).project;
 const assetRoot=new URL('../../assets/paint/',import.meta.url);
 const manifest=JSON.parse(fs.readFileSync(new URL('manifest.json',assetRoot),'utf8'));
 const map=JSON.parse(fs.readFileSync(new URL(manifest.runtimePaint,assetRoot),'utf8'));
 assert.equal(map.paintFingerprint,manifest.paintFingerprint);
 const center=part=>{const t=map.triangles.filter(t=>t.partId===part).sort((a,b)=>area(b)-area(a))[0];return [0,1].map(i=>t.paintUv.reduce((s,p)=>s+p[i],0)/3);};
 const records=[];
 for(const side of ['left','right'])for(const flipX of [false,true]){
  const from='su7.front-'+side+'-door',start=center(from);
  const source={...project.editor.layers[0],placement:{...project.editor.layers[0].placement,surfaceId:from,xMm:start[0],yMm:start[1],rotationDeg:27,flipX,widthMm:100,heightMm:60}};
  delete source.placement.anchor;
  for(const target of ['su7.front-'+side+'-fender','su7.hood']){
   const point=center(target);
   const moved=await ipc('viewport_transport',{session:project.session,layer:source,surfaceId:target,point,offset:[0,0]});
   const back=await ipc('viewport_transport',{session:project.session,layer:moved,surfaceId:from,point:start,offset:[0,0]});
   assert(Math.cos((back.placement.rotationDeg-27)*Math.PI/180)>.99999);
   assert.equal(back.placement.flipX??false,flipX);
   assert.equal(moved.placement.widthMm,100);assert.equal(moved.placement.heightMm,60);
   assert.equal(moved.placement.anchor.partId,target);
   const coverage=await ipc('viewport_coverage',{session:project.session,layer:moved});assert(coverage.length>0);
   const grabbed=await ipc('viewport_transport',{session:project.session,layer:source,surfaceId:target,point,offset:[.1,.1]});
   const ox=(Boolean(grabbed.placement.flipX)!==flipX)?-.1:.1;
   const grabCoverage=await ipc('viewport_coverage',{session:project.session,layer:grabbed});
   const image=imageAt(grabCoverage,target,point);
   assert(image,'grabbed point must remain covered on the target chart');
   assert(Math.abs((image[0]-.5)*100-ox)<.002);
   assert(Math.abs((image[1]-.5)*60-.1)<.002);
   records.push({from,target,flipX,rotation:moved.placement.rotationDeg,back:back.placement.rotationDeg,fragments:coverage.length});
  }
 }
 assert.equal((await ipc('read_view')).project.revision,project.revision);
 fs.writeFileSync(path.join(reportDir,'orientation-transport.json'),JSON.stringify(records,null,2));
 checks.push('shared Rust transport: left/right door to fender/hood and back, mirrored and unmirrored artwork retain orientation, size and valid coverage');
}
function area(t){const [a,b,c]=t.world,e=b.map((v,i)=>v-a[i]),f=c.map((v,i)=>v-a[i]);return Math.hypot(e[1]*f[2]-e[2]*f[1],e[2]*f[0]-e[0]*f[2],e[0]*f[1]-e[1]*f[0]);}
function imageAt(fragments,part,point){
 for(const f of fragments.filter(f=>f.partId===part)){
  const [a,b,c]=f.paintUv,e=[b[0]-a[0],b[1]-a[1]],v=[c[0]-a[0],c[1]-a[1]],q=[point[0]-a[0],point[1]-a[1]],d=e[0]*v[1]-e[1]*v[0];
  if(Math.abs(d)<1e-10)continue;
  const y=(q[0]*v[1]-q[1]*v[0])/d,z=(e[0]*q[1]-e[1]*q[0])/d,x=1-y-z;
  if(Math.min(x,y,z)>=-1e-5)return [0,1].map(i=>f.imageUv[0][i]*x+f.imageUv[1][i]*y+f.imageUv[2][i]*z);
 }
 return null;
}
