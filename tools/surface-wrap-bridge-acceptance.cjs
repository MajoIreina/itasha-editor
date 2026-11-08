const assert=require('node:assert/strict');
const fs=require('node:fs');
const vm=require('node:vm');
const path=require('node:path');

class Vector3 {
  constructor(x=0,y=0,z=0){this.x=x;this.y=y;this.z=z;}
  toArray(){return [this.x,this.y,this.z];}
  clone(){return new Vector3(this.x,this.y,this.z);}
  add(v){this.x+=v.x;this.y+=v.y;this.z+=v.z;return this;}
  sub(v){this.x-=v.x;this.y-=v.y;this.z-=v.z;return this;}
  addScaledVector(v,s){this.x+=v.x*s;this.y+=v.y*s;this.z+=v.z*s;return this;}
  multiplyScalar(s){this.x*=s;this.y*=s;this.z*=s;return this;}
  dot(v){return this.x*v.x+this.y*v.y+this.z*v.z;}
  cross(v){const x=this.y*v.z-this.z*v.y,y=this.z*v.x-this.x*v.z,z=this.x*v.y-this.y*v.x;this.x=x;this.y=y;this.z=z;return this;}
  lengthSq(){return this.dot(this);}
  length(){return Math.sqrt(this.lengthSq());}
  normalize(){const length=this.length();return length?this.multiplyScalar(1/length):this;}
  distanceTo(v){return this.clone().sub(v).length();}
}

function gridMesh(name,part,minX,maxX,z,cells=32){
  const vertices=Array.from({length:cells+1},(_,x)=>Array.from({length:cells+1},(_,y)=>new Vector3(
    minX+(maxX-minX)*x/cells,y/cells,z
  )));
  const faces=[];
  for(let x=0;x<cells;x++)for(let y=0;y<cells;y++){
    const a=vertices[x][y],b=vertices[x+1][y],c=vertices[x+1][y+1],d=vertices[x][y+1];
    faces.push({part,region:'left',points:[a,b,c]},{part,region:'left',points:[a,c,d]});
  }
  return {name,userData:{studioFaces:faces}};
}

const paint={projection:p=>[p.x/4,1-p.y],wrapLimits:{version:3,coverageVersion:4,maxFaces:48000}};
const runtime={window:{SU7Paint:paint},SU7Paint:paint};
vm.runInNewContext(fs.readFileSync(path.join(__dirname,'..','app','surface-wrap.js'),'utf8'),runtime);
const SurfaceWrap=runtime.window.SU7SurfaceWrap;

const door=gridMesh('door-shell','doorLB',0,1,0);
const body=gridMesh('body-shell','body',1.015,2.015,.015);
const bumper=gridMesh('bumper-shell','rearBumper',2.065,3.065,.03);
const graph=new SurfaceWrap([door,body,bumper],Vector3);
const wrap=graph.make({
  mesh:door.name,
  face:(31*32+16)*2,
  point:[.985,.515,0],
  normal:[0,0,1],
  right:[1,0,0],
  down:[0,1,0],
  width:.6,
  height:.6,
  at:[.5,.5]
});

const coveredParts=new Set(wrap.patches.map(p=>p.part));
assert.equal(wrap.version,3,'new wraps use the migrated v3 coverage algorithm');
assert.ok(coveredParts.has('doorLB'),'the starting rear-door shell remains covered');
assert.ok(coveredParts.has('body'),'the decal crosses the disconnected door-to-body seam');
assert.ok(!coveredParts.has('rearBumper'),'the decal does not bridge to an unrelated painted shell');
console.log('PASS disconnected rear door joins adjacent body without reaching bumper');
