// Real input + render verification in an isolated packaged Electron session.
const {_electron:electron}=require('../desktop/node_modules/playwright-core');
const fs=require('node:fs'),path=require('node:path'),os=require('node:os'),assert=require('node:assert/strict');
const root=path.resolve(__dirname,'..'),output=path.join(__dirname,'acceptance','camera-navigation'),data=fs.mkdtempSync(path.join(os.tmpdir(),'itasha-camera-'));
fs.mkdirSync(output,{recursive:true});
const checks=[],errors=[],metrics={};let app,page,viewer;
const check=(name,condition)=>{assert.ok(condition,name);checks.push(name);console.log('PASS '+name);};
const same=(a,b)=>Math.abs(a-b)<1e-8;
const state=()=>viewer.evaluate(()=>su7Bridge.cameraState);
const frame=()=>viewer.waitForFunction(()=>su7Bridge.info.renderCount>window.__lastCameraFrame);
async function drag(button,dx,dy,modifiers=[]){
  const bounds=await page.locator('#viewer').boundingBox();
  for(const key of modifiers)await page.keyboard.down(key);
  await page.mouse.move(bounds.x+bounds.width*.48,bounds.y+bounds.height*.62);await page.mouse.down({button});await page.mouse.move(bounds.x+bounds.width*.48+dx,bounds.y+bounds.height*.62+dy,{steps:8});await page.mouse.up({button});
  for(const key of modifiers)await page.keyboard.up(key);
}
async function projection(){
  return viewer.evaluate(async()=>{
    const r=Object.values(__su7Scenes).find(r=>r.scene.getObjectByName('carPaint_external_4')),mesh=r.scene.getObjectByName('carPaint_external_4'),original=mesh.onBeforeRender,state=su7Bridge.cameraState;let capture;
    mesh.onBeforeRender=function(renderer,scene,camera,...rest){
      original.call(this,renderer,scene,camera,...rest);if(!camera.isPerspectiveCamera||Math.abs(camera.fov-state.fov)>1e-6)return;
      const xs=[],ys=[];for(const x of [state.framing.min[0],state.framing.max[0]])for(const y of [state.framing.min[1],state.framing.max[1]])for(const z of [state.framing.min[2],state.framing.max[2]]){const p=new r.Vector3(x,y,z).project(camera);xs.push(p.x);ys.push(p.y);}
      capture={fov:camera.fov,position:camera.position.toArray(),size:Math.max(Math.max(...xs)-Math.min(...xs),Math.max(...ys)-Math.min(...ys)),projection:camera.projectionMatrix.elements.slice()};
    };
    try{await su7Bridge.screenshot();return capture;}finally{mesh.onBeforeRender=original;}
  });
}
async function paintPoint(){
  return viewer.evaluate(()=>{
    const canvas=document.querySelector('canvas'),rect=canvas.getBoundingClientRect();
    for(let y=.4;y<.8;y+=.04)for(let x=.28;x<.8;x+=.04){const hit=su7Bridge.paintHit(rect.left+x*rect.width,rect.top+y*rect.height);if(hit?.part==='doorLF'||hit?.part==='doorLB')return {x:rect.left+x*rect.width,y:rect.top+y*rect.height,hit};}return null;
  });
}
(async()=>{
  app=await electron.launch({executablePath:path.join(root,'desktop/electron/痛车编辑器.exe'),cwd:root,env:{...process.env,ITASHA_ROOT:root,ITASHA_DATA_DIR:data,ITASHA_PROFILE_DIR:path.join(data,'profile'),ITASHA_PORT:'0',ITASHA_TEST_MODE:'1'},timeout:60000});
  page=await app.firstWindow();page.on('pageerror',e=>errors.push(e.stack));page.on('console',m=>{if(m.type()==='error')errors.push(m.text());});page.on('dialog',d=>d.accept().catch(()=>{}));
  await app.evaluate(({BrowserWindow})=>{const w=BrowserWindow.getAllWindows()[0];w.setPosition(-30000,-30000,false);w.showInactive();});
  await page.waitForFunction(()=>window.studio);await page.evaluate(()=>localStorage.setItem('su7-studio-settings',JSON.stringify({layout:'both',quality:'medium',autosave:false,live:true})));await page.reload();await page.waitForFunction(()=>window.studio);
  await page.click('#home-new');await page.fill('#new-project-name','镜头导航验收');await page.click('#new-project-create');await page.waitForFunction(()=>studio.ready,null,{timeout:120000});await page.click('#configuration-confirm');await page.waitForFunction(()=>studio.stage==='edit');
  viewer=page.frames().find(f=>f.url().includes('/configurator/'));
  await viewer.waitForFunction(()=>document.querySelector('canvas')?.isConnected&&su7Bridge.info.renderCount>2);
  await viewer.evaluate(()=>new Promise(resolve=>requestAnimationFrame(()=>requestAnimationFrame(resolve))));
  const initial=await state();check('Initial lens matches the existing 30 degree preview',initial.fov===30);
  await drag('left',90,25);let next=await state();
  metrics.firstDrag={initial,next};
  check('Left drag rotates without changing FOV or distance',!same(initial.yaw,next.yaw)&&same(initial.distance,next.distance)&&same(initial.fov,next.fov));
  let previous=next;await drag('right',45,20);next=await state();
  check('Right drag pans without rotating or zooming',JSON.stringify(previous.target)!==JSON.stringify(next.target)&&same(previous.yaw,next.yaw)&&same(previous.pitch,next.pitch)&&same(previous.distance,next.distance)&&same(previous.fov,next.fov));
  previous=next;await drag('middle',-35,15);next=await state();check('Middle drag pans',JSON.stringify(previous.target)!==JSON.stringify(next.target)&&same(previous.distance,next.distance));
  previous=next;await drag('left',25,-15,['Shift']);next=await state();check('Shift left drag pans without changing orbit',JSON.stringify(previous.target)!==JSON.stringify(next.target)&&same(previous.yaw,next.yaw));
  await page.click('#camera-open');check('Lens panel exposes the current FOV and framing lock',await page.locator('#camera-panel').isVisible()&&await page.locator('#camera-preserve').isChecked());
  await page.click('#camera-reset');const base=await state(),baseProjection=await projection();
  check('Rendered camera has no legacy configurator sidebar offset',Math.abs(baseProjection.projection[8])<.01&&Math.abs(baseProjection.projection[9])<.01);
  for(const fov of [70,15,90,50,30]){
    await page.fill('#camera-fov-value',String(fov));await page.locator('#camera-fov-value').press('Tab');next=await state();const actual=await projection();
    check('FOV '+fov+' preserves the projected vehicle size in the actual renderer',same(next.fov,fov)&&Math.abs(actual.size-baseProjection.size)<1e-7&&same(actual.fov,fov));
  }
  next=await state();check('Repeated lens changes return to the original distance',same(next.distance,base.distance));
  metrics.fov={initial:baseProjection,restored:await projection()};
  await page.uncheck('#camera-preserve');previous=await state();await page.fill('#camera-fov-value','55');await page.locator('#camera-fov-value').press('Tab');next=await state();
  check('Unlocked FOV changes the lens at a fixed camera distance',same(previous.distance,next.distance)&&next.fov===55);
  await page.check('#camera-preserve');await page.click('#camera-reset');await page.click('#camera-close');
  await viewer.evaluate(()=>su7Bridge.view('left'));const point=await paintPoint();assert.ok(point);const box=await page.locator('#viewer').boundingBox();await page.mouse.move(box.x+point.x,box.y+point.y);
  previous=await state();await page.mouse.wheel(0,-180);await page.waitForTimeout(80);next=await state();
  check('Wheel dollies toward the cursor without modifying FOV',next.distance<previous.distance&&same(next.fov,previous.fov));
  check('Dolly pans the orbit center toward the surface anchor',JSON.stringify(next.target)!==JSON.stringify(previous.target));
  previous=next;await page.keyboard.down('Alt');await page.mouse.wheel(0,120);await page.keyboard.up('Alt');await page.waitForTimeout(80);next=await state();
  check('Alt wheel changes FOV and keeps framing enabled',next.fov>previous.fov&&next.preserveFraming);
  const beforeViews=next;for(const name of ['top','front','right','rear','left','hero'])await viewer.evaluate(name=>su7Bridge.view(name),name);
  next=await state();check('Named viewpoints preserve lens, distance and pan',same(beforeViews.fov,next.fov)&&same(beforeViews.distance,next.distance)&&JSON.stringify(beforeViews.target)===JSON.stringify(next.target));
  await viewer.evaluate(()=>su7Bridge.resetView());await viewer.evaluate(()=>su7Bridge.view('left'));
  const focusPoint=await paintPoint();assert.ok(focusPoint);previous=await state();await page.mouse.dblclick(box.x+focusPoint.x,box.y+focusPoint.y);next=await state();
  check('Double click focuses the clicked surface and keeps its lens',same(previous.fov,next.fov)&&next.distance<previous.distance&&Math.hypot(...next.target.map((v,i)=>v-focusPoint.hit.point[i]))<.03);
  await page.keyboard.press('Home');next=await state();check('Home fits the whole vehicle while preserving direction and FOV',Math.hypot(...next.target.map((v,i)=>v-[0,.8,0][i]))<1e-9&&same(next.yaw,previous.yaw)&&same(next.fov,previous.fov));
  previous=await state();await page.mouse.move(box.x+box.width*.5,box.y+box.height*.6);await page.mouse.down({button:'right'});await page.mouse.move(box.x+box.width*.6,box.y+box.height*.65);await page.keyboard.press('Escape');await page.mouse.up({button:'right'});next=await state();
  check('Escape cancels the entire camera gesture',JSON.stringify(previous)===JSON.stringify(next));
  previous=next;await app.evaluate(({BrowserWindow})=>BrowserWindow.getAllWindows()[0].setSize(1400,900));await page.waitForTimeout(250);next=await state();
  check('Resizing the viewport does not refit or change lens distance',same(previous.fov,next.fov)&&same(previous.distance,next.distance));
  await viewer.evaluate(()=>su7Bridge.setCamera({fov:50}));previous=await state();
  await page.click('#config-toggle');await page.waitForFunction(()=>studio.stage==='configure');await page.click('#configuration-confirm');await page.waitForFunction(()=>studio.stage==='edit');await viewer.evaluate(()=>new Promise(resolve=>requestAnimationFrame(()=>requestAnimationFrame(resolve))));
  check('Returning from vehicle configuration restores the entire editing camera',JSON.stringify(previous)===JSON.stringify(await state()));
  await page.evaluate(async()=>{studio.setPart('all','left',false);const c=document.createElement('canvas');c.width=400;c.height=150;const x=c.getContext('2d');x.fillStyle='#ff2877';x.fillRect(0,0,400,150);await studio.addImage('镜头定位素材.png',c.toDataURL());});await page.fill('#prop-x','700');await page.locator('#prop-x').press('Tab');await page.fill('#prop-y','260');await page.locator('#prop-y').press('Tab');await page.evaluate(()=>studio.sync());
  await page.click('#camera-open');previous=await state();await page.click('#camera-focus');next=await state();
  check('Focus artwork frames the selected ordinary image without moving it',next.distance<previous.distance&&same(next.fov,previous.fov)&&await page.evaluate(()=>studio.allLayers[0].x===700&&studio.allLayers[0].y===260));
  await page.screenshot({path:path.join(output,'镜头设置.png')});await page.click('#camera-close');
  // In artwork mode, camera gestures must never move the selected layer.
  await page.click('#artwork-tool');const layerBefore=await page.evaluate(()=>JSON.stringify(studio.allLayers));previous=await state();await drag('right',30,10);await drag('left',35,5,['Alt']);next=await state();
  check('Right pan and Alt orbit still work in artwork mode',JSON.stringify(previous.target)!==JSON.stringify(next.target)&&!same(previous.yaw,next.yaw));
  check('Camera navigation leaves artwork coordinates unchanged',layerBefore===await page.evaluate(()=>JSON.stringify(studio.allLayers)));await page.click('#artwork-tool');
  await viewer.locator('canvas[data-engine]').focus();await page.keyboard.press('Control+d');await page.waitForFunction(()=>studio.allLayers.length===2);
  check('Editor shortcuts still work when the 3D canvas has focus',await page.evaluate(()=>studio.allLayers.length===2));
  await page.keyboard.press('Control+z');await page.waitForFunction(()=>studio.allLayers.length===1);await page.keyboard.press('Control+Shift+z');await page.waitForFunction(()=>studio.allLayers.length===2);
  await page.locator('#layer-list .layer-item').last().click();await viewer.locator('canvas[data-engine]').focus();await page.keyboard.press('Delete');await page.waitForFunction(()=>studio.allLayers.length===1);
  check('Undo redo and deletion work from the focused 3D viewport',await page.evaluate(()=>studio.allLayers.length===1));
  await page.locator('#layer-list .layer-item').first().click();await viewer.locator('canvas[data-engine]').focus();await page.keyboard.press('Escape');await page.waitForFunction(()=>studio.selected===null);
  check('Escape without a camera gesture clears the editor selection',await page.locator('#artwork-selection').isHidden());
  await page.click('#high-res-render');check('Offscreen render defaults to the current framing and FOV',await page.locator('#render-current').isChecked()&&await page.locator('#render-fov').isDisabled());
  await page.fill('#render-width','1024');await page.fill('#render-height','768');previous=await state();await page.click('#render-start');await page.waitForFunction(()=>document.querySelector('#render-status').textContent==='渲染完成');
  check('Offscreen rendering does not mutate the interactive camera',JSON.stringify(previous)===JSON.stringify(await state()));
  check('Current-framing render produces the requested image',await page.evaluate(()=>document.querySelector('#render-image').naturalWidth===1024&&document.querySelector('#render-image').naturalHeight===768));
  await page.uncheck('#render-current');await page.fill('#render-fov','70');await page.fill('#render-pitch','90');await page.click('#render-start');await page.waitForFunction(()=>document.querySelector('#render-status').textContent==='渲染完成');
  check('Custom render accepts an explicit FOV and an exact top view',await page.evaluate(()=>document.querySelector('#render-dimensions').textContent.includes('FOV 70')));await page.locator('#high-res-dialog .close-dialog').click();
  // Navigation input processing should finish before the next foreground frame.
  metrics.navigation=await viewer.evaluate(async()=>{
    const canvas=document.querySelector('canvas'),box=canvas.getBoundingClientRect(),values=[];
    for(let i=0;i<30;i++){const t=performance.now();canvas.dispatchEvent(new WheelEvent('wheel',{bubbles:true,cancelable:true,deltaY:i%2?12:-12,clientX:box.width*.5,clientY:box.height*.6}));values.push(performance.now()-t);await new Promise(requestAnimationFrame);}
    const sorted=[...values].sort((a,b)=>a-b);return {samples:values.length,p95Ms:sorted[Math.floor(values.length*.95)],maxMs:sorted.at(-1)};
  });check('Camera input has no continuous animation or expensive event work',metrics.navigation.p95Ms<30);
  const cdp=await page.context().newCDPSession(page);metrics.layouts=[];
  for(const [width,height] of [[1920,1080],[1366,768],[900,650],[720,520]]){
    await cdp.send('Emulation.setDeviceMetricsOverride',{width,height,deviceScaleFactor:1,mobile:false});await page.waitForFunction(({width,height})=>innerWidth===width&&innerHeight===height,{width,height});await page.click('#camera-open');
    const layout=await page.evaluate(()=>{const p=document.querySelector('#camera-panel').getBoundingClientRect(),a=document.querySelector('.viewer-area').getBoundingClientRect(),button=document.querySelector('#camera-open').getBoundingClientRect();return {viewport:[innerWidth,innerHeight],panel:p.toJSON(),area:a.toJSON(),scroll:document.querySelector('.viewer-area').scrollTop,fits:p.left>=a.left&&p.right<=a.right+1&&p.top>=a.top&&p.bottom<=a.bottom+1&&p.height>70&&button.top>=a.top&&button.bottom<=a.bottom+1};});metrics.layouts.push(layout);
    check('Lens panel and toolbar fit '+width+' x '+height,layout.fits&&layout.scroll===0);await page.screenshot({path:path.join(output,'镜头设置-'+width+'.png')});await page.click('#camera-close');
  }
  await cdp.send('Emulation.clearDeviceMetricsOverride');
  check('No frontend or shader errors',errors.length===0);
})().catch(async e=>{errors.push(e.stack);console.error(e);process.exitCode=1;if(page&&!page.isClosed())await page.screenshot({path:path.join(output,'failure.png')}).catch(()=>{});}).finally(async()=>{
  if(app)await app.close().catch(()=>{});if(!process.exitCode)fs.rmSync(path.join(output,'failure.png'),{force:true});fs.writeFileSync(path.join(output,'verification.json'),JSON.stringify({time:new Date().toISOString(),checks,errors,metrics,data},null,2));console.log('Completed '+checks.length+' camera checks');console.log(JSON.stringify(metrics,null,2));
});
