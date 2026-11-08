/* Continuous decal coordinates on connected factory-painted triangles. */
window.SU7SurfaceWrap=class {
  constructor(meshes,V){
    this.V=V;this.meshes=meshes;this.vertices=[];this.faces=[];this.byMesh=new Map();this.projectedViews=new Map();const weld=new Map();
    for(const mesh of meshes){const list=[];for(const f of mesh.userData.studioFaces){const ids=f.points.map(p=>{const key=p.toArray().map(x=>Math.round(x*10000)).join();let id=weld.get(key);if(id===undefined){id=this.vertices.length;weld.set(key,id);this.vertices.push({p:p.clone(),n:new V(),links:new Map(),faces:[]});}return id;});const weightedNormal=f.points[1].clone().sub(f.points[0]).cross(f.points[2].clone().sub(f.points[0])),normal=weightedNormal.clone().normalize(),id=this.faces.length,face={mesh,part:f.part,region:f.region,ids,normal,index:list.length};this.faces.push(face);list.push(id);for(const i of ids){this.vertices[i].n.add(weightedNormal);this.vertices[i].faces.push(id);}for(let j=0;j<3;j++){const a=ids[j],b=ids[(j+1)%3];this.link(a,b);}}this.byMesh.set(mesh.name,list);}
    for(const v of this.vertices)v.n.normalize();
    // Bridge adjacent body/door shells and small painted islands split from a panel.
    // Unpainted glass, trim, mirrors and interior surfaces never enter this graph.
    const candidatesSet=new Set(this.vertices.map((_,i)=>i)),component=new Int32Array(this.vertices.length).fill(-1);let componentId=0;
    const componentParts=[];for(const root of candidatesSet){if(component[root]>=0)continue;const id=componentId,queue=[root],parts=new Set();component[root]=id;for(let head=0;head<queue.length;head++){const vertex=this.vertices[queue[head]];for(const face of vertex.faces)parts.add(this.faces[face].part);for(const next of vertex.links.keys())if(component[next]<0){component[next]=id;queue.push(next);}}componentParts.push(parts);componentId++;}
    const componentSizes=new Uint32Array(componentId);for(const id of candidatesSet)componentSizes[component[id]]++;
    const bridgeLimit=1024,bridgeTarget=new Int32Array(componentId).fill(-1),bridgeDistance=new Float32Array(componentId).fill(Infinity);
    const adjacentPaintedParts=new Set([
      'doorLF:doorLB','doorLB:doorLF','doorRF:doorRB','doorRB:doorRF',
      'body:doorLF','doorLF:body','body:doorLB','doorLB:body',
      'body:doorRF','doorRF:body','body:doorRB','doorRB:body',
      'body:bonnet','bonnet:body','frontBumper:bonnet','bonnet:frontBumper',
      'body:trunk','trunk:body','body:rearBumper','rearBumper:body',
      'trunk:empennage','empennage:trunk'
    ]);
    // Accept close islands on the same paint panel plus model-verified neighboring panels.
    const neighboringComponents=(a,b)=>[...componentParts[a]].some(pa=>[...componentParts[b]].some(pb=>pa===pb||adjacentPaintedParts.has(pa+':'+pb)));
    const compatibleComponents=(a,b)=>componentSizes[a]<=bridgeLimit||componentSizes[b]<=bridgeLimit||neighboringComponents(a,b);
    const grid=new Map(),cell=.065,key=p=>[p.x,p.y,p.z].map(v=>Math.floor(v/cell));
    for(const id of candidatesSet){const k=key(this.vertices[id].p).join();if(!grid.has(k))grid.set(k,[]);grid.get(k).push(id);}
    const compatible=(a,b)=>{const v=this.vertices[a],other=this.vertices[b];if(v.links.has(b)||component[a]===component[b]||v.n.dot(other.n)<.4)return null;const delta=other.p.clone().sub(v.p),d=delta.length(),normal=v.n.clone().add(other.n).normalize();return d<cell&&Math.abs(delta.dot(normal))<.025?d:null;};
    for(const a of candidatesSet){const v=this.vertices[a],k=key(v.p),ca=component[a];for(let x=-1;x<=1;x++)for(let y=-1;y<=1;y++)for(let z=-1;z<=1;z++)for(const b of grid.get([k[0]+x,k[1]+y,k[2]+z].join())||[]){if(a>=b)continue;const cb=component[b];if(!compatibleComponents(ca,cb))continue;const d=compatible(a,b);if(d===null)continue;if(componentSizes[ca]<=bridgeLimit&&d<bridgeDistance[ca]){bridgeTarget[ca]=cb;bridgeDistance[ca]=d;}if(componentSizes[cb]<=bridgeLimit&&d<bridgeDistance[cb]){bridgeTarget[cb]=ca;bridgeDistance[cb]=d;}if(componentSizes[ca]>bridgeLimit&&componentSizes[cb]>bridgeLimit){if(d<bridgeDistance[ca]){bridgeTarget[ca]=cb;bridgeDistance[ca]=d;}if(d<bridgeDistance[cb]){bridgeTarget[cb]=ca;bridgeDistance[cb]=d;}}}}
    // A panel may meet several shells: the rear bumper touches both quarter
    // panels, and the trunk is split above and below the lamps. Retain each
    // verified neighbor, rather than choosing one nearest shell for the panel.
    // Unknown small islands still attach only to their nearest compatible shell.
    this.seams=0;for(const a of candidatesSet){const v=this.vertices[a],k=key(v.p),ca=component[a],candidates=new Map();for(let x=-1;x<=1;x++)for(let y=-1;y<=1;y++)for(let z=-1;z<=1;z++)for(const b of grid.get([k[0]+x,k[1]+y,k[2]+z].join())||[]){if(a>=b)continue;const cb=component[b];if(!neighboringComponents(ca,cb)&&bridgeTarget[ca]!==cb&&bridgeTarget[cb]!==ca)continue;const d=compatible(a,b);if(d!==null){let list=candidates.get(cb);if(!list)candidates.set(cb,list=[]);list.push([b,d]);}}for(const list of candidates.values()){list.sort((a,b)=>a[1]-b[1]);for(const [b] of list.slice(0,4)){this.link(a,b);this.seams++;}}}

  }
  link(a,b){if(a===b)return;const d=this.vertices[a].p.distanceTo(this.vertices[b].p);this.vertices[a].links.set(b,d);this.vertices[b].links.set(a,d);}
  vec(a){return new this.V(...a);}
  projectedHit(region,u,v){
    const bins=32,view={left:[0,0,1],right:[0,0,-1],top:[0,1,0],front:[-1,0,0],rear:[1,0,0]}[region];if(!view||u<0||u>1||v<0||v>1)return null;
    let index=this.projectedViews.get(region);if(!index)this.projectedViews.clear();
    if(!index){
      const cells=new Map(),size=1/bins;
      for(const face of this.faces){
        if(face.normal.x*view[0]+face.normal.y*view[1]+face.normal.z*view[2]<.015)continue;
        const points=face.ids.map(id=>this.vertices[id].p),xy=points.map(p=>SU7Paint.projection(p,region)),xs=xy.map(p=>p[0]),ys=xy.map(p=>p[1]);
        const x0=Math.max(0,Math.floor(Math.min(...xs)/size)),x1=Math.min(bins-1,Math.floor(Math.min(1,Math.max(...xs))/size)),y0=Math.max(0,Math.floor(Math.min(...ys)/size)),y1=Math.min(bins-1,Math.floor(Math.min(1,Math.max(...ys))/size));
        if(x1<x0||y1<y0)continue;
        const item={face,xy,points};for(let x=x0;x<=x1;x++)for(let y=y0;y<=y1;y++){const key=x+':'+y;let list=cells.get(key);if(!list)cells.set(key,list=[]);list.push(item);}
      }
      index={cells,size};this.projectedViews.set(region,index);
    }
    const list=index.cells.get(Math.min(bins-1,Math.floor(u/index.size))+':'+Math.min(bins-1,Math.floor(v/index.size)))||[];let best=null;
    for(const item of list){
      const [[ax,ay],[bx,by],[cx,cy]]=item.xy,den=(by-cy)*(ax-cx)+(cx-bx)*(ay-cy);if(Math.abs(den)<1e-12)continue;
      const a=((by-cy)*(u-cx)+(cx-bx)*(v-cy))/den,b=((cy-ay)*(u-cx)+(ax-cx)*(v-cy))/den,c=1-a-b;if(a< -1e-7||b< -1e-7||c< -1e-7)continue;
      const point=item.points[0].clone().multiplyScalar(a).addScaledVector(item.points[1],b).addScaledVector(item.points[2],c),depth=point.x*view[0]+point.y*view[1]+point.z*view[2];if(best&&depth<=best.depth)continue;
      best={depth,face:item.face,point:{part:item.face.part,region,u,v,mesh:item.face.mesh.name,face:item.face.index,point:point.toArray(),normal:item.face.normal.toArray()}};
    }
    return best?.point||null;
  }
  withGeometry(wrap){
    // Older projects only stored coordinates for the dominant normal bucket.
    // Retain the bind-pose vertices so every orthographic editor view can project
    // all front-facing triangles, including curved faces assigned to another bucket.
    for(const patch of wrap.patches){
      if(patch.xyz?.length===patch.faces.length*9)continue;
      const faces=this.byMesh.get(patch.mesh);if(!faces)continue;
      patch.xyz=patch.faces.flatMap(index=>this.faces[faces[index]].ids.flatMap(id=>this.vertices[id].p.toArray().map(n=>Math.round(n*1e7)/1e7)));
    }
    return wrap;
  }
  frame(hit,region,rotation=0,previous){
    const n=this.vec(hit.normal),axes={left:[[1,0,0],[0,-1,0]],right:[[-1,0,0],[0,-1,0]],top:[[1,0,0],[0,0,1]],front:[[0,0,1],[0,-1,0]],rear:[[0,0,-1],[0,-1,0]]},[right,down]=axes[region];
    let r;if(previous){const from=this.vec(previous.normal),axis=from.clone().cross(n),dot=Math.max(-1,Math.min(1,from.dot(n)));r=this.vec(previous.right);if(axis.lengthSq()>1e-12)r.applyAxisAngle(axis.normalize(),Math.acos(dot));}else{const angle=rotation*Math.PI/180;r=this.vec(right).multiplyScalar(Math.cos(angle)).addScaledVector(this.vec(down),Math.sin(angle));}
    r.addScaledVector(n,-r.dot(n));if(r.lengthSq()<1e-8)r=this.vec(down).cross(n);r.normalize();const d=r.clone().cross(n).normalize();return {normal:n.toArray(),right:r.toArray(),down:d.toArray()};
  }
  make(spec){
    const start=this.byMesh.get(spec.mesh)?.[spec.face];if(start===undefined)throw Error('无法定位素材所在的外壳三角面');const center=this.vec(spec.point),n=this.vec(spec.normal),right=this.vec(spec.right),down=this.vec(spec.down),radius=Math.min(3,Math.hypot(spec.width*Math.max(spec.at[0],1-spec.at[0]),spec.height*Math.max(spec.at[1],1-spec.at[1]))+.08);
    const distance=new Map(),queue=[];for(const i of this.faces[start].ids){const d=this.vertices[i].p.distanceTo(center);distance.set(i,d);queue.push(i);}const found=new Set();for(let head=0;head<queue.length;head++){const i=queue[head],v=this.vertices[i],d=distance.get(i);v.faces.forEach(f=>found.add(f));for(const [j,len] of v.links){const next=d+len;if(next>radius||this.vertices[j].n.dot(n)<-.35||next>=(distance.get(j)??Infinity)-1e-6)continue;distance.set(j,next);queue.push(j);}if(queue.length>150000)break;}
    const uv=new Map(),map=i=>{if(uv.has(i))return uv.get(i);const v=this.vertices[i],delta=v.p.clone().sub(center),normal=v.n,cos=Math.max(-1,Math.min(1,n.dot(normal))),angle=Math.acos(cos),sin=Math.sin(angle),t=normal.clone().addScaledVector(n,-cos);let x=delta.dot(right),y=delta.dot(down);if(sin>1e-4&&angle<2){t.normalize();const amount=delta.dot(t)*(angle/sin-1);x+=amount*t.dot(right);y+=amount*t.dot(down);}const result=[spec.at[0]+x/spec.width,spec.at[1]+y/spec.height];uv.set(i,result);return result;};
    // Rear-facing concave lips may slope down and still be visible exterior
    // paint. A negative Y normal alone must not remove the bands beside the lamps.
    const groups=new Map();let count=0;for(const id of found){const f=this.faces[id];if(f.normal.dot(n)<-.1)continue;if(/^doorL/.test(f.part)&&f.normal.z<-.01||/^doorR/.test(f.part)&&f.normal.z>.01||f.part==='bonnet'&&f.normal.y<-.01||['trunk','empennage'].includes(f.part)&&f.normal.y<-.01&&f.normal.x<.2)continue;const uvs=f.ids.map(map);if(Math.max(...uvs.map(p=>p[0]))<0||Math.min(...uvs.map(p=>p[0]))>1||Math.max(...uvs.map(p=>p[1]))<0||Math.min(...uvs.map(p=>p[1]))>1)continue;const key=f.mesh.name+':'+f.part+':'+f.region;let g=groups.get(key);if(!g)groups.set(key,g={mesh:f.mesh.name,part:f.part,region:f.region,faces:[],xy:[],uv:[]});g.faces.push(f.index);for(let j=0;j<3;j++){g.xy.push(...SU7Paint.projection(this.vertices[f.ids[j]].p,f.region));g.uv.push(...uvs[j]);}count++;if(count>SU7Paint.wrapLimits.maxFaces)throw Error('跨面素材覆盖范围过大，请缩小素材或分成多个图层');}
    const round=n=>Math.round(n*1e7)/1e7;return this.withGeometry({...spec,version:SU7Paint.wrapLimits.version,coverageVersion:SU7Paint.wrapLimits.coverageVersion,patches:[...groups.values()].map(g=>({...g,xy:g.xy.map(round),uv:g.uv.map(round)}))});
  }
  normal(mesh,face,point){const f=this.faces[this.byMesh.get(mesh)?.[face]];if(!f)return [0,1,0];const [a,b,c]=f.ids.map(i=>this.vertices[i].p),p=this.vec(point),v0=b.clone().sub(a),v1=c.clone().sub(a),v2=p.clone().sub(a),d00=v0.dot(v0),d01=v0.dot(v1),d11=v1.dot(v1),d20=v2.dot(v0),d21=v2.dot(v1),den=d00*d11-d01*d01;const y=den?(d11*d20-d01*d21)/den:0,z=den?(d00*d21-d01*d20)/den:0;return this.vertices[f.ids[0]].n.clone().multiplyScalar(1-y-z).addScaledVector(this.vertices[f.ids[1]].n,y).addScaledVector(this.vertices[f.ids[2]].n,z).normalize().toArray();}
};
