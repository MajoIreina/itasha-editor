// Batched live preview for continuous curved decals. Geometry and UVs are used
// unchanged. Production exports keep the independent Canvas/SVG drawing path.
window.SU7WrapRasterizer=(()=>{
  let canvas,gl,program,buffer,maskTexture,maskState,failed=false,draws=0;
  const textures=new Map(),geometry=new WeakMap();let textureBytes=0;
  const vertex=`#version 300 es
    layout(location=0) in vec2 position;
    layout(location=1) in vec2 uv;
    layout(location=2) in float mask;
    uniform vec2 size,viewport;
    uniform vec3 rowX,rowY;
    out vec2 texUV;flat out int maskLayer;
    void main(){vec3 p=vec3(position*size,1.0);vec2 q=vec2(dot(rowX,p),dot(rowY,p));
      gl_Position=vec4(q.x/viewport.x*2.0-1.0,1.0-q.y/viewport.y*2.0,0.0,1.0);
      texUV=uv;maskLayer=int(mask);}`;
  const fragment=`#version 300 es
    precision highp float;precision highp sampler2DArray;
    uniform sampler2D image;uniform sampler2DArray masks;uniform vec2 viewport;
    in vec2 texUV;flat in int maskLayer;out vec4 color;
    void main(){if(any(lessThan(texUV,vec2(0.0)))||any(greaterThan(texUV,vec2(1.0))))discard;
      float a=maskLayer<0?1.0:texture(masks,vec3(gl_FragCoord.x/viewport.x,1.0-gl_FragCoord.y/viewport.y,float(maskLayer))).r;
      color=texture(image,texUV)*a;}`;
  function init(){
    if(failed)return false;if(gl)return true;
    if(window.itashaHost?.gpuAtStartup===false)return false;
    canvas=document.createElement('canvas');
    gl=canvas.getContext('webgl2',{alpha:true,antialias:false,depth:false,stencil:false,premultipliedAlpha:true,preserveDrawingBuffer:true,failIfMajorPerformanceCaveat:true});
    if(!gl){failed=true;return false;}
    const current=gl;canvas.addEventListener('webglcontextlost',()=>{if(gl===current)failed=true;});
    const shader=(kind,source)=>{const s=gl.createShader(kind);gl.shaderSource(s,source);gl.compileShader(s);if(!gl.getShaderParameter(s,gl.COMPILE_STATUS))throw Error(gl.getShaderInfoLog(s));return s;};
    program=gl.createProgram();const vs=shader(gl.VERTEX_SHADER,vertex),fs=shader(gl.FRAGMENT_SHADER,fragment);gl.attachShader(program,vs);gl.attachShader(program,fs);gl.linkProgram(program);gl.deleteShader(vs);gl.deleteShader(fs);
    if(!gl.getProgramParameter(program,gl.LINK_STATUS))throw Error(gl.getProgramInfoLog(program));
    buffer=gl.createBuffer();gl.useProgram(program);gl.bindBuffer(gl.ARRAY_BUFFER,buffer);
    for(const [index,count,offset] of [[0,2,0],[1,2,8],[2,1,16]]){gl.enableVertexAttribArray(index);gl.vertexAttribPointer(index,count,gl.FLOAT,false,20,offset);}
    gl.uniform1i(gl.getUniformLocation(program,'image'),0);gl.uniform1i(gl.getUniformLocation(program,'masks'),1);
    gl.enable(gl.BLEND);gl.blendFunc(gl.ONE,gl.ONE_MINUS_SRC_ALPHA);gl.pixelStorei(gl.UNPACK_ALIGNMENT,1);return true;
  }
  function parameters(target){gl.texParameteri(target,gl.TEXTURE_MIN_FILTER,gl.LINEAR);gl.texParameteri(target,gl.TEXTURE_MAG_FILTER,gl.LINEAR);gl.texParameteri(target,gl.TEXTURE_WRAP_S,gl.CLAMP_TO_EDGE);gl.texParameteri(target,gl.TEXTURE_WRAP_T,gl.CLAMP_TO_EDGE);}
  function source(im){
    gl.activeTexture(gl.TEXTURE0);let entry=textures.get(im);
    if(entry){textures.delete(im);textures.set(im,entry);gl.bindTexture(gl.TEXTURE_2D,entry.texture);return;}
    const bytes=im.width*im.height*4;
    while(textures.size&&(textures.size>=4||textureBytes+bytes>64*1048576)){const [old,value]=textures.entries().next().value;gl.deleteTexture(value.texture);textureBytes-=value.bytes;textures.delete(old);}
    const texture=gl.createTexture();gl.bindTexture(gl.TEXTURE_2D,texture);parameters(gl.TEXTURE_2D);gl.pixelStorei(gl.UNPACK_PREMULTIPLY_ALPHA_WEBGL,true);gl.texImage2D(gl.TEXTURE_2D,0,gl.RGBA,gl.RGBA,gl.UNSIGNED_BYTE,im);gl.pixelStorei(gl.UNPACK_PREMULTIPLY_ALPHA_WEBGL,false);
    textures.set(im,{texture,bytes});textureBytes+=bytes;
  }
  function masks(paths,r,m,w,h){
    const keys=Object.keys(paths).filter(k=>k.startsWith(r+':')).sort(),signature=[r,w,h,m.a,m.b,m.c,m.d,m.e,m.f].join(':');
    gl.activeTexture(gl.TEXTURE1);
    if(maskState?.paths===paths&&maskState.signature===signature){gl.bindTexture(gl.TEXTURE_2D_ARRAY,maskTexture);return maskState.keys;}
    if(maskTexture)gl.deleteTexture(maskTexture);maskTexture=gl.createTexture();gl.bindTexture(gl.TEXTURE_2D_ARRAY,maskTexture);parameters(gl.TEXTURE_2D_ARRAY);
    gl.texStorage3D(gl.TEXTURE_2D_ARRAY,1,gl.R8,w,h,Math.max(1,keys.length));
    const c=document.createElement('canvas');c.width=w;c.height=h;const x=c.getContext('2d',{willReadFrequently:true}),alpha=new Uint8Array(w*h);
    for(let i=0;i<keys.length;i++){x.resetTransform();x.clearRect(0,0,w,h);x.setTransform(m);x.fillStyle='#fff';x.fill(paths[keys[i]],'evenodd');const rgba=x.getImageData(0,0,w,h).data;for(let j=0;j<alpha.length;j++)alpha[j]=rgba[j*4+3];gl.texSubImage3D(gl.TEXTURE_2D_ARRAY,0,0,0,i,w,h,1,gl.RED,gl.UNSIGNED_BYTE,alpha);}
    c.width=c.height=1;maskState={paths,signature,keys};return keys;
  }
  function triangles(wrap,r,part,keys){
    let cache=geometry.get(wrap);if(!cache)geometry.set(wrap,cache=new Map());const key=[r,part,wrap.patches.every(p=>p.xyz?.length===p.faces.length*9),...keys].join(':');if(cache.has(key))return cache.get(key);
    const list=[];for(const p of SU7WrapDrawing.patches(wrap,r,part))for(let i=0;i<p.faces.length;i++)list.push({p,i,depth:p.depth?.[i]??0});list.sort((a,b)=>a.depth-b.depth);
    const values=new Float32Array(list.length*15);let offset=0;
    for(const {p,i} of list){const layer=keys.indexOf(r+':'+p.part);for(let j=0;j<3;j++){const k=i*6+j*2;values[offset++]=p.xy[k];values[offset++]=p.xy[k+1];values[offset++]=p.uv[k];values[offset++]=p.uv[k+1];values[offset++]=layer;}}
    cache.set(key,values);return values;
  }
  function draw(destination,wrap,r,part,im,size,opacity,paths){
    const w=destination.canvas.width,h=destination.canvas.height;if(w*h>4000000||w>4096||h>4096||im.width*im.height*4>64*1048576)return false;
    try{
      if(!init()||gl.isContextLost())return false;
      if(canvas.width!==w||canvas.height!==h){canvas.width=w;canvas.height=h;}gl.viewport(0,0,w,h);gl.clearColor(0,0,0,0);gl.clear(gl.COLOR_BUFFER_BIT);gl.useProgram(program);
      const m=destination.getTransform(),keys=masks(paths,r,m,w,h),vertices=triangles(wrap,r,part,keys);source(im);
      gl.uniform2f(gl.getUniformLocation(program,'size'),...size);gl.uniform2f(gl.getUniformLocation(program,'viewport'),w,h);gl.uniform3f(gl.getUniformLocation(program,'rowX'),m.a,m.c,m.e);gl.uniform3f(gl.getUniformLocation(program,'rowY'),m.b,m.d,m.f);
      gl.bindBuffer(gl.ARRAY_BUFFER,buffer);gl.bufferData(gl.ARRAY_BUFFER,vertices,gl.DYNAMIC_DRAW);gl.drawArrays(gl.TRIANGLES,0,vertices.length/5);
      if(gl.getError()!==gl.NO_ERROR)throw Error('Curved preview upload failed');
      destination.save();destination.resetTransform();destination.globalAlpha=opacity;destination.drawImage(canvas,0,0);destination.restore();draws++;return true;
    }catch(error){failed=true;return false;}
  }
  function release(){if(gl){for(const entry of textures.values())gl.deleteTexture(entry.texture);if(maskTexture)gl.deleteTexture(maskTexture);gl.deleteBuffer(buffer);gl.deleteProgram(program);gl.getExtension('WEBGL_lose_context')?.loseContext();}textures.clear();textureBytes=0;maskState=null;gl=null;canvas=null;failed=false;}
  return {draw,release,get info(){return {available:!!gl&&!failed,draws,textureBytes};}};
})();
