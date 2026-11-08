/* Adapter for the locally archived renderer. */
(async() => {
  'use strict';
  if(!window.SU7Paint)await new Promise((resolve,reject)=>{const s=document.createElement('script');s.src='/__studio/paint-body.js';s.onload=resolve;s.onerror=reject;document.head.append(s);});
  const Paint=window.SU7Paint;
  await new Promise((resolve,reject)=>{const s=document.createElement('script');s.src='/__studio/paint-projection.js';s.onload=resolve;s.onerror=reject;document.head.append(s);});
  const Projection=window.SU7PaintProjection;
  await new Promise((resolve,reject)=>{const s=document.createElement('script');s.src='/__studio/surface-wrap.js';s.onload=resolve;s.onerror=reject;document.head.append(s);});
  await new Promise((resolve,reject)=>{const s=document.createElement('script');s.src='/__studio/lighting.js';s.onload=resolve;s.onerror=reject;document.head.append(s);});
  for(const file of ['camera-navigation.js','camera-input.js','camera-framing.js'])await new Promise((resolve,reject)=>{const s=document.createElement('script');s.src='/__studio/'+file;s.onload=resolve;s.onerror=reject;document.head.append(s);});
  const embedded = window.parent !== window;
  if(embedded){
    document.addEventListener('dragover',e=>{e.preventDefault();parent.postMessage({type:'su7-drag-enter'},location.origin);});
    document.addEventListener('dragleave',()=>parent.postMessage({type:'su7-drag-leave'},location.origin));
    document.addEventListener('drop',e=>{e.preventDefault();e.stopPropagation();parent.postMessage({type:'su7-import-files',hit:paintHit(e.clientX,e.clientY),assetId:e.dataTransfer.getData('application/x-su7-asset'),layerId:e.dataTransfer.getData('application/x-su7-layer'),files:[...e.dataTransfer.files],text:e.dataTransfer.getData('text/uri-list')||e.dataTransfer.getData('text/plain')},location.origin);},true);
  }
  const qualityPresets={low:{tile:512,pixelRatio:.75,samples:0},medium:{tile:1024,pixelRatio:1,samples:2},high:{tile:2048,pixelRatio:2,samples:4},ultra:{tile:4096,pixelRatio:3,samples:4},extreme:{tile:8192,pixelRatio:4,samples:8}};
  let quality=(navigator.deviceMemory&&navigator.deviceMemory<=4)?'medium':'high';
  try{quality=JSON.parse(localStorage.getItem('su7-studio-settings')||'{}').quality||quality;}catch{}
  if(!qualityPresets[quality])quality='medium';
  let renderRatio=qualityPresets[quality].pixelRatio;
  let graphics={precision:qualityPresets[quality].tile,resolution:renderRatio,aa:'msaa'+qualityPresets[quality].samples},actualSamples=0;
  try{Object.assign(graphics,JSON.parse(localStorage.getItem('su7-studio-settings')||'{}').graphics||{});}catch{}
  renderRatio=Number(graphics.resolution)||renderRatio;
  if(embedded){try{Object.defineProperty(window,'devicePixelRatio',{configurable:true,get:()=>renderRatio});}catch{}
    const getContext=HTMLCanvasElement.prototype.getContext;
    HTMLCanvasElement.prototype.getContext=function(type,options){return getContext.call(this,type,/^webgl/.test(type)?{...options,antialias:false,powerPreference:quality==='low'?'low-power':'default'}:options);};
  }
  const regions = Projection.regions;
  let lightingEngine,lightingValue={mode:'studio',hour:12,brightness:1,temperature:6500};
  let runtime,engine, meshes = [], prepared = false, tiles = {}, atlas, texture, base = '#00659e', enabled = false,factoryPaint=true,renderCount=0;
  let report = [], readySent = false, last = 0;
  const partMaterials=new Map(),partTextures=new Map();
  let projectionState;
  let optionsHidden=false,configTabName='车身',paintSettleUntil=0;
  document.addEventListener('click',e=>{const text=e.target.textContent?.trim();if(['车身','车轮','内饰','选装'].includes(text)&&e.target.getBoundingClientRect().x>innerWidth*.5)configTabName=text;const card=e.target.closest('[data-track-params]');if(card){try{if(JSON.parse(card.getAttribute('data-track-params')).nr_car_group_tag==='车漆')paintSettleUntil=performance.now()+2200;}catch{}}},true);
  let displayCamera,navigation,cameraInput,cameraReportQueued=false;
  let editingStage=false,editingCameraState=null;
  let artworkTool=false,artworkDrag=false,artworkMoveQueued=false,artworkMoveEvent;
  let interactionQuality=false;
  let wrapGraph,artworkLayers=[],chosenArtwork=null,selectionVisible=true;const wrappedLayers=new Map(),wrappedTextureCache=new Map(),alphaImages=new Map();
  function surfaceGraph(){return wrapGraph||=(new SU7SurfaceWrap(meshes,runtime.Vector3));}
  function sceneHit(x,y){
    if(!prepared||!engine||!navigation)return null;
    const ray=new runtime.Raycaster(),rect=runtime.renderer.domElement.getBoundingClientRect();
    fitCamera(engine.camera);ray.setFromCamera({x:(x-rect.left)/rect.width*2-1,y:-(y-rect.top)/rect.height*2+1},displayCamera);
    return ray.intersectObjects(engine._resourceManager.getModelGroup().children,true).find(h=>{for(let p=h.object;p;p=p.parent)if(!p.visible)return false;return !h.object.name.startsWith('studio_')&&!/decal/i.test(h.object.name)&&h.object.material?.visible!==false;})||null;
  }
  function paintHit(x,y){
    const hit=sceneHit(x,y);
    if(!hit)return null;const face=hit.object.userData.studioFaces?.[hit.faceIndex];if(!face)return null;
    // Use bind-pose coordinates for an opened door or lid as well.
    const local=hit.object.worldToLocal(hit.point.clone()),original=hit.object.userData.studioBindWorld;
    if(original)local.applyMatrix4(original);else hit.object.localToWorld(local);
    const uv=Paint.projection(local,face.region),point=local.toArray();return {part:face.part,region:face.region,u:uv[0],v:uv[1],mesh:hit.object.name,face:hit.faceIndex,point,normal:surfaceGraph().normal(hit.object.name,hit.faceIndex,point)};
  }
  function alphaAt(id,u,v){const data=alphaImages.get(id);return !data||data.pixels[(Math.min(data.h-1,Math.floor(v*data.h))*data.w+Math.min(data.w-1,Math.floor(u*data.w)))*4+3]>12;}
  function pickArtwork(hit){if(!hit)return null;for(const l of [...artworkLayers].reverse()){if(!l.visible||l.opacity<.01)continue;let uv;if(l.wrap){const g=wrappedLayers.get(l.id)?.wrap?.patches.find(p=>p.mesh===hit.mesh&&p.faces.includes(hit.face));if(g){const i=g.faces.indexOf(hit.face),face=meshes.find(m=>m.name===hit.mesh).userData.studioFaces[hit.face],V=runtime.Vector3,p=new V(...hit.point),[a,b,c]=face.points,v0=b.clone().sub(a),v1=c.clone().sub(a),v2=p.sub(a),d00=v0.dot(v0),d01=v0.dot(v1),d11=v1.dot(v1),d20=v2.dot(v0),d21=v2.dot(v1),den=d00*d11-d01*d01,y=den?(d11*d20-d01*d21)/den:0,z=den?(d00*d21-d01*d20)/den:0;uv=[0,1].map(j=>(1-y-z)*g.uv[i*6+j]+y*g.uv[i*6+2+j]+z*g.uv[i*6+4+j]);}}else if((!l.part||l.part===hit.part)&&Projection.accepts(meshes.find(m=>m.name===hit.mesh).userData.studioFaces[hit.face],l.region)){const [u,v]=Paint.projection({x:hit.point[0],y:hit.point[1],z:hit.point[2]},l.region),s=l.size,x=u*s[0]-l.x,y=v*s[1]-l.y,a=-l.rotation*Math.PI/180;uv=[.5+(x*Math.cos(a)-y*Math.sin(a))/l.w,.5+(x*Math.sin(a)+y*Math.cos(a))/l.h];}if(uv&&uv.every(v=>v>=0&&v<=1)&&alphaAt(l.id,...uv))return {id:l.id,uv,locked:l.locked};}return null;}
  const sendArtwork=(phase,e,extra={})=>parent.postMessage({type:'su7-artwork-drag',phase,hit:paintHit(e.clientX,e.clientY),...extra},location.origin);
  document.addEventListener('pointerdown',e=>{if(!artworkTool||e.altKey||e.shiftKey||!prepared||!document.body.classList.contains('studio-view')||e.target!==runtime.renderer.domElement||e.button!==0)return;e.preventDefault();e.stopImmediatePropagation();const pick=pickArtwork(paintHit(e.clientX,e.clientY));artworkDrag={pointer:e.pointerId,x:e.clientX,y:e.clientY,moving:false};e.target.setPointerCapture(e.pointerId);sendArtwork('start',e,{layerId:pick?.id,grab:pick?.uv,locked:pick?.locked});},true);
  document.addEventListener('pointermove',e=>{if(!artworkDrag||e.pointerId!==artworkDrag.pointer)return;e.preventDefault();e.stopImmediatePropagation();if(!artworkDrag.moving&&Math.hypot(e.clientX-artworkDrag.x,e.clientY-artworkDrag.y)<3)return;artworkDrag.moving=true;artworkMoveEvent={clientX:e.clientX,clientY:e.clientY};if(!artworkMoveQueued){artworkMoveQueued=true;requestAnimationFrame(()=>{artworkMoveQueued=false;if(artworkDrag&&artworkMoveEvent)sendArtwork('move',artworkMoveEvent);artworkMoveEvent=null;});}},true);
  for(const t of ['pointerup','pointercancel'])document.addEventListener(t,e=>{if(!artworkDrag||e.pointerId!==artworkDrag.pointer)return;const moving=artworkDrag.moving;artworkDrag=false;e.preventDefault();e.stopImmediatePropagation();sendArtwork(t==='pointercancel'?'cancel':'end',e,{moving});},true);
  document.addEventListener('keydown',e=>{if(e.key==='Escape'&&artworkDrag){artworkDrag=false;parent.postMessage({type:'su7-artwork-drag',phase:'cancel'},location.origin);}});
  function cancelArtworkGesture(){if(!artworkDrag)return;artworkDrag=false;parent.postMessage({type:'su7-artwork-drag',phase:'cancel'},location.origin);}
  window.addEventListener('blur',cancelArtworkGesture);document.addEventListener('lostpointercapture',e=>{if(artworkDrag&&e.pointerId===artworkDrag.pointer)cancelArtworkGesture();},true);
  cameraInput=new SU7CameraInput({element:()=>runtime?.renderer.domElement,navigation:()=>navigation,active:()=>embedded&&prepared&&!artworkDrag&&document.body.classList.contains('studio-view'),anchor:(x,y)=>sceneHit(x,y)?.point.toArray(),onClick:e=>{if(editingStage)sendArtwork('select',e,{layerId:pickArtwork(paintHit(e.clientX,e.clientY))?.id});else pickPart(e.clientX,e.clientY);},onFocus:focusPoint,onFocusSelected:focusArtwork,onShortcut:e=>{if(editingStage)parent.postMessage({type:'su7-editor-shortcut',key:e.key,ctrlKey:e.ctrlKey,metaKey:e.metaKey,shiftKey:e.shiftKey},location.origin);}});
  const parts={doorLF:['左前门','DOORLF'],doorLB:['左后门','DOORLB'],doorRF:['右前门','DOORRF'],doorRB:['右后门','DOORRB'],bonnet:['前备箱','BONNET'],trunk:['后备箱','TRUNK'],empennage:['尾翼','EMPENNAGE']};
  let motionWatch;
  function keepAnimationAlive(){if(motionWatch)return;function tick(){motionWatch=null;if(!prepared)return;if(Object.values(partStates()).some(p=>p.moving)){engine.renderOnEventStart(true);motionWatch=setTimeout(tick,200);}else engine.renderOnEventEnd(200);}tick();}
  function partStates(){const list=engine?._resourceManager?.getAnimationsList()||{};return Object.fromEntries(Object.entries(parts).map(([key,[label]])=>[key,{label,available:!!list[key],open:list[key]?._clip?.animationState==='open',moving:!!list[key]?.isRunning(),time:list[key]?.time,duration:list[key]?.getClip().duration}]));}
  function togglePart(key){if(editingStage||!parts[key]||!partStates()[key].available)return false;const state=partStates()[key],action=state.open?'close':'open',config=engine._animationConfig.getAnimationSwitch()[(key+'_'+action).toUpperCase()];if(!config)return false;engine.animationControl({...config,startFrame:state.moving?Math.max(0,Math.min(1,state.time/state.duration)):config.startFrame},undefined,true);keepAnimationAlive();return true;}
  function pickPart(x,y){let node=sceneHit(x,y)?.object;while(node){if(parts[node.name]){togglePart(node.name);return;}node=node.parent;}}
  const paintKeys={600061067:'CapriBlue',600061064:'LavaRed',600061061:'MineralGray',600061068:'IndigoGreen',600061066:'RougePink',600061059:'FrozenSnowWhite',600061062:'MysteriousFog',600061063:'SamuraiBlack'};
  const nativeMaterials=new Map();let selectedPaint,lastPaintPoll=0;
  let forcedSelections=[];
  const defaultsReady=fetch('/__catalog').then(r=>r.json()).then(c=>{forcedSelections=c.defaults||[];});
  window.addEventListener('resize',()=>{setTimeout(()=>{applyRenderGraphics();redraw();},180);});
  let TILE = qualityPresets[quality].tile;
  function collect(rt) {
    runtime = rt;lightingEngine=new SU7Lighting(rt);lightingEngine.set(lightingValue);
    const element=rt.renderer.domElement;
    navigation=new SU7CameraNavigation({width:innerWidth,height:innerHeight,onChange:()=>{
      redraw();if(cameraReportQueued||!embedded)return;cameraReportQueued=true;requestAnimationFrame(()=>{cameraReportQueued=false;if(prepared)parent.postMessage({type:'su7-camera-change',state:navigation.state},location.origin);});
    }});
    cameraInput.prepare();
    if(embedded){const setPixelRatio=rt.renderer.setPixelRatio.bind(rt.renderer);rt.renderer.setPixelRatio=()=>setPixelRatio(renderRatio);rt.renderer.setPixelRatio(renderRatio);}
    TILE=Math.min(TILE,Math.max(256,Math.floor(rt.renderer.capabilities.maxTextureSize/768)*256));
    rt.scene.updateMatrixWorld(true);
    rt.scene.traverse(mesh => {
      if (!mesh.isMesh || !Paint.prepare(mesh,rt.Vector3)) return;
      mesh.userData.studioBindWorld=mesh.matrixWorld.clone();
      mesh.userData.studioOriginalGeometry=mesh.geometry;
      mesh.userData.studioSource=mesh.material;
      meshes.push(mesh);
    });
    if(embedded) {
      const style=document.createElement('style');
      style.textContent='body{overflow:hidden!important}body.studio-view .configurator-right-container{visibility:hidden!important}body [class*="control-btn-fade-in"],body p.text-light-200{visibility:hidden!important}body.studio-view [class*="configArea"],body.studio-view [class*="bottomBar"],body.studio-view [class*="bottomAction"],body.studio-view [class*="topBar"],body.studio-view [class*="switchMode"]{visibility:hidden!important}.studio-hide-options [data-studio-option]{display:none!important}';
      document.head.append(style);
      const logo=document.querySelector('svg[viewBox="0 0 102 31"]');
      if(logo)logo.parentElement.style.visibility='hidden';
      document.querySelectorAll('p').forEach(e=>{if(e.textContent.startsWith('受光线'))e.style.visibility='hidden';});
      hideOriginalUI(true);
    }
    prepared=true;
  }
  function hideOriginalUI(hide) {
    document.body.classList.toggle('studio-view',hide);
    const pane=document.querySelector('.configurator-right-container');
    if(pane)pane.style.setProperty('visibility',hide?'hidden':'visible','important');
    for(const el of document.querySelectorAll('div')) {
      const t=el.innerText;
      if(!t || t.length>2200 || !/车身\s*车轮\s*内饰\s*选装/.test(t))continue;
      const rect=el.getBoundingClientRect();
      if(rect.x>innerWidth*.5 && rect.height>innerHeight*.45) {
        el.style.setProperty('visibility',hide?'hidden':'visible','important');
        break;
      }
    }
    for(const el of document.querySelectorAll('a,button')) {
      if(/总览|下一步|小米汽车/.test(el.textContent))el.style.visibility=hide?'hidden':'';
    }
  }
  function preparePaint() {
    if(atlas)return;
    atlas=document.createElement('canvas');atlas.width=TILE*3;atlas.height=TILE*2;
    texture=new runtime.Texture(atlas);texture.colorSpace='srgb';texture.flipY=false;texture.needsUpdate=true;texture.channel=2;
    projectionState={color:{value:meshes[0].userData.studioSource.color.clone().set(base)},alpha:{value:factoryPaint?0:1},texel:{value:{x:1/atlas.width,y:1/atlas.height}},views:{value:new Float32Array(5).fill(1)}};
    meshes.forEach(mesh=>{
      installPaintMaterial(mesh,mesh.userData.studioSource,Projection.geometry(mesh));
    });
    enabled=true;
    updateAtlas();
  }
  function preparePart(key){
    if(partMaterials.has(key))return partMaterials.get(key);
    const [part,region]=key.split(':'),items=[];
    if(!Paint.parts[part]||!regions.includes(region))return items;
    for(const mesh of meshes){
      const geometry=Projection.geometry(mesh,part,region);if(!geometry)continue;
      const material=new runtime.PhysicalMaterial({color:0xffffff,transparent:true,alphaTest:.002,roughness:.4,metalness:0,clearcoat:.3,depthWrite:false,polygonOffset:true,polygonOffsetFactor:-4,polygonOffsetUnits:-4});material.name='SU7_Part_Artwork';material.envMap=mesh.userData.studioSource.envMap;Projection.partMaterial(material);
      const overlay=new runtime.Mesh(geometry,material);overlay.name='studio_part_'+key;overlay.renderOrder=11+regions.indexOf(region)*.01;overlay.visible=false;mesh.add(overlay);items.push({material,overlay});
    }
    partMaterials.set(key,items);return items;
  }
  function updateAtlas() {
    if(!atlas)return;
    const c=atlas.getContext('2d');c.clearRect(0,0,atlas.width,atlas.height);
    projectionState.color.value.set(base);projectionState.alpha.value=factoryPaint?0:1;projectionState.texel.value.x=1/atlas.width;projectionState.texel.value.y=1/atlas.height;
    regions.forEach((r,i)=>{if(tiles[r])c.drawImage(tiles[r],i%3*TILE,Math.floor(i/3)*TILE,TILE,TILE);});
    texture.needsUpdate=true;
    redraw();
  }
  let drawQueued=false;
  function redraw() {
    if(!engine||drawQueued)return;drawQueued=true;
    requestAnimationFrame(()=>{drawQueued=false;engine.requestFrames(1).catch(e=>{if(prepared)report.push(e.message);});});
  }
  function resolvePaint(){const paint=readConfiguration().map(v=>{try{return JSON.parse(v.params);}catch{return {};}}).find(p=>p.nr_car_group_tag==='车漆');selectedPaint=paint&&nativeMaterials.get('carBody_MT_'+paintKeys[paint.nr_car_sku_id]);if(selectedPaint)meshes.forEach(m=>{m.material=selectedPaint;m.userData.studioSource=selectedPaint;});}
  function fitCamera(camera){
    if(!displayCamera)displayCamera=camera.clone();
    displayCamera.copy(camera,false);navigation.viewport(innerWidth,innerHeight);return navigation.apply(displayCamera);
  }
  function focusPoint(x,y){const hit=sceneHit(x,y);if(!hit)return false;navigation.focus(hit.point.toArray(),.35);return true;}
  function focusArtwork(){const layer=artworkLayers.find(l=>l.id===chosenArtwork),bounds=SU7CameraFraming.artwork(meshes,layer,wrappedLayers.get(chosenArtwork)?.wrap);if(!bounds)return false;navigation.fit(bounds);return true;}
  function installPaintMaterial(mesh,source,geometry) {
    const mat=new runtime.PhysicalMaterial({color:0xffffff,map:texture,transparent:true,alphaTest:.002,roughness:.4,metalness:0,clearcoat:.3,depthWrite:false,polygonOffset:true,polygonOffsetFactor:-2,polygonOffsetUnits:-2});
    mat.name='SU7_Artwork';mat.envMap=source.envMap;mat.lightMap=source.lightMap;mat.lightMapIntensity=lightingEngine?.base(source)?.lightMapIntensity??source.lightMapIntensity;mat.aoMap=source.aoMap;mat.aoMapIntensity=source.aoMapIntensity;
    Projection.atlasMaterial(mat,projectionState);
    const overlay=new runtime.Mesh(geometry,mat);overlay.name='studio_overlay_'+mesh.name;overlay.isDecal=true;overlay.renderOrder=10;overlay.castShadow=false;overlay.receiveShadow=true;
    mesh.add(overlay);mesh.userData.studioOverlay=overlay;mesh.userData.studioMaterial=mat;
  }
  async function update(data) {
    if(!prepared)throw Error('模型尚未载入');
    const delay=paintSettleUntil-performance.now();if(delay>0)await new Promise(r=>setTimeout(r,delay));
    const previousBase=base,previousFactory=factoryPaint,initial=!atlas;
    preparePaint();base=data.base||base;factoryPaint=data.factoryPaint!==false;
    projectionState.views.value.set(regions.map(r=>!data.activeRegions||data.activeRegions.includes(r)?1:0));
    for(const mesh of meshes)mesh.userData.studioOverlay.visible=!factoryPaint||projectionState.views.value.some(Boolean);
    await Promise.all(Object.entries(data.tiles||{}).map(async([r,src])=>{
      if(!regions.includes(r))return;
      const img=new Image();img.src=src;await img.decode();tiles[r]=img;
    }));
    await Promise.all(Object.entries(data.partTiles||{}).map(async([key,item])=>{
      if(!item){for(const m of partMaterials.get(key)||[])m.overlay.visible=false;partTextures.get(key)?.dispose();partTextures.delete(key);return;}
      const mats=preparePart(key);if(!mats.length)return;
      const img=new Image();img.src=item.src;await img.decode();const t=new runtime.Texture(img);t.colorSpace='srgb';t.flipY=false;t.channel=2;t.anisotropy=runtime.renderer.capabilities.getMaxAnisotropy();const [x,y,w,h]=item.bounds;t.offset.set(-x/w,-y/h);t.repeat.set(1/w,1/h);t.needsUpdate=true;
      t.userData.studioBounds=item.bounds;
      const old=partTextures.get(key);partTextures.set(key,t);for(const m of mats){m.material.map=t;m.material.needsUpdate=true;m.overlay.visible=true;}old?.dispose();
    }));
    if(data.wrapLayers)await updateWrapLayers(data.wrapLayers);
      if(initial||previousBase!==base||previousFactory!==factoryPaint||Object.keys(data.tiles||{}).length)updateAtlas();else redraw();return {painted:meshes.length};
  }
  function releaseWrappedTexture(item){if(!item.texture)return;const cached=wrappedTextureCache.get(item.signature);if(cached&&--cached.refs<=0){cached.texture.dispose();wrappedTextureCache.delete(item.signature);}item.texture=null;}
  function disposeWrapped(item){for(const m of item.nodes){m.parent.remove(m);m.geometry.dispose();}item.material?.dispose();releaseWrappedTexture(item);}
  function renderWrapped(id,wrap){
    const item=wrappedLayers.get(id);if(!item||item.wrap===wrap)return;surfaceGraph().withGeometry(wrap);
    const previous=new Map(item.nodes.map(node=>[node.parent.name,node])),grouped=new Map();
    for(const patch of wrap.patches){let group=grouped.get(patch.mesh);if(!group)grouped.set(patch.mesh,group={faces:[],uv:[]});for(let i=0;i<patch.faces.length;i++){group.faces.push(patch.faces[i]);for(let j=0;j<6;j++)group.uv.push(patch.uv[i*6+j]);}}
    const nodes=[];
    for(const [name,group] of grouped){
      const mesh=meshes.find(m=>m.name===name);let node=previous.get(name);previous.delete(name);
      const oldFaces=node?.userData.studioSurfaceFaces;
      if(oldFaces&&oldFaces.length===group.faces.length&&oldFaces.every((f,i)=>f===group.faces[i])){
        // The body geometry did not change: upload only new decal coordinates.
        node.geometry.attributes.uv.array.set(group.uv);node.geometry.attributes.uv.needsUpdate=true;
      }else{
        if(node){node.parent.remove(node);node.geometry.dispose();}
        const original=mesh.userData.studioOriginalGeometry,idx=original.index,a=original.attributes.position,n=original.attributes.normal,position=[],normal=[];
        for(const f of group.faces)for(let j=0;j<3;j++){const i=idx?idx.getX(f*3+j):f*3+j;position.push(a.getX(i)+n.getX(i)*.00035,a.getY(i)+n.getY(i)*.00035,a.getZ(i)+n.getZ(i)*.00035);normal.push(n.getX(i),n.getY(i),n.getZ(i));}
        const geometry=new mesh.geometry.constructor(),A=mesh.geometry.attributes.position.constructor;
        for(const [key,values] of Object.entries({position,normal,uv:group.uv}))geometry.setAttribute(key,new A(new Float32Array(values),key==='uv'?2:3));
        node=new runtime.Mesh(geometry,item.material);node.name='studio_wrap_'+id;node.userData.studioLayerId=id;node.userData.studioSurfaceFaces=group.faces;node.frustumCulled=false;mesh.add(node);
      }
      node.renderOrder=20+item.order*.001;node.visible=item.visible!==false;nodes.push(node);
    }
    for(const node of previous.values()){node.parent.remove(node);node.geometry.dispose();}
    item.nodes=nodes;item.wrap=wrap;redraw();
  }
  function updateArtworkSelection(){let changed=false;for(const [id,item] of wrappedLayers){const value=selectionVisible&&editingStage&&chosenArtwork===id?1:0;if(item.selected.value!==value){item.selected.value=value;changed=true;}}if(changed)redraw();}
  async function updateWrapLayers(layers){
    const ids=new Set(layers.map(l=>l.id));for(const [id,item] of wrappedLayers)if(!ids.has(id)){disposeWrapped(item);wrappedLayers.delete(id);}
    for(const l of layers){
      let item=wrappedLayers.get(l.id);
      if(!item){
        const selected={value:0},material=new runtime.PhysicalMaterial({color:0xffffff,transparent:true,alphaTest:.002,roughness:.4,metalness:0,clearcoat:.3,depthWrite:false,polygonOffset:true,polygonOffsetFactor:-6,polygonOffsetUnits:-6});
        material.name='SU7_Continuous_Artwork';material.envMap=meshes[0].userData.studioSource.envMap;
        material.onBeforeCompile=shader=>{
          shader.uniforms.studioSelected=selected;
          shader.fragmentShader='uniform float studioSelected;\n'+shader.fragmentShader.replace('#include <map_fragment>',`#include <map_fragment>
            #ifdef USE_MAP
            if(any(lessThan(vMapUv,vec2(0.)))||any(greaterThan(vMapUv,vec2(1.)))) discard;
            if(studioSelected > 0.5) {
              float edge = min(min(vMapUv.x,1.0-vMapUv.x),min(vMapUv.y,1.0-vMapUv.y));
              float pixel = max(max(fwidth(vMapUv.x),fwidth(vMapUv.y)),0.00001);
              float outline = 1.0-smoothstep(pixel,pixel*2.5,edge);
              diffuseColor.rgb = mix(diffuseColor.rgb,vec3(0.18,0.62,1.0),outline);
              diffuseColor.a = max(diffuseColor.a,outline*0.95);
            }
            #endif`);
        };
        material.customProgramCacheKey=()=> 'su7-continuous-decal-selection-v2';item={nodes:[],material,selected};wrappedLayers.set(l.id,item);
      }
      item.order=l.order;item.visible=l.visible;item.material.opacity=l.opacity;for(const n of item.nodes)n.visible=l.visible;
      if(item.signature!==l.signature||!item.texture){
        let cached=wrappedTextureCache.get(l.signature);if(!cached){if(!l.src)throw Error('跨面贴图缓存已释放，请重新同步');const im=new Image();im.src=l.src;await im.decode();const texture=new runtime.Texture(im);texture.flipY=false;texture.colorSpace='srgb';texture.anisotropy=runtime.renderer.capabilities.getMaxAnisotropy();texture.needsUpdate=true;cached={texture,refs:0};wrappedTextureCache.set(l.signature,cached);}
        releaseWrappedTexture(item);cached.refs++;item.texture=cached.texture;item.material.map=item.texture;item.material.needsUpdate=true;item.signature=l.signature;
      }
      // Editor operations replace wrap values; do not serialize every triangle
      // twice just to determine whether the shared same-origin value changed.
      if(item.wrap!==l.wrap)renderWrapped(l.id,l.wrap);for(const n of item.nodes){n.visible=l.visible;n.renderOrder=20+l.order*.001;}
    }
    updateArtworkSelection();redraw();
  }
  function setArtworkLayers(list,selected){const rank=l=>l.wrap?2:l.part?1:0;artworkLayers=[...list].sort((a,b)=>rank(a)-rank(b)||(!a.wrap?regions.indexOf(a.region)-regions.indexOf(b.region):0));chosenArtwork=selected;const ids=new Set(list.map(l=>l.id));for(const id of alphaImages.keys())if(!ids.has(id))alphaImages.delete(id);for(const l of list){if(!l.thumbnail||alphaImages.get(l.id)?.src===l.thumbnail)continue;const im=new Image();im.onload=()=>{const c=document.createElement('canvas');c.width=64;c.height=Math.max(1,Math.round(64*im.height/im.width));const x=c.getContext('2d',{willReadFrequently:true});x.drawImage(im,0,0,c.width,c.height);alphaImages.set(l.id,{src:l.thumbnail,w:c.width,h:c.height,pixels:x.getImageData(0,0,c.width,c.height).data});};im.src=l.thumbnail;}updateArtworkSelection();}
  function makeWrap(hit,options){const graph=surfaceGraph(),previous=options.previous,frame=graph.frame(hit,options.region||hit.region,options.rotation||0,previous);return graph.make({mesh:hit.mesh,face:hit.face,point:hit.point,...frame,width:options.width,height:options.height,at:options.at||[.5,.5]});}
  function reframeWrap(wrap,width,height,rotation){const r=surfaceGraph().vec(wrap.right),n=surfaceGraph().vec(wrap.normal);r.applyAxisAngle(n,-rotation*Math.PI/180);return surfaceGraph().make({...wrap,width,height,right:r.toArray(),down:r.clone().cross(n).normalize().toArray()});}
  async function exportPaintMasks(){
    if(Object.values(partStates()).some(p=>p.open||p.moving))throw Error('请先收起全部互动部件');
    const T=runtime,renderer=new T.renderer.constructor({antialias:false,alpha:false,preserveDrawingBuffer:true}),scene=new T.Scene(),owned=[],mats=[];
    renderer.setPixelRatio(1);renderer.setClearColor(0,1);
    const ids=Object.keys(Paint.parts),vertex='attribute float paintId; varying float id; void main(){id=paintId;gl_Position=projectionMatrix*modelViewMatrix*vec4(position,1.);}',fragment='varying float id; void main(){gl_FragColor=vec4(id/255.,0.,0.,1.);}';
    engine._resourceManager.getModelGroup().updateMatrixWorld(true);
    engine._resourceManager.getModelGroup().traverse(m=>{
      if(!m.isMesh||m.name.startsWith('studio_')||/decal/i.test(m.name)||m.material?.opacity===0||Paint.ignoreGuideOccluder(m))return;for(let p=m;p;p=p.parent)if(!p.visible)return;
      const faces=m.userData.studioFaces;let geo=m.geometry;
      if(faces){geo=geo.index?geo.toNonIndexed():geo.clone();const A=geo.attributes.position.constructor;geo.setAttribute('paintId',new A(new Float32Array(faces.flatMap(f=>Array(3).fill(ids.indexOf(f.part)+1))),1));owned.push(geo);}
      const material=new T.ShaderMaterial({vertexShader:faces?vertex:'void main(){gl_Position=projectionMatrix*modelViewMatrix*vec4(position,1.);}',fragmentShader:faces?fragment:'void main(){gl_FragColor=vec4(0.,0.,0.,1.);}',side:m.material?.side||0});mats.push(material);
      const clone=new T.Mesh(geo,material);clone.matrixAutoUpdate=false;clone.matrix.copy(m.matrixWorld);scene.add(clone);
    });
    const result={ids,views:{}};
    try{for(const r of regions){
      const side=['left','right'].includes(r),top=r==='top',w=Math.min(renderer.capabilities.maxTextureSize,(side||top)?6400:3200),h=Math.round(w*(top?2.1/5.2:side?1.55/5.2:1.55/2.1)),cam=new T.OrthographicCamera(-(side||top?2.6:1.05),(side||top?2.6:1.05),(top?1.05:.775),-(top?1.05:.775),.1,50);
      const center=new T.Vector3(0,.775,0);cam.position.set(...(r==='left'?[0,.775,10]:r==='right'?[0,.775,-10]:top?[0,10,0]:r==='front'?[-10,.775,0]:[10,.775,0]));if(top){center.set(0,0,0);cam.up.set(0,0,-1);}cam.lookAt(center);cam.updateMatrixWorld(true);renderer.setSize(w,h,false);renderer.render(scene,cam);result.views[r]={width:w,height:h,png:renderer.domElement.toDataURL('image/png')};await new Promise(resolve=>setTimeout(resolve,0));
    }return result;}finally{owned.forEach(g=>g.dispose());mats.forEach(m=>m.dispose());renderer.dispose();renderer.forceContextLoss();delete window.__su7Scenes?.[scene.uuid];}
  }
  function guides(region) {
    preparePaint();
    const edges=new Map();
    const tile=regions.indexOf(region),col=tile%3,row=Math.floor(tile/3);
    meshes.forEach(m=>{const uv=m.userData.studioOverlay.geometry.attributes.uv2;for(let i=0;i<uv.count;i+=3){if(Math.floor(uv.getX(i)*3)!==col||Math.floor(uv.getY(i)*2)!==row)continue;const p=[0,1,2].map(j=>[Math.round((uv.getX(i+j)*3-col)*10000)/10000,Math.round((uv.getY(i+j)*2-row)*10000)/10000]);for(let j=0;j<3;j++){const a=p[j],b=p[(j+1)%3],sa=a.join(','),sb=b.join(',');if(sa===sb)continue;const key=sa<sb?sa+';'+sb:sb+';'+sa;const prior=edges.get(key);if(prior)prior.count++;else edges.set(key,{count:1,points:[a,b]});}}});
    return [...edges.values()].filter(e=>e.count===1).map(e=>e.points);
  }
  function view(name) {
    return navigation?.view(name)||false;
  }
  function configTab(label) {
    configTabName=label;
    const candidates=[...document.querySelectorAll('div,span,button')].filter(e=>e.textContent.trim()===label&&e.children.length===0);
    const node=candidates.find(e=>e.getBoundingClientRect().x>innerWidth*.5)||candidates[0];
    if(node)node.click();
  }
  function hideOptions(hidden=true) {
    hidden=true;
    optionsHidden=hidden;
    document.body.classList.toggle('studio-hide-options',hidden);
    document.querySelectorAll('div,span,button').forEach(e=>{
      if(e.children.length===0 && e.textContent.trim()==='选装') {
        const node=e;node.setAttribute('data-studio-option','');node.style.display=hidden?'none':'';
      }
    });
    if(hidden&&configTabName==='选装')configTab('车轮');
  }
  let observing=false;
  function hideSpeedMode(){document.querySelectorAll('div,span,a,button').forEach(e=>{if(e.children.length===0&&/2D\s*极速模式/i.test(e.textContent)){const n=e.closest('button,a')||e;n.hidden=true;n.style.setProperty('display','none','important');}});}
  const watchOptions=new MutationObserver(()=>{if(observing)return;observing=true;setTimeout(()=>{hideSpeedMode();if(optionsHidden)document.querySelectorAll('div,span,button').forEach(e=>{if(e.children.length===0&&e.textContent.trim()==='选装'){const n=e;if(n.style.display!=='none'){n.setAttribute('data-studio-option','');n.style.display='none';}}});observing=false;},80);});
  watchOptions.observe(document.documentElement,{subtree:true,childList:true});
  const selectedCard=e=>!!e.querySelector('[class*="configSelectWrapSelected"],[class*="UltraCheckedButton_checkedButton"],[class*="UltraCheckedButton_containedButton"]');
  function readConfiguration() {
    const result=[...document.querySelectorAll('[data-track-params]')].filter(selectedCard).map(e=>({params:e.getAttribute('data-track-params'),label:e.textContent.trim()}));
    const ids=new Set(result.map(v=>JSON.parse(v.params).nr_car_sku_id));return [...result,...forcedSelections.filter(v=>!ids.has(JSON.parse(v.params).nr_car_sku_id))];
  }
  async function selectConfiguration(id,select=true){const el=[...document.querySelectorAll('[data-track-params]')].find(e=>{try{return String(JSON.parse(e.getAttribute('data-track-params')).nr_car_sku_id)===String(id);}catch{return false;}});if(!el)return false;if(selectedCard(el)!==select){(el.querySelector('[class*="configSelectWrap"],[class*="UltraCheckedButton_uncheckedButton"],[class*="UltraCheckedButton_checkedButton"]')||el).click();await new Promise(r=>setTimeout(r,1300));}resolvePaint();redraw();return selectedCard(el)===select;}
  async function restoreConfiguration(config,factoryColor) {
    await defaultsReady;
    const ids=config.map(item=>{try{const p=JSON.parse(item.params);return String(p.nr_car_sku_id||'');}catch{return '';}}).filter(Boolean);
    if(!config.some(item=>{try{return JSON.parse(item.params).nr_car_group_tag==='车漆';}catch{return false;}})&&factoryColor){const pool=engine?._resourceManager?.getMaterialList()||{},Color=meshes[0].userData.studioSource.color.constructor;const fallback=Object.entries(paintKeys).find(([id,key])=>{const material=nativeMaterials.get('carBody_MT_'+key),definition=pool['carBody_MT_'+key],value=definition?.params?.color;return '#'+(material?material.color.getHexString():value!==undefined?new Color(value).getHexString():{'600061067':'00659e','600061064':'680d0d'}[id])===factoryColor;});if(fallback)ids.unshift(fallback[0]);}
    for(const id of ids)await selectConfiguration(id);
    configTab('车身');redraw();
  }
  async function extract() {
    preparePaint();const files=[],originals=[];
    for(const m of meshes){
      const g=m.userData.studioOriginalGeometry,uv=g.attributes.uv,idx=g.index;
      const o={name:m.name,material:m.userData.studioSource.name,uv:[],triangles:[],bindPositions:[]},position=g.attributes.position;
      for(let i=0;i<uv.count;i++){o.uv.push([uv.getX(i),uv.getY(i)]);o.bindPositions.push(new runtime.Vector3(position.getX(i),position.getY(i),position.getZ(i)).applyMatrix4(m.userData.studioBindWorld).toArray());}
      const count=idx?idx.count:g.attributes.position.count;
      for(let i=0;i<count;i+=3)o.triangles.push([0,1,2].map(j=>idx?idx.getX(i+j):i+j));originals.push(o);
      const d=o.triangles.map(tri=>'M'+tri.map(i=>o.uv[i].map((v,j)=>(j?1-v:v)*2048).map(v=>v.toFixed(2)).join(',')).join('L')+'Z').join('');
      files.push({name:o.name+'-original-UV.svg',data:`<svg xmlns="http://www.w3.org/2000/svg" width="2048" height="2048" viewBox="0 0 2048 2048"><rect width="2048" height="2048" fill="white"/><path d="${d}" fill="none" stroke="#262a30" stroke-width=".6"/></svg>`});
    }
    files.push({name:'original-UV.json',data:JSON.stringify(originals)});
    const source=meshes[0].userData.studioSource;
    files.push({name:'artwork-alpha-atlas.png',data:atlas.toDataURL('image/png'),base64:true});
    const composite=document.createElement('canvas');composite.width=atlas.width;composite.height=atlas.height;
    const dc=composite.getContext('2d');dc.fillStyle=factoryPaint?'#'+source.color.getHexString():base;dc.fillRect(0,0,composite.width,composite.height);dc.drawImage(atlas,0,0);
    const mapping=[];
    for(const [key,t] of partTextures){const [part,region]=key.split(':'),tile=regions.indexOf(region),b=t.userData.studioBounds;if(!b)continue;dc.drawImage(t.image,(tile%3+b[0])*TILE,(Math.floor(tile/3)+b[1])*TILE,b[2]*TILE,b[3]*TILE);files.push({name:'parts/'+part+'-'+region+'.png',data:t.image.src,base64:true});mapping.push({part,region,label:Paint.parts[part].label,bounds:b,width:t.image.width,height:t.image.height});}
    files.push({name:'parts/projection.json',data:JSON.stringify({parts:mapping,projection:'five-direction-facing-projection',calibrated:false},null,2)});
    files.push({name:'paint-projection.json',data:JSON.stringify({version:2,method:'five-direction-facing-projection',coordinates:'original-UV.json bindPositions',facingThreshold:.015,compositeOrder:regions,atlas:{columns:3,rows:2,regions},base:{mode:factoryPaint?'factory':'custom',color:base},projection:{left:['(x+2.6)/5.2','(1.55-y)/1.55'],right:['(2.6-x)/5.2','(1.55-y)/1.55'],top:['(x+2.6)/5.2','(z+1.05)/2.1'],front:['(z+1.05)/2.1','(1.55-y)/1.55'],rear:['(1.05-z)/2.1','(1.55-y)/1.55']}},null,2)});
    files.push({name:'editable-color-atlas.png',data:composite.toDataURL('image/png'),base64:true});
    files.push({name:'factory-paint.json',data:JSON.stringify({name:source.name,color:'#'+source.color.getHexString(),roughness:source.roughness,metalness:source.metalness,clearcoat:source.clearcoat,mode:factoryPaint?'factory':'custom',textureChannel:2,atlas:{width:atlas.width,height:atlas.height,columns:3,regions},note:'editable-color-atlas 为颜色参考；原厂金属漆外观由物理材质、光照和贴图共同生成。'},null,2)});
    for(const key of ['lightMap','aoMap','specularColorMap','_microflakeNormalMap','_flakeLightMap']) {
      const t=source[key];if(!t)continue;
      try {files.push({name:key+'.png',data:await texturePNG(t),base64:true});}catch(e){report.push(key+': '+e.message);}
    }
    files.push({name:'说明.txt',data:'车漆原本使用物理材质颜色，无现成漫反射彩色贴图。editable-color-atlas.png 为生成的可编辑颜色参考图；artwork-alpha-atlas.png 只含透明图案。五向贴图覆盖朝向该视图的全部漆面，曲面重叠处依次叠加，完整规则见 paint-projection.json。original-UV.json 保留原始 UV、三角形与绑定姿态顶点，不改变光照/AO UV。UV 与投影未经实车尺寸标定，不可直接作为裁切线。\n'+report.join('\n')});
    return files;
  }
  async function texturePNG(source) {
    const T=runtime,w=source.image.width,h=source.image.height;
    if(source.image instanceof HTMLImageElement || source.image instanceof HTMLCanvasElement || source.image instanceof ImageBitmap) {
      const c=document.createElement('canvas');c.width=w;c.height=h;c.getContext('2d').drawImage(source.image,0,0);return c.toDataURL('image/png');
    }
    const target=new T.WebGLRenderTarget(w,h),scene=new T.Scene(),cam=new T.OrthographicCamera(-1,1,1,-1,0,1);
    const material=new T.ShaderMaterial({uniforms:{tex:{value:source}},vertexShader:'varying vec2 vUv; void main(){ vUv=uv; gl_Position=vec4(position.xy,0.,1.); }',fragmentShader:'uniform sampler2D tex; varying vec2 vUv; void main(){ gl_FragColor=texture2D(tex,vUv); }',depthTest:false,depthWrite:false});
    const geo=new T.PlaneGeometry(2,2);scene.add(new T.Mesh(geo,material));const previous=T.renderer.getRenderTarget();
    try {
      T.renderer.setRenderTarget(target);T.renderer.render(scene,cam);
      const pixels=new Uint8Array(w*h*4);T.renderer.readRenderTargetPixels(target,0,0,w,h,pixels);
      const c=document.createElement('canvas');c.width=w;c.height=h;const ctx=c.getContext('2d'),out=ctx.createImageData(w,h);
      for(let y=0;y<h;y++)out.data.set(pixels.subarray(y*w*4,(y+1)*w*4),(h-y-1)*w*4);
      ctx.putImageData(out,0,0);return c.toDataURL('image/png');
    } finally {T.renderer.setRenderTarget(previous);target.dispose();material.dispose();geo.dispose();delete window.__su7Scenes[scene.uuid];}
  }
  function setQuality(level) {
    if(!qualityPresets[level])return;
    quality=level;
    return {quality,tile:TILE,pixelRatio:renderRatio};
  }
  function applyRenderGraphics(){
    if(!runtime||!engine)return;
    const cap=runtime.renderer.capabilities,w=Math.max(1,innerWidth),h=Math.max(1,innerHeight),requested=graphics.aa==='off'?0:Number(graphics.aa.replace('msaa',''))||0;
    actualSamples=interactionQuality?0:Math.min(requested,cap.maxSamples||0);
    const budgets={low:256,medium:384,high:768,ultra:1024,extreme:1536},pixels=Math.min(16000000,budgets[quality]*1048576/(64+actualSamples*24));
    renderRatio=Math.max(.25,Math.min(Number(graphics.resolution)||1,Math.sqrt(pixels/(w*h)),cap.maxTextureSize/w,cap.maxTextureSize/h,8192/w,8192/h));if(interactionQuality)renderRatio=Math.max(.25,renderRatio*.6);
    runtime.renderer.setPixelRatio(renderRatio);engine._dpr=renderRatio;engine._fpsWarnThreshold=0;engine.renderAreaScale=1;
    engine.traversePlugins(p=>{if(p.type==='EffectComposerPlugin'){if(p.multisampling!==actualSamples)p.multisampling=actualSamples;p._checkOutputPass?.();p.composer.setSize(w,h,false);}if(/TRAA|Temporal/i.test(p.type))p.enable=graphics.aa!=='off'&&!interactionQuality;});
    const anisotropy=Math.min(cap.getMaxAnisotropy(),quality==='low'?1:quality==='medium'?4:16);
    for(const m of meshes){const source=m.userData.studioSource;for(const key of ['map','normalMap','aoMap','lightMap'])if(source[key]&&source[key].anisotropy!==anisotropy){source[key].anisotropy=anisotropy;source[key].needsUpdate=true;}}
    if(texture){texture.anisotropy=anisotropy;texture.needsUpdate=true;}
  }
  function setGraphics(options){graphics={...graphics,...options};graphics.precision=Math.max(512,Math.min(8192,Number(graphics.precision)||2048));const wanted=Math.min(graphics.precision,2048,runtime?Math.floor(runtime.renderer.capabilities.maxTextureSize/3):2048);if(TILE!==wanted&&atlas){TILE=wanted;atlas.width=TILE*3;atlas.height=TILE*2;texture.dispose();texture=new runtime.Texture(atlas);texture.colorSpace='srgb';texture.flipY=false;texture.channel=2;for(const m of meshes){m.userData.studioMaterial.map=texture;m.userData.studioMaterial.needsUpdate=true;}updateAtlas();}else TILE=wanted;applyRenderGraphics();redraw();return {precision:graphics.precision,pixelRatio:renderRatio,samples:actualSamples};}
  window.su7Bridge = {
    attachViewer(viewer){
      if(engine===viewer)return;engine=viewer;const render=viewer.render.bind(viewer);let saved;
      viewer.render=function(dt){
        if(!prepared||!embedded||!document.body.classList.contains('studio-view'))return render(dt);
        const camera=viewer.camera,privateProps=camera.privateProps;if(!saved)saved=camera.clone();saved.copy(camera,false);resolvePaint();camera.copy(fitCamera(camera),false);camera.updateMatrixWorld(true);
        // The archived compositor reapplies the configurator's sidebar offset
        // from privateProps even after clearViewOffset(). Editor rendering and
        // ray picking must use the same centered projection.
        camera.privateProps={...privateProps,offsetX:0,offsetY:0};
        if(graphics.aa==='off')engine.traversePlugins(p=>{if(/TRAA|Temporal/i.test(p.type)&&p.enable)p.enable=false;});
        try{lightingEngine?.apply();render(dt);renderCount++;}finally{camera.copy(saved,false);camera.privateProps=privateProps;camera.updateMatrixWorld(true);}
      };
    },
    registerMaterial(key,material){if(key.startsWith('carBody_MT_')&&material.isMeshPhysicalMaterial)nativeMaterials.set(key,material);},
    frame(rt) {
      if(!prepared && rt.scene.getObjectByName('carPaint_external_4')?.material.isMeshPhysicalMaterial)collect(rt);
      if(prepared&&engine&&runtime.renderer.domElement.isConnected&&!readySent) {
        readySent=true;if(embedded)parent.postMessage({type:'su7-ready',meshes:meshes.length},location.origin);
      }
    }, setLighting(value){lightingValue={...lightingValue,...value};lightingEngine?.set(lightingValue);redraw();},get lighting(){return lightingEngine?.actual||lightingValue;},update,guides,view,extract,hideOptions,configTab,readConfiguration,restoreConfiguration,selectConfiguration,setQuality,setGraphics,togglePart,paintHit,exportPaintMasks,makeWrap,reframeWrap,setArtworkLayers,pickArtwork,
    previewWrap(id,wrap){renderWrapped(id,wrap);},projectedPaintHit(region,u,v){return surfaceGraph().projectedHit(region,u,v);},setInteractionQuality(value){value=!!value;if(interactionQuality===value)return;interactionQuality=value;applyRenderGraphics();redraw();},hasArtworkTexture(id,signature){return wrappedTextureCache.has(signature);},
    get wraps(){return [...wrappedLayers].map(([id,v])=>({id,selected:!!v.selected.value,parts:[...new Set(v.wrap.patches.map(p=>p.part))],regions:[...new Set(v.wrap.patches.map(p=>p.region))],nodes:v.nodes.length,texture:v.texture?.image&&[v.texture.image.width,v.texture.image.height]}));},
    setStage(value){
      const entering=value==='edit'&&!editingStage;
      if(editingStage&&value!=='edit')editingCameraState=navigation?.state;
      if(value==='edit')for(const [key,state] of Object.entries(partStates()))if(state.open)togglePart(key);
      editingStage=value==='edit';
      if(entering)requestAnimationFrame(()=>{if(prepared&&editingStage){navigation.viewport(innerWidth,innerHeight);if(editingCameraState)navigation.restore(editingCameraState);else navigation.fit();}});
      updateArtworkSelection();
    },get editingStage(){return editingStage;},setArtworkTool(value){artworkTool=!!value;artworkDrag=false;},
    closeParts(){for(const [key,state] of Object.entries(partStates()))if(state.open)togglePart(key);},
    get parts(){return partStates();},
    showConfiguration(show){hideOriginalUI(true);view('hero');},
    release(){prepared=false;clearTimeout(motionWatch);cameraInput?.dispose();if(runtime){runtime.renderer.dispose();runtime.renderer.forceContextLoss();}watchOptions.disconnect();},
    get cameraState(){return navigation?.state||null;},
    setCamera({fov,preserveFraming}={}){if(!navigation)return null;if(typeof preserveFraming==='boolean')navigation.setFraming(preserveFraming);if(Number.isFinite(fov))navigation.setFov(fov);return navigation.state;},
    fitView(){return navigation?.fit();},resetView(){return navigation?.reset();},focusArtwork,
    get ready(){return prepared;},
    get diagnostics(){const plugins=[];engine?.traversePlugins(p=>plugins.push({type:p.type,keys:Object.keys(p),enable:p.enable,samples:p.multisampling,composer:p.composer?{width:p.composer.inputBuffer?.width,height:p.composer.inputBuffer?.height,samples:p.composer.inputBuffer?.samples}:null}));return {dpr:engine?._dpr,width:engine?._width,height:engine?._height,plugins};},
    get factoryMaterials(){return [...nativeMaterials.values()];},
    get info(){return {meshes:meshes.length,regions,quality,pixelRatio:renderRatio,graphics:{...graphics,samples:actualSamples},renderCount,engineAttached:!!engine,artworkTextures:wrappedTextureCache.size,factoryColor:(selectedPaint||meshes[0]?.userData.studioSource)?.color.getHexString(),factoryName:(selectedPaint||meshes[0]?.userData.studioSource)?.name,atlas:atlas?{width:atlas.width,height:atlas.height}:null,gpu:runtime?{textures:runtime.renderer.info.memory.textures,geometries:runtime.renderer.info.memory.geometries,canvas:{width:runtime.renderer.domElement.width,height:runtime.renderer.domElement.height},adapter:(()=>{const gl=runtime.renderer.getContext(),ext=gl.getExtension('WEBGL_debug_renderer_info');return ext?gl.getParameter(ext.UNMASKED_RENDERER_WEBGL):gl.getParameter(gl.RENDERER);})()}:null};},
    async screenshot(){const previous=selectionVisible;selectionVisible=false;updateArtworkSelection();try{await engine.requestFrames(1);return runtime.renderer.domElement.toDataURL('image/png');}finally{selectionVisible=previous;updateArtworkSelection();}},
    async renderOffscreen({yaw,pitch,width,height,fov=navigation?.state.fov||30,currentView=false}){
      const cap=runtime.renderer.capabilities,max=cap.maxTextureSize;
      if(![width,height].every(Number.isInteger)||width<512||height<512||width>max||height>max||width*height>16000000)throw Error(`输出尺寸超出限制（显卡最大纹理边长 ${max}px，像素总量上限 16 MP）`);
      if(!currentView&&(![yaw,pitch,fov].every(Number.isFinite)||yaw< -180||yaw>180||pitch< -10||pitch>90||fov<15||fov>90))throw Error('相机角度或 FOV 超出有效范围');
      const T=runtime,renderer=T.renderer,state=navigation.renderState({yaw,pitch,fov,width,height,current:currentView});
      const cam=navigation.apply(engine.camera.clone(),width,height,state);
      const target=new T.WebGLRenderTarget(width,height,{depthBuffer:true,stencilBuffer:false});if(T.SRGBColorSpace)target.texture.colorSpace=T.SRGBColorSpace;
      const previous=renderer.getRenderTarget(),pixels=new Uint8Array(width*height*4),canvas=document.createElement('canvas');
      try{await engine.requestFrames(1);lightingEngine?.apply();renderer.setRenderTarget(target);renderer.render(T.scene,cam);renderer.readRenderTargetPixels(target,0,0,width,height,pixels);canvas.width=width;canvas.height=height;const ctx=canvas.getContext('2d'),image=ctx.createImageData(width,height);for(let y=0;y<height;y++)image.data.set(pixels.subarray(y*width*4,(y+1)*width*4),(height-y-1)*width*4);ctx.putImageData(image,0,0);return canvas.toDataURL('image/png');}
      finally{renderer.setRenderTarget(previous);target.dispose();canvas.width=canvas.height=1;}
    }
  };
})();
