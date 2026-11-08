const {test}=require('node:test');
const assert=require('node:assert/strict');
const Camera=require('../../app/camera-navigation.js');
const close=(a,b,epsilon=1e-9)=>assert.ok(Math.abs(a-b)<epsilon,`${a} != ${b}`);
const dot=(a,b)=>a.reduce((n,x,i)=>n+x*b[i],0);
const sub=(a,b)=>a.map((x,i)=>x-b[i]);
function project(camera,p){const pose=camera.pose(),delta=sub(p,pose.position),depth=-dot(delta,pose.direction),scale=Math.tan(pose.fov*Math.PI/360);return [dot(delta,pose.right)/depth/scale/(camera.width/camera.height),dot(delta,pose.up)/depth/scale];}

test('orbit, named views and viewport resize retain distance and FOV',()=>{
  const camera=new Camera({width:900,height:700}),initial=camera.state;
  for(let i=0;i<100;i++)camera.orbit(25,7*Math.sin(i));
  for(const name of ['left','right','top','front','rear','hero']){camera.view(name);close(camera.state.distance,initial.distance);close(camera.state.fov,initial.fov);}
  camera.viewport(450,900);close(camera.state.distance,initial.distance);close(camera.state.fov,initial.fov);
});
test('FOV changes preserve the actual body bounds without cumulative zoom drift',()=>{
  const camera=new Camera({width:900,height:700}),initial=camera.state,size=camera.framingSize();
  for(let i=0;i<30;i++)for(const fov of [15,90,20,70,35,30]){camera.setFov(fov);close(camera.framingSize(),size,1e-9);}
  close(camera.state.distance,initial.distance,1e-8);
  camera.setFraming(false);camera.setFov(60);close(camera.state.distance,initial.distance,1e-8);close(camera.state.fov,60);
});
test('panning follows screen pixels at every FOV without changing zoom',()=>{
  for(const fov of [15,30,60,90]){
    const camera=new Camera({width:900,height:700});camera.setFov(fov);const before=camera.state,p=[...before.target],a=project(camera,p);
    camera.pan(140,-45);const b=project(camera,p);close((b[0]-a[0])*camera.width/2,140);close(-(b[1]-a[1])*camera.height/2,-45);close(camera.state.fov,before.fov);close(camera.state.distance,before.distance);
  }
});
test('cursor-anchored dolly holds the world point under the same screen pixel',()=>{
  const camera=new Camera({width:900,height:700}),point=[1,.9,.6],a=project(camera,point),initial=camera.state;
  for(let i=0;i<20;i++){camera.dolly(-25,point);const b=project(camera,point);close(a[0],b[0]);close(a[1],b[1]);close(camera.state.fov,initial.fov);}
  for(let i=0;i<20;i++)camera.dolly(25,point);
  close(camera.state.distance,initial.distance);camera.state.target.forEach((x,i)=>close(x,initial.target[i]));
});
test('top view and input bounds stay finite; snapshots and render poses are independent',()=>{
  const camera=new Camera({width:900,height:700});camera.view('top');camera.orbit(100,10000);camera.dolly(-1e6);camera.setFov(1e6);camera.pan(2,3);
  assert.ok(Object.values(camera.pose()).flat().filter(x=>typeof x==='number').every(Number.isFinite));
  assert.ok(camera.state.distance>=camera.limits.distance[0]);assert.ok(camera.state.fov<=90);
  const before=camera.state,snapshot=camera.state;snapshot.target[0]=1e9;assert.deepEqual(camera.state,before);
  assert.equal(camera.restore({...before,target:[NaN,0,0]}),false);camera.orbit(NaN,0);camera.pan(Infinity,0);camera.dolly(NaN);camera.setFov(NaN);assert.deepEqual(camera.state,before);
  assert.deepEqual(camera.renderState({current:true}),before);
  camera.renderState({yaw:-80,pitch:30,fov:50,width:1600,height:900});assert.deepEqual(camera.state,before);
});
test('fit contains all eight body corners and retains the requested lens and direction',()=>{
  const bounds={min:[-2.6,0,-1.1],max:[2.6,1.6,1.1]};
  for(const size of [[1600,900],[400,900],[900,300]])for(const view of ['hero','top','left','front'])for(const fov of [15,30,90]){
    const camera=new Camera({width:size[0],height:size[1]});camera.view(view);camera.setFov(fov);camera.fit();close(camera.state.fov,fov);assert.equal(camera.state.view,view);
    for(const x of [bounds.min[0],bounds.max[0]])for(const y of [bounds.min[1],bounds.max[1]])for(const z of [bounds.min[2],bounds.max[2]])assert.ok(project(camera,[x,y,z]).every(v=>Math.abs(v)<=1));
  }
});
