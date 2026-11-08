// Real SU7 regression for disconnected rear paint shells and old-project repair.
// Uses generated opaque artwork and an isolated profile; no user image is read.
const { _electron:electron }=require('../desktop/node_modules/playwright-core');
const { PNG }=require('../desktop/node_modules/pngjs');
const fs=require('node:fs'),path=require('node:path'),os=require('node:os'),assert=require('node:assert/strict');
const root=path.resolve(__dirname,'..'),output=path.join(__dirname,'acceptance','rear-wrap');
const data=fs.mkdtempSync(path.join(os.tmpdir(),'itasha-rear-wrap-'));
fs.mkdirSync(output,{recursive:true});
// Bind-pose anchors reproduce a bumper-origin wrap and a lower-trunk-origin wrap.
const anchors=[
  {region:'left',part:'rearBumper',mesh:'carPaint_internal_1',face:4028,point:[1.8936735328,.7597638204,.9227932486],normal:[.2364201168,.2528690816,.9381698972],right:[.884120434,.344515414,-.3156583402],down:[.4030342251,-.9040831583,.1421163477],at:[.524862414,.5416884352],width:17.4352924461,height:9.808617629},
  {region:'rear',part:'trunk',mesh:'carPaint_external_8',face:6570,point:[2.4581242393,.7648849339,-.1051901595],normal:[.863861212,.5027021693,-.0321610833],right:[.0906607085,-.2179628249,-.9717370235],down:[.4955042303,-.8365301763,.2338649649],at:[.4363058552,.4907031533],width:63,height:35.2103365385}
];

// Expected pixels are the original vehicle's visible painted triangles. Actual
// pixels execute the production decal shader, including its UV and alpha discard.
async function coverage({region,part,leftOnly=false}){
  const T=Object.values(__su7Scenes).find(r=>r.scene.getObjectByName('carPaint_external_4'));
  const top=region==='top',side=['left','right'].includes(region),width=top||side?1600:1000,height=top?646:side?480:738;
  const scene=new T.Scene(),camera=new T.OrthographicCamera(top||side?-2.6:-1.05,top||side?2.6:1.05,top?1.05:.775,top?-1.05:-.775,.1,50);
  camera.position.set(...({left:[0,.775,10],right:[0,.775,-10],top:[0,10,0],rear:[10,.775,0]}[region]));
  if(top)camera.up.set(0,0,-1);camera.lookAt(new T.Vector3(0,top?0:.775,0));camera.updateMatrixWorld(true);
  const black=new T.ShaderMaterial({vertexShader:'void main(){gl_Position=projectionMatrix*modelViewMatrix*vec4(position,1.);}',fragmentShader:'void main(){gl_FragColor=vec4(0.,0.,0.,1.);}'});
  const white=new T.ShaderMaterial({vertexShader:'attribute float coverage;varying float value;void main(){value=coverage;gl_Position=projectionMatrix*modelViewMatrix*vec4(position,1.);}',fragmentShader:'varying float value;void main(){gl_FragColor=vec4(vec3(value),1.);}'});
  const sources=[],overlays=[],owned=[],materials=[black,white];
  T.scene.updateMatrixWorld(true);
  T.scene.traverse(m=>{
    if(!m.isMesh||m.material?.opacity===0||SU7Paint.ignoreGuideOccluder(m))return;
    for(let p=m;p;p=p.parent)if(!p.visible)return;
    const overlay=m.name.startsWith('studio_wrap_');if(m.name.startsWith('studio_')&&!overlay)return;
    let geometry=m.geometry,material=black;
    if(overlay){
      material=m.material.clone();const original=m.material.onBeforeCompile;
      material.onBeforeCompile=(shader,renderer)=>{original(shader,renderer);shader.fragmentShader=shader.fragmentShader.replace('#include <dithering_fragment>','#include <dithering_fragment>\ngl_FragColor=vec4(1.);');};
      material.customProgramCacheKey=()=>m.material.customProgramCacheKey()+'-rear-test';materials.push(material);
    }else if(m.userData.studioFaces){
      geometry=geometry.index?geometry.toNonIndexed():geometry.clone();owned.push(geometry);
      const A=geometry.attributes.position.constructor;
      geometry.setAttribute('coverage',new A(new Float32Array(m.userData.studioFaces.flatMap(f=>{
        const p=f.points.reduce((v,p)=>v.add(p),new T.Vector3()).multiplyScalar(1/3);
        return Array(3).fill(f.part===part&&p.x>1.6&&p.y>.65&&p.y<1.16&&(!leftOnly||p.z>0)?1:0);
      })),1));material=white;
    }
    const clone=new T.Mesh(geometry,material);clone.matrixAutoUpdate=false;clone.matrix.copy(m.matrixWorld);clone.frustumCulled=false;clone.renderOrder=m.renderOrder;scene.add(clone);
    if(overlay){clone.visible=false;overlays.push(clone);}else sources.push(clone);
  });
  const renderer=T.renderer,target=new T.WebGLRenderTarget(width,height),previous=renderer.getRenderTarget();
  const savedColor=T.scene.getObjectByName('carPaint_external_4').material.color.clone();renderer.getClearColor(savedColor);const savedAlpha=renderer.getClearAlpha();
  const read=()=>{const pixels=new Uint8Array(width*height*4);renderer.render(scene,camera);renderer.readRenderTargetPixels(target,0,0,width,height,pixels);const c=document.createElement('canvas');c.width=width;c.height=height;c.getContext('2d').putImageData(new ImageData(new Uint8ClampedArray(pixels.buffer),width,height),0,0);return c.toDataURL();};
  try{
    renderer.setClearColor(0,1);renderer.setRenderTarget(target);const expected=read();
    for(const m of sources)m.material=black;for(const m of overlays)m.visible=true;
    return {expected,actual:read()};
  }finally{renderer.setRenderTarget(previous);renderer.setClearColor(savedColor,savedAlpha);target.dispose();owned.forEach(g=>g.dispose());materials.forEach(m=>m.dispose());delete __su7Scenes[scene.uuid];}
}

function compare(result,name){
  const expected=PNG.sync.read(Buffer.from(result.expected.split(',')[1],'base64')),actual=PNG.sync.read(Buffer.from(result.actual.split(',')[1],'base64'));
  const {width:w,height:h}=expected,diff=new PNG({width:w,height:h});let pixels=0,missing=0;
  const white=(p,x,y)=>p[(y*w+x)*4]>200;
  for(let y=0;y<h;y++)for(let x=0;x<w;x++){
    const e=white(expected.data,x,y),a=white(actual.data,x,y),i=(y*w+x)*4;
    diff.data.set(e?(a?[255,255,255,255]:[255,0,0,255]):[0,0,0,255],i);
    if(x<3||y<3||x>=w-3||y>=h-3)continue;
    if([-2,0,2].every(d=>white(expected.data,x+d,y)&&white(expected.data,x,y+d))){pixels++;if(!a)missing++;}
  }
  fs.writeFileSync(path.join(output,name+'.png'),PNG.sync.write(diff));return {pixels,missing,ratio:missing/Math.max(1,pixels)};
}

let app,page;const metrics={},errors=[],checks=[];
const check=(name,ok)=>{assert.ok(ok,name);checks.push(name);console.log('PASS '+name);};
(async()=>{
  app=await electron.launch({executablePath:path.join(root,'desktop/electron/痛车编辑器.exe'),cwd:root,env:{...process.env,ITASHA_ROOT:root,ITASHA_DATA_DIR:data,ITASHA_PROFILE_DIR:path.join(data,'profile'),ITASHA_PORT:'0',ITASHA_TEST_MODE:'1'},timeout:60000});
  page=await app.firstWindow();await app.evaluate(({BrowserWindow})=>{const w=BrowserWindow.getAllWindows()[0];w.setPosition(-30000,-30000,false);w.showInactive();});
  page.on('pageerror',e=>errors.push(e.stack));page.on('dialog',d=>d.accept().catch(()=>{}));
  await page.setViewportSize({width:1600,height:900});await page.waitForFunction(()=>window.studio);
  await page.evaluate(()=>localStorage.setItem('su7-studio-settings',JSON.stringify({layout:'both',quality:'medium',autosave:false,live:false})));
  await page.reload();await page.waitForFunction(()=>window.studio);
  await page.click('#home-new');await page.fill('#new-project-name','后部漆面覆盖回归');await page.click('#new-project-create');
  await page.waitForFunction(()=>studio.ready,null,{timeout:120000});await page.click('#configuration-confirm');
  const viewer=page.frames().find(f=>f.url().includes('/configurator/'));
  for(let index=0;index<anchors.length;index++){
    const spec=anchors[index];
    await page.evaluate(async({spec,index})=>{
      if(!studio.project.library.length){const c=document.createElement('canvas');c.width=c.height=64;const x=c.getContext('2d');x.fillStyle='#fff';x.fillRect(0,0,64,64);await studio.addImage('opaque.png',c.toDataURL());}
      const template=studio.allLayers[0],p=structuredClone(studio.project);
      for(const r of Object.values(p.panels))r.layers=[];
      p.panels[spec.region].layers=[{...template,id:'rear-test-'+index,part:spec.part,w:30000,h:16764,wrap:{...spec,version:3,patches:[]}}];
      await studio.openProject(p);await studio.sync();
    },{spec,index});
    const rebuilt=await page.evaluate(()=>structuredClone(studio.allLayers[0].wrap));
    check(`anchor ${index}: old v3 wrap is rebuilt with the current coverage revision`,rebuilt.version===3&&rebuilt.coverageVersion===4&&rebuilt.patches.length>0);
    for(const key of ['point','normal','right','down','at','width','height'])assert.deepEqual(rebuilt[key],spec[key],`migration preserves ${key}`);
    const parts=new Set(rebuilt.patches.map(p=>p.part));
    check(`anchor ${index}: rear paint neighbors are connected`,['trunk','body','rearBumper','empennage'].every(p=>parts.has(p)));
    check(`anchor ${index}: rear artwork does not reach the front bumper or bonnet`,!parts.has('frontBumper')&&!parts.has('bonnet'));
    metrics[index]={faces:rebuilt.patches.reduce((n,p)=>n+p.faces.length,0),coverage:{}};
    for(const [region,part] of [['top','trunk'],['rear','trunk'],['top','empennage'],['left','body'],['right','body']]){
      // A left-facing decal deliberately excludes the far right quarter panel.
      if(index===0&&region==='right')continue;
      const result=compare(await viewer.evaluate(coverage,{region,part,leftOnly:index===0}),`${index}-${region}-${part}`);
      metrics[index].coverage[region+':'+part]=result;console.log('COVERAGE',index,region,part,result);
      check(`anchor ${index}: ${region}/${part} has no visible paint hole`,result.pixels>50&&result.ratio<.01);
    }
    const image=await viewer.evaluate(()=>su7Bridge.renderOffscreen({yaw:50,pitch:18,width:1200,height:900}));
    fs.writeFileSync(path.join(output,`anchor-${index}.png`),Buffer.from(image.split(',')[1],'base64'));
    const saved=await page.evaluate(()=>studio.save(true)),project=JSON.parse(fs.readFileSync(path.join(data,'projects',saved.file),'utf8'));
    await page.evaluate(p=>studio.openProject(p),project);await page.evaluate(()=>studio.sync());
    const reopened=await page.evaluate(()=>structuredClone(studio.allLayers[0].wrap));
    assert.deepEqual(reopened,rebuilt,'current wraps reopen without repeated migration or UV drift');
    check(`anchor ${index}: saved coverage reopens unchanged`,true);
  }
  check('no renderer errors',errors.length===0);
})().catch(async e=>{errors.push(e.stack);console.error(e);process.exitCode=1;}).finally(async()=>{
  if(app)await app.close().catch(()=>{});
  fs.writeFileSync(path.join(output,'verification.json'),JSON.stringify({checks,errors,metrics},null,2));
  console.log(`DONE ${checks.length} checks; ${errors.length} errors`);
});
