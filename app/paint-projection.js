/* Ordinary 2D artwork projected onto every facing painted triangle.
 * Bind-pose coordinates keep projection stable when a door or lid opens.
 * The five transparent views share one atlas and one overlay per body mesh.
 */
window.SU7PaintProjection = (() => {
  'use strict';
  const Paint=window.SU7Paint,regions=['left','right','top','front','rear'],epsilon=.015;
  function normal(face){const [a,b,c]=face.points;return b.clone().sub(a).cross(c.clone().sub(a)).normalize();}
  function facing(n,region){return region==='left'?n.z:region==='right'?-n.z:region==='top'?n.y:region==='front'?-n.x:n.x;}
  function accepts(face,region){return !!face&&facing(normal(face),region)>=epsilon;}

  function geometry(mesh,part,region){
    const original=mesh.userData.studioOriginalGeometry,A=original.attributes.position.constructor;
    const faces=mesh.userData.studioFaces;
    if(part){
      const g=new original.constructor(),position=[],normals=[],uv=[],source=original.attributes,index=original.index;
      for(let f=0;f<faces.length;f++){
        if(faces[f].part!==part||!accepts(faces[f],region))continue;
        for(let j=0;j<3;j++){
          const i=index?index.getX(f*3+j):f*3+j,n=source.normal;
          position.push(source.position.getX(i)+n.getX(i)*.0003,source.position.getY(i)+n.getY(i)*.0003,source.position.getZ(i)+n.getZ(i)*.0003);
          normals.push(n.getX(i),n.getY(i),n.getZ(i));uv.push(...Paint.projection(faces[f].points[j],region));
        }
      }
      if(!position.length){g.dispose();return null;}
      g.setAttribute('position',new A(new Float32Array(position),3));g.setAttribute('normal',new A(new Float32Array(normals),3));g.setAttribute('uv2',new A(new Float32Array(uv),2));
      return g;
    }
    const g=original.index?original.toNonIndexed():original.clone(),positions=g.attributes.position,normals=g.attributes.normal;
    const bind=new Float32Array(positions.count*3),directions=new Float32Array(positions.count*3),uv=new Float32Array(positions.count*2);
    faces.forEach((face,f)=>{
      const n=normal(face),tile=regions.indexOf(face.region);
      face.points.forEach((p,j)=>{
        const i=f*3+j,[u,v]=Paint.projection(p,face.region);
        bind.set(p.toArray(),i*3);directions.set(n.toArray(),i*3);
        // Retain the legacy atlas coordinates for guide/extraction consumers.
        uv.set([(tile%3+Math.max(.001,Math.min(.999,u)))/3,(Math.floor(tile/3)+Math.max(.001,Math.min(.999,v)))/2],i*2);
        positions.setXYZ(i,positions.getX(i)+normals.getX(i)*.0002,positions.getY(i)+normals.getY(i)*.0002,positions.getZ(i)+normals.getZ(i)*.0002);
      });
    });
    g.setAttribute('uv2',new A(uv,2));g.setAttribute('studioPaintPosition',new A(bind,3));g.setAttribute('studioPaintNormal',new A(directions,3));return g;
  }

  const declarations=`
    varying vec3 studioPaintPoint;
    varying vec3 studioPaintDirection;
    uniform vec3 studioPaintBase;
    uniform float studioPaintBaseAlpha;
    uniform vec2 studioPaintTexel;
    uniform float studioPaintViews[5];
    vec4 studioPaintOver(vec4 below, vec4 above) {
      float alpha = above.a + below.a * (1.0 - above.a);
      return vec4((above.rgb * above.a + below.rgb * below.a * (1.0 - above.a)) / max(alpha, 0.000001), alpha);
    }
    vec4 studioPaintSample(vec2 uv, vec2 tile) {
      if (any(lessThan(uv, vec2(0.0))) || any(greaterThan(uv, vec2(1.0)))) return vec4(0.0);
      vec2 inset = studioPaintTexel * vec2(3.0, 2.0) * 0.5;
      return texture2D(map, (tile + clamp(uv, inset, vec2(1.0) - inset)) / vec2(3.0, 2.0));
    }
  `;
  const fragment=`
    vec3 p = studioPaintPoint;
    vec3 n = studioPaintDirection;
    vec4 paint = vec4(studioPaintBase, studioPaintBaseAlpha);
    if (studioPaintViews[0] > 0.5 && n.z >= 0.015) paint = studioPaintOver(paint, studioPaintSample(vec2((p.x+2.6)/5.2, (1.55-p.y)/1.55), vec2(0.0,0.0)));
    if (studioPaintViews[1] > 0.5 && n.z <= -0.015) paint = studioPaintOver(paint, studioPaintSample(vec2((2.6-p.x)/5.2, (1.55-p.y)/1.55), vec2(1.0,0.0)));
    if (studioPaintViews[2] > 0.5 && n.y >= 0.015) paint = studioPaintOver(paint, studioPaintSample(vec2((p.x+2.6)/5.2, (p.z+1.05)/2.1), vec2(2.0,0.0)));
    if (studioPaintViews[3] > 0.5 && n.x <= -0.015) paint = studioPaintOver(paint, studioPaintSample(vec2((p.z+1.05)/2.1, (1.55-p.y)/1.55), vec2(0.0,1.0)));
    if (studioPaintViews[4] > 0.5 && n.x >= 0.015) paint = studioPaintOver(paint, studioPaintSample(vec2((1.05-p.z)/2.1, (1.55-p.y)/1.55), vec2(1.0,1.0)));
    diffuseColor *= paint;
  `;
  function atlasMaterial(material,state){
    material.onBeforeCompile=shader=>{
      Object.assign(shader.uniforms,{studioPaintBase:state.color,studioPaintBaseAlpha:state.alpha,studioPaintTexel:state.texel,studioPaintViews:state.views});
      shader.vertexShader='attribute vec3 studioPaintPosition;\nattribute vec3 studioPaintNormal;\nvarying vec3 studioPaintPoint;\nvarying vec3 studioPaintDirection;\n'+shader.vertexShader.replace('#include <begin_vertex>','#include <begin_vertex>\nstudioPaintPoint = studioPaintPosition; studioPaintDirection = studioPaintNormal;');
      // Insert after map declarations because the projection sampler uses map.
      shader.fragmentShader=shader.fragmentShader.replace('#include <map_pars_fragment>','#include <map_pars_fragment>\n'+declarations).replace('#include <map_fragment>',fragment);
    };
    material.customProgramCacheKey=()=> 'su7-facing-paint-atlas-v1';
  }
  function partMaterial(material){
    material.onBeforeCompile=shader=>{shader.fragmentShader=shader.fragmentShader.replace('#include <map_fragment>',`#include <map_fragment>
      #ifdef USE_MAP
      if(any(lessThan(vMapUv,vec2(0.0)))||any(greaterThan(vMapUv,vec2(1.0)))) discard;
      #endif`);};
    material.customProgramCacheKey=()=> 'su7-facing-part-paint-v1';
  }
  return {geometry,atlasMaterial,partMaterial,accepts,regions};
})();
