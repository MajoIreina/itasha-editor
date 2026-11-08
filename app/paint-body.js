/* Exterior paint geometry. Shared by the viewport and offline guide generator. */
window.SU7Paint = (() => {
  const parts={doorLF:{label:'左前门',region:'left'},doorLB:{label:'左后门',region:'left'},doorRF:{label:'右前门',region:'right'},doorRB:{label:'右后门',region:'right'},frontBumper:{label:'前保险杠',region:'front'},rearBumper:{label:'后保险杠',region:'rear'},bonnet:{label:'前备箱盖',region:'top'},trunk:{label:'后备箱盖',region:'top'},empennage:{label:'尾翼',region:'top'},body:{label:'其余裸露油漆车体',region:null}};
  const names={carPaint_external_10:'frontBumper',carPaint_external_4:'doorLF',carPaint_external_5:'doorLB',carPaint_external_6:'doorRF',carPaint_external_7:'doorRB',carPaint_external_3_1:'bonnet',carPaint_external_8:'trunk',carPaint_external_2:'empennage',carPaint_internal_23:'body'};
  function projection(p,r){return r==='left'?[(p.x+2.6)/5.2,(1.55-p.y)/1.55]:r==='right'?[(2.6-p.x)/5.2,(1.55-p.y)/1.55]:r==='top'?[(p.x+2.6)/5.2,(p.z+1.05)/2.1]:r==='front'?[(p.z+1.05)/2.1,(1.55-p.y)/1.55]:[(1.05-p.z)/2.1,(1.55-p.y)/1.55];}
  function region(n){return Math.abs(n.y)>=Math.max(Math.abs(n.z),Math.abs(n.x))?'top':Math.abs(n.z)>=Math.abs(n.x)?n.z>0?'left':'right':n.x<0?'front':'rear';}
  function prepare(mesh,V){
    if(mesh.userData.studioFaces)return mesh.userData.studioFaces;
    const g=mesh.geometry,a=g.attributes.position,ix=g.index,n=ix?.count||a.count,faces=[];
    if(!names[mesh.name]&&mesh.name!=='carPaint_internal_1')return null;
    // The rear bumper is a disconnected component of the factory shell mesh.
    // Weld coincident vertices before finding components; do not cut by a box.
    let components;
    if(mesh.name==='carPaint_internal_1'){
      const parent=Array.from({length:a.count},(_,i)=>i),weld=new Map();
      const find=i=>parent[i]===i?i:(parent[i]=find(parent[i]));const join=(a,b)=>parent[find(a)]=find(b);
      for(let i=0;i<a.count;i++){const k=[a.getX(i),a.getY(i),a.getZ(i)].map(v=>Math.round(v*10000)).join();if(weld.has(k))join(i,weld.get(k));else weld.set(k,i);}
      for(let i=0;i<n;i+=3){const ids=[0,1,2].map(j=>ix?ix.getX(i+j):i+j);join(ids[0],ids[1]);join(ids[0],ids[2]);}
      const bounds=new Map();for(let i=0;i<a.count;i++){const root=find(i),p=mesh.localToWorld(new V(a.getX(i),a.getY(i),a.getZ(i)));let b=bounds.get(root);if(!b)bounds.set(root,b={minX:9,maxY:-9,minZ:9,maxZ:-9});b.minX=Math.min(b.minX,p.x);b.maxY=Math.max(b.maxY,p.y);b.minZ=Math.min(b.minZ,p.z);b.maxZ=Math.max(b.maxZ,p.z);}
      components=Array.from({length:a.count},(_,i)=>{const b=bounds.get(find(i));return b.minX>1.6&&b.maxY<.83&&b.maxZ-b.minZ>1.5?'rearBumper':'body';});
    }
    for(let i=0;i<n;i+=3){const ids=[0,1,2].map(j=>ix?ix.getX(i+j):i+j),p=ids.map(k=>mesh.localToWorld(new V(a.getX(k),a.getY(k),a.getZ(k))));const normal=p[1].clone().sub(p[0]).cross(p[2].clone().sub(p[0])).normalize(),part=components?components[ids[0]]:names[mesh.name];faces.push({part,region:region(normal),points:p});}
    mesh.userData.studioFaces=faces;return faces;
  }
  function ignoreGuideOccluder(mesh){return /后视镜/.test(mesh.material?.name||'')||/^car(?:RearviewMirror|OsrvmEcMirror|Mirror_(?:top|bottom)|Plate_bgs_)/i.test(mesh.name);}
  return {parts,projection,region,prepare,ignoreGuideOccluder};
})();
