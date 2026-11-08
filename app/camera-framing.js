/* Resolve artwork bounds in the current articulated vehicle pose. */
window.SU7CameraFraming={
  artwork(meshes,layer,wrap){
    if(!layer)return null;
    const min=[Infinity,Infinity,Infinity],max=[-Infinity,-Infinity,-Infinity],Paint=window.SU7Paint,transforms=new Map();
    const add=(mesh,face)=>{
      if(!transforms.has(mesh)){mesh.updateWorldMatrix(true,false);transforms.set(mesh,mesh.matrixWorld.clone().multiply(mesh.userData.studioBindWorld.clone().invert()));}
      for(const point of face.points){const p=point.clone().applyMatrix4(transforms.get(mesh)).toArray();for(let k=0;k<3;k++){min[k]=Math.min(min[k],p[k]);max[k]=Math.max(max[k],p[k]);}}
    };
    if(wrap){
      for(const patch of wrap.patches){const mesh=meshes.find(m=>m.name===patch.mesh);if(!mesh)continue;for(let i=0;i<patch.faces.length;i++){
        const uv=patch.uv.slice(i*6,i*6+6);if(Math.max(uv[0],uv[2],uv[4])<0||Math.min(uv[0],uv[2],uv[4])>1||Math.max(uv[1],uv[3],uv[5])<0||Math.min(uv[1],uv[3],uv[5])>1)continue;add(mesh,mesh.userData.studioFaces[patch.faces[i]]);
      }}
    }else{
      const angle=-layer.rotation*Math.PI/180,c=Math.cos(angle),s=Math.sin(angle);
      for(const mesh of meshes)for(const face of mesh.userData.studioFaces){
        if(layer.part&&face.part!==layer.part||!window.SU7PaintProjection.accepts(face,layer.region))continue;
        const uv=face.points.map(p=>{const [u,v]=Paint.projection(p,layer.region),x=u*layer.size[0]-layer.x,y=v*layer.size[1]-layer.y;return [.5+(x*c-y*s)/layer.w,.5+(x*s+y*c)/layer.h];});
        if(Math.max(...uv.map(p=>p[0]))<0||Math.min(...uv.map(p=>p[0]))>1||Math.max(...uv.map(p=>p[1]))<0||Math.min(...uv.map(p=>p[1]))>1)continue;add(mesh,face);
      }
    }
    return min.every(Number.isFinite)?{min,max}:null;
  }
};
