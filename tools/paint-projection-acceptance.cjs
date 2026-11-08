// Real SU7 GPU coverage regression. Uses a temporary profile and project.
const { _electron: electron } = require('../desktop/node_modules/playwright-core');
const fs = require('node:fs');
const path = require('node:path');
const os = require('node:os');
const assert = require('node:assert/strict');
const { PNG } = require('../desktop/node_modules/pngjs');
const root = path.resolve(__dirname, '..');

// Expected coverage comes from the original opaque model's orthographic
// silhouette, independent of projection UVs, normal buckets or artwork shaders.
// Actual coverage executes the production physical shader and alpha test;
// only its final output color is replaced with white for measurement.
async function coverage({region,part,legacy=false,empty=false,raw=false}) {
  const T=Object.values(__su7Scenes).find(r=>r.scene.getObjectByName('carPaint_external_4'));
  const top=region==='top',side=['left','right'].includes(region),width=side||top?1600:1000,height=top?646:side?480:738;
  const scene=new T.Scene(),camera=new T.OrthographicCamera(-(side||top?2.6:1.05),side||top?2.6:1.05,top?1.05:.775,-(top?1.05:.775),.1,50);
  camera.position.set(...({left:[0,.775,10],right:[0,.775,-10],top:[0,10,0],front:[-10,.775,0],rear:[10,.775,0]}[region]));
  if(top)camera.up.set(0,0,-1);camera.lookAt(new T.Vector3(0,top?0:.775,0));camera.updateMatrixWorld(true);
  const black=new T.ShaderMaterial({vertexShader:'void main(){gl_Position=projectionMatrix*modelViewMatrix*vec4(position,1.);}',fragmentShader:'void main(){gl_FragColor=vec4(0.,0.,0.,1.);}'});
  const white=new T.ShaderMaterial({vertexShader:'attribute float coverage; varying float value; void main(){value=coverage;gl_Position=projectionMatrix*modelViewMatrix*vec4(position,1.);}',fragmentShader:'varying float value; void main(){gl_FragColor=vec4(vec3(value),1.);}'});
  const sources=[],overlays=[],owned=[],materials=[black,white];
  T.scene.updateMatrixWorld(true);
  T.scene.traverse(m=>{
    if(!m.isMesh||m.material?.opacity===0)return;
    for(let p=m;p;p=p.parent)if(!p.visible)return;
    const isOverlay=/^studio_(overlay_|part_)/.test(m.name);
    if(m.name.startsWith('studio_')&&!isOverlay)return;
    let geometry=m.geometry,material=black;
    if(isOverlay){
      material=m.material.clone();const original=m.material.onBeforeCompile;
      material.onBeforeCompile=(shader,renderer)=>{
        if(!legacy||!m.name.startsWith('studio_overlay_'))original(shader,renderer);
        shader.fragmentShader=shader.fragmentShader.replace('#include <dithering_fragment>','#include <dithering_fragment>\ngl_FragColor = '+(raw?'diffuseColor':'vec4(1.0)')+';');
      };
      material.customProgramCacheKey=()=>m.material.customProgramCacheKey()+'-coverage-'+legacy+'-'+raw;
      materials.push(material);
    }else if(m.userData.studioFaces){
      geometry=geometry.index?geometry.toNonIndexed():geometry.clone();owned.push(geometry);
      const A=geometry.attributes.position.constructor;
      geometry.setAttribute('coverage',new A(new Float32Array(m.userData.studioFaces.flatMap(f=>Array(3).fill(!empty&&(!part||f.part===part)?1:0))),1));material=white;
    }
    const clone=new T.Mesh(geometry,material);clone.matrixAutoUpdate=false;clone.matrix.copy(m.matrixWorld);clone.frustumCulled=false;clone.renderOrder=m.renderOrder;scene.add(clone);
    if(isOverlay){clone.visible=false;overlays.push(clone);}else sources.push(clone);
  });
  const renderer=T.renderer,target=new T.WebGLRenderTarget(width,height),previous=renderer.getRenderTarget();
  // getClearColor requires a Color; the factory material supplies its constructor.
  const savedColor=T.scene.getObjectByName('carPaint_external_4').material.color.clone();renderer.getClearColor(savedColor);const savedAlpha=renderer.getClearAlpha();
  const read=()=>{const pixels=new Uint8Array(width*height*4);renderer.render(scene,camera);renderer.readRenderTargetPixels(target,0,0,width,height,pixels);const c=document.createElement('canvas');c.width=width;c.height=height;c.getContext('2d').putImageData(new ImageData(new Uint8ClampedArray(pixels.buffer),width,height),0,0);return c.toDataURL('image/png');};
  try{
    renderer.setClearColor(0,1);renderer.setRenderTarget(target);const expected=read();
    for(const source of sources)source.material=black;for(const overlay of overlays)overlay.visible=true;
    return {width,height,expected,actual:read()};
  }finally{renderer.setRenderTarget(previous);renderer.setClearColor(savedColor,savedAlpha);target.dispose();owned.forEach(g=>g.dispose());materials.forEach(m=>m.dispose());delete __su7Scenes[scene.uuid];}
}

function compare(result,rect,output){
  const {width:w,height:h}=result,e=PNG.sync.read(Buffer.from(result.expected.split(',')[1],'base64')).data,a=PNG.sync.read(Buffer.from(result.actual.split(',')[1],'base64')).data;let covered=0,missing=0,leaked=0;
  const inside=(x,y,margin=0)=>!rect||x>rect.x+margin&&x<rect.x+rect.w-margin&&y>rect.y+margin&&y<rect.y+rect.h-margin;
  const white=(data,x,y)=>data[((h-1-y)*w+x)*4]>200;
  const image=new PNG({width:w,height:h});
  for(let y=0;y<h;y++)for(let x=0;x<w;x++){
    const i=(y*w+x)*4,expected=white(e,x,y)&&inside(x,y),actual=white(a,x,y);
    image.data.set(expected?(actual?[255,255,255,255]:[255,0,0,255]):actual?[0,200,255,255]:[0,0,0,255],i);
    if(x<3||y<3||x>=w-3||y>=h-3)continue;
    const interior=inside(x,y,3)&&[-2,0,2].every(d=>white(e,x+d,y)&&white(e,x,y+d));
    if(interior){covered++;if(!actual)missing++;}
    if(actual&&(!inside(x,y,-3)||[-2,0,2].every(d=>!white(e,x+d,y)&&!white(e,x,y+d))))leaked++;
  }
  if(output)fs.writeFileSync(output,PNG.sync.write(image));
  return {covered,missing,missingRatio:missing/Math.max(1,covered),leaked};
}

async function main(){
  const output=path.join(__dirname,'acceptance','projection'),data=fs.mkdtempSync(path.join(os.tmpdir(),'itasha-projection-test-'));
  fs.mkdirSync(output,{recursive:true});const checks=[],errors=[],metrics={};let app,page;
  const check=(name,condition)=>{assert.ok(condition,name);checks.push(name);console.log('PASS '+name);};
  try{
    app=await electron.launch({executablePath:path.join(root,'desktop/electron/痛车编辑器.exe'),cwd:root,env:{...process.env,ITASHA_ROOT:root,ITASHA_DATA_DIR:data,ITASHA_PROFILE_DIR:path.join(data,'profile'),ITASHA_PORT:'0',ITASHA_TEST_MODE:'1'},timeout:60000});
    page=await app.firstWindow();page.on('pageerror',e=>errors.push(e.stack));page.on('console',m=>{if(m.type()==='error')errors.push(m.text());});page.on('dialog',d=>d.accept().catch(()=>{}));
    await app.evaluate(({BrowserWindow})=>{const w=BrowserWindow.getAllWindows()[0];w.setPosition(-30000,-30000,false);w.showInactive();});
    await page.waitForFunction(()=>window.studio);await page.evaluate(()=>localStorage.setItem('su7-studio-settings',JSON.stringify({layout:'both',quality:'medium',autosave:false,live:true})));await page.reload();await page.waitForFunction(()=>window.studio);
    await page.click('#home-new');await page.fill('#new-project-name','二维曲面投影回归');await page.click('#new-project-create');await page.waitForFunction(()=>studio.ready,null,{timeout:120000});await page.click('#configuration-confirm');await page.waitForFunction(()=>studio.stage==='edit');
    const viewer=page.frames().find(f=>f.url().includes('/configurator/'));
    const sync=async()=>{await page.evaluate(()=>studio.sync());await page.waitForFunction(()=>document.querySelector('#sync-status').textContent.includes('已同步'));};
    await page.evaluate(async()=>{studio.setPart('all','left',false);const c=document.createElement('canvas');c.width=712;c.height=250;const x=c.getContext('2d');x.fillStyle='#fff';x.fillRect(0,0,c.width,c.height);await studio.addImage('coverage.png',c.toDataURL());});
    const layer=await page.evaluate(()=>{const l=studio.allLayers.find(l=>l.id===studio.selected);return {asset:l.asset,id:l.id};});
    const setLayer=async(region,part,rect,paintMode='factory')=>{
      await page.evaluate(async({region,part,rect,paintMode,layer})=>{
        for(const panel of Object.values(studio.project.panels))panel.layers=[];
        studio.setPart(part||'all',region,false);const l=studio.addAssetLayer(layer.asset);Object.assign(l,{x:rect.x+rect.w/2,y:rect.y+rect.h/2,w:rect.w,h:rect.h,rotation:0});studio.project.paintMode=paintMode;await studio.sync();
      },{region,part,rect,paintMode,layer});
      // Exercise the UI change path to refresh picking and transform metadata.
      await page.selectOption('#paint-mode',paintMode);await sync();
    };
    const rect={x:1167.391304347826-356,y:155.21619135234596-125,w:712,h:250};
    await setLayer('left',null,rect);
    check('2D import preserves ordinary editable coordinates',await page.evaluate(()=>!studio.allLayers[0].wrap));
    metrics.legacy=compare(await viewer.evaluate(coverage,{region:'left',legacy:true}),rect,path.join(output,'legacy-coverage.png'));
    check('Regression detects the former normal-bucket gaps',metrics.legacy.missingRatio>.02);
    metrics.reproduction=compare(await viewer.evaluate(coverage,{region:'left'}),rect,path.join(output,'fixed-coverage.png'));
    check('User-positioned rear door and quarter panel have continuous coverage',metrics.reproduction.covered>10000&&metrics.reproduction.missingRatio<.005&&metrics.reproduction.leaked<300);
    check('Artwork on faces assigned to another normal region remains selectable',await viewer.evaluate(()=>{
      const r=Object.values(__su7Scenes).find(r=>r.scene.getObjectByName('carPaint_external_4')),l=parent.studio.allLayers[0];let tested=0;
      r.scene.traverse(m=>{for(let i=0;i<(m.userData.studioFaces?.length||0);i++){
        const f=m.userData.studioFaces[i];if(f.region==='left')continue;const p=f.points[0].clone().add(f.points[1]).add(f.points[2]).multiplyScalar(1/3),[u,v]=SU7Paint.projection(p,'left');if(Math.abs(u*1600-l.x)>l.w/2-5||Math.abs(v*480-l.y)>l.h/2-5)continue;
        const hit=su7Bridge.projectedPaintHit('left',u,v);if(!hit||hit.mesh!==m.name||hit.face!==i)continue;
        hit.region=f.region;[hit.u,hit.v]=SU7Paint.projection(p,f.region);if(su7Bridge.pickArtwork(hit)?.id===l.id)tested++;
      }});return tested>20;
    }));
    metrics.opposite=compare(await viewer.evaluate(coverage,{region:'right',empty:true}),null,path.join(output,'opposite-coverage.png'));
    check('Left artwork does not leak onto the opposite exterior',metrics.opposite.leaked<30);
    await viewer.evaluate(()=>su7Bridge.setGraphics({precision:2048}));
    const resized=compare(await viewer.evaluate(coverage,{region:'left'}),rect);
    check('Changing atlas resolution preserves curved coverage',resized.missingRatio<.005);
    await viewer.evaluate(()=>su7Bridge.setGraphics({precision:1024}));
    for(const region of ['left','right','top','front','rear']){
      const rect={x:0,y:0,w:['front','rear'].includes(region)?1000:1600,h:region==='top'?646:['front','rear'].includes(region)?738:480};await setLayer(region,null,rect);
      const result=metrics[region]=compare(await viewer.evaluate(coverage,{region}),rect,path.join(output,region+'-coverage.png'));
      check(region+' projection covers curved painted surfaces',result.covered>10000&&result.missingRatio<.005&&result.leaked<500);
    }
    for(const part of ['doorLB','body']){
      const rect={x:0,y:0,w:1600,h:480};await setLayer('left',part,rect);
      const result=metrics[part]=compare(await viewer.evaluate(coverage,{region:'left',part}),rect,path.join(output,part+'-coverage.png'));
      check(part+' projection stays continuous inside the selected part',result.covered>1000&&result.missingRatio<.007&&result.leaked<500);
    }
    const identities=()=>{const r=Object.values(__su7Scenes).find(r=>r.scene.getObjectByName('carPaint_external_4')),ids=[];r.scene.traverse(m=>{if(/^studio_(overlay_|part_)/.test(m.name))ids.push(m.geometry.uuid);});return ids;};
    const before=await viewer.evaluate(identities);await page.fill('#prop-w','1400');await page.locator('#prop-w').press('Tab');await sync();
    check('Transforming artwork reuses projection geometry',JSON.stringify(before)===JSON.stringify(await viewer.evaluate(identities)));
    await setLayer('left',null,rect,'custom');
    metrics.base=compare(await viewer.evaluate(coverage,{region:'left'}),null,path.join(output,'custom-base-coverage.png'));
    check('Custom base paint covers the whole body independently of transparent artwork',metrics.base.missingRatio<.005);
    await setLayer('left',null,{x:0,y:0,w:1600,h:480});
    const topLayer=await page.evaluate(async()=>{
      studio.setPart('all','top',false);const c=document.createElement('canvas');c.width=c.height=32;const x=c.getContext('2d');x.fillStyle='rgba(255,0,0,0.5)';x.fillRect(0,0,32,32);await studio.addImage('top-red.png',c.toDataURL());const l=studio.allLayers.find(l=>l.id===studio.selected);Object.assign(l,{x:800,y:323,w:1600,h:646});
      // Rust JSON object ordering can differ from UI surface ordering after reopen.
      studio.project.panels=Object.fromEntries(Object.entries(studio.project.panels).sort(([a],[b])=>a.localeCompare(b)));return l.id;
    });await page.selectOption('#paint-mode','factory');await sync();
    const overlap=await viewer.evaluate(()=>{
      // Stay inside both part masks so silhouette antialiasing cannot change
      // the expected 50% red over opaque white color sample.
      const masks={};
      for(const region of ['left','top']){
        const ctx=document.createElement('canvas').getContext('2d'),path=new Path2D(),size=[1600,region==='left'?480:646];
        for(const loop of parent.studio.paintData.views[region].parts.doorLB.loops){loop.forEach(([u,v],i)=>i?path.lineTo(u*size[0],v*size[1]):path.moveTo(u*size[0],v*size[1]));path.closePath();}
        masks[region]=(u,v)=>[-3,0,3].every(d=>ctx.isPointInPath(path,u*size[0]+d,v*size[1])&&ctx.isPointInPath(path,u*size[0],v*size[1]+d));
      }
      for(let y=100;y<300;y+=5)for(let x=400;x<1250;x+=5){
        const h=su7Bridge.projectedPaintHit('left',x/1600,y/480);if(!h||h.part!=='doorLB')continue;
        const r=Object.values(__su7Scenes).find(r=>r.scene.getObjectByName(h.mesh)),f=r.scene.getObjectByName(h.mesh).userData.studioFaces[h.face],n=f.points[1].clone().sub(f.points[0]).cross(f.points[2].clone().sub(f.points[0])).normalize(),p={x:h.point[0],y:h.point[1],z:h.point[2]},[u,v]=SU7Paint.projection(p,'top');
        if(n.y<.2||n.z<.2||!masks.left(x/1600,y/480)||!masks.top(u,v))continue;
        h.region=f.region;[h.u,h.v]=SU7Paint.projection(p,f.region);return {x,y,pick:su7Bridge.pickArtwork(h)?.id};
      }return null;
    });
    check('Overlapping views select the visible upper layer after JSON reordering',overlap?.pick===topLayer);
    const raw=await viewer.evaluate(coverage,{region:'left',raw:true}),pixels=PNG.sync.read(Buffer.from(raw.actual.split(',')[1],'base64')).data,index=((raw.height-1-overlap.y)*raw.width+overlap.x)*4;
    metrics.overlapColor=Array.from(pixels.subarray(index,index+4));
    check('Transparent cross-view artwork composites without erasing the lower view',pixels[index]>245&&pixels[index+1]>110&&pixels[index+1]<145&&pixels[index+2]>110&&pixels[index+2]<145);
    await page.evaluate(()=>{for(const l of studio.allLayers)l.part='doorLB';});await page.selectOption('#paint-mode','factory');await sync();
    check('Part-specific overlapping views use the same selection order',await viewer.evaluate(({x,y,id})=>{const h=su7Bridge.projectedPaintHit('left',x/1600,y/480);return su7Bridge.pickArtwork(h)?.id===id;},{...overlap,id:topLayer}));
    const partRaw=await viewer.evaluate(coverage,{region:'left',raw:true}),partPixels=PNG.sync.read(Buffer.from(partRaw.actual.split(',')[1],'base64')).data;
    metrics.partOverlapColor=Array.from(partPixels.subarray(index,index+4));
    check('Part-specific views use the same transparent composite order',partPixels[index]>245&&partPixels[index+1]>110&&partPixels[index+1]<145&&partPixels[index+2]>110&&partPixels[index+2]<145);
    const exported=await viewer.evaluate(async()=>{const files=await su7Bridge.extract();return {mapping:JSON.parse(files.find(f=>f.name==='paint-projection.json').data),geometry:JSON.parse(files.find(f=>f.name==='original-UV.json').data).every(m=>m.bindPositions.length===m.uv.length&&m.triangles.length>0)};});
    check('UV extraction records the curved projection rules and bind vertices',exported.mapping.version===2&&exported.geometry);
    check('No frontend or shader errors',errors.length===0);
  }catch(e){errors.push(e.stack);console.error(e);process.exitCode=1;}finally{
    if(app)await app.close().catch(()=>{});fs.writeFileSync(path.join(output,'verification.json'),JSON.stringify({time:new Date().toISOString(),checks,errors,metrics,data},null,2));console.log(JSON.stringify(metrics,null,2));
  }
}
module.exports={coverage,compare};
if(require.main===module)main();
