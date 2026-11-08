// Real packaged Electron + Rust measurements, isolated from user projects.
// Test windows are hidden but run at foreground timer/rAF priority.
const { _electron: electron } = require('../desktop/node_modules/playwright-core');
const fs = require('node:fs');
const path = require('node:path');
const os = require('node:os');
const assert = require('node:assert/strict');
const { performance } = require('node:perf_hooks');
const root = path.resolve(__dirname, '..');
const output = path.join(__dirname, 'acceptance', 'performance');
fs.mkdirSync(output, { recursive: true });
const data = fs.mkdtempSync(path.join(os.tmpdir(), 'itasha-perf-'));
const report = { time: new Date().toISOString(), runtime: 'Electron + Rust', quality: 'medium', window: [1600, 1000], cpu: os.cpus()[0].model, logicalProcessors: os.cpus().length, ramGB: Math.round(os.totalmem() / 2 ** 30), metrics: {}, errors: [] };
const stats = values => { const v = [...values].sort((a,b)=>a-b); return { samples: v.length, median: v[Math.floor(v.length * .5)], p95: v[Math.min(v.length-1, Math.floor(v.length * .95))], max: v.at(-1) }; };
let app, page;
async function timed(name, action) { const t = performance.now(); const result = await action(); report.metrics[name] = +(performance.now() - t).toFixed(2); console.log(name + ': ' + report.metrics[name] + ' ms'); return result; }
async function dragMeasure(name, selector) {
  await page.evaluate(() => {
    window.__latency = []; window.__frames = []; window.__lastFrame = 0; window.__measure = true;
    const canvas = document.getElementById('design-canvas');
    window.__pointerProbe = e => { if (e.buttons) { const t = performance.now(); requestAnimationFrame(() => { if(window.__measure) __latency.push(performance.now() - t); }); } };
    canvas.addEventListener('pointermove', __pointerProbe);
    const frame = t => { if (!__measure) return; if (__lastFrame) __frames.push(t - __lastFrame); __lastFrame=t; requestAnimationFrame(frame); }; requestAnimationFrame(frame);
  });
  const b = await page.locator(selector).boundingBox(); assert.ok(b, selector);
  await page.mouse.move(b.x + b.width / 2, b.y + b.height / 2); await page.mouse.down();
  for (let i=1;i<=45;i++) { await page.mouse.move(b.x+b.width/2+Math.sin(i/10)*32, b.y+b.height/2+Math.sin(i/9)*8); await page.waitForTimeout(16); }
  await page.mouse.up();
  const v = await page.evaluate(() => { __measure=false; document.getElementById('design-canvas').removeEventListener('pointermove', __pointerProbe); return { latency:__latency, frames:__frames }; });
  report.metrics[name] = { eventToNextFrameMs: stats(v.latency), frameIntervalMs: stats(v.frames) }; console.log(name + ': ' + JSON.stringify(report.metrics[name]));
}
(async()=>{
  const start=performance.now();
  app=await electron.launch({executablePath:path.join(root,'desktop/electron/痛车编辑器.exe'),cwd:root,env:{...process.env,ITASHA_ROOT:root,ITASHA_DATA_DIR:data,ITASHA_PROFILE_DIR:path.join(data,'profile'),ITASHA_PORT:'0',ITASHA_TEST_MODE:'1'},timeout:60000});
  page=await app.firstWindow(); page.on('pageerror',e=>report.errors.push(e.stack)); page.on('dialog',d=>d.accept().catch(()=>{}));
  // A never-shown Windows window has a 1 Hz compositor despite timer switches.
  // Show it outside the desktop so the benchmark uses foreground frame timing.
  await app.evaluate(({BrowserWindow})=>{const w=BrowserWindow.getAllWindows()[0];w.setPosition(-30000,-30000,false);w.showInactive();});
  await page.waitForFunction(()=>window.studio); report.metrics.launchToHomeMs=+(performance.now()-start).toFixed(2);
  report.gpu=await app.evaluate(async ({app})=>app.getGPUInfo('basic'));
  await page.evaluate(()=>localStorage.setItem('su7-studio-settings',JSON.stringify({layout:'both',quality:'medium',autosave:false,live:true})));
  await page.reload(); await page.waitForFunction(()=>window.studio);
  await page.click('#home-new'); await page.fill('#new-project-name','性能验收');
  await timed('firstModelLoadMs',async()=>{await page.click('#new-project-create');await page.waitForFunction(()=>studio.ready,null,{timeout:120000});});
  await page.click('#configuration-confirm'); await page.waitForFunction(()=>studio.stage==='edit'); await page.evaluate(()=>studio.sync());
  // Native import including file read, format decode, color conversion, preview and UI.
  await page.evaluate(()=>studio.setPart('all','left',false));
  await timed('import4KMs',()=>page.evaluate(async()=>{
    const c=document.createElement('canvas');c.width=4096;c.height=2048;const x=c.getContext('2d');const g=x.createLinearGradient(0,0,4096,2048);g.addColorStop(0,'#f05');g.addColorStop(1,'#0df');x.fillStyle=g;x.fillRect(0,0,c.width,c.height);x.fillStyle='#fff';x.font='bold 300px sans-serif';x.fillText('SU7 PERFORMANCE',200,1000);
    const blob=await new Promise(r=>c.toBlob(r,'image/png')); await studio.importFiles([new File([blob],'4K性能图.png',{type:'image/png'})]);
  }));
  await page.evaluate(()=>studio.sync()); await dragMeasure('flatDrag','[data-transform="e"]');
  const viewer=page.frames().find(f=>f.url().includes('/configurator/')); await viewer.evaluate(()=>su7Bridge.view('left')); await page.waitForTimeout(400);
  // Fixed real SU7 front-door anchor avoids expensive benchmark-only ray scans.
  const hit=await viewer.evaluate(()=>{
    const record=Object.values(__su7Scenes).find(r=>r.scene.getObjectByName('carPaint_external_4'));
    let best;
    record.scene.traverse(mesh=>{for(let i=0;i<(mesh.userData.studioFaces?.length||0);i++){
      const f=mesh.userData.studioFaces[i];if(f.part!=='doorLF'||f.region!=='left')continue;
      const p=f.points[0].clone().add(f.points[1]).add(f.points[2]).multiplyScalar(1/3),score=Math.abs(p.x+.35)+Math.abs(p.y-.85);
      if(!best||score<best.score){const n=f.points[1].clone().sub(f.points[0]).cross(f.points[2].clone().sub(f.points[0])).normalize();const [u,v]=SU7Paint.projection(p,'left');best={score,mesh:mesh.name,face:i,point:p.toArray(),normal:n.toArray(),part:'doorLF',region:'left',u,v};}
    }});return best;
  });assert.ok(hit);
  await page.evaluate(hit=>{studio.setPart('all','left',false);studio.addAssetLayer(studio.project.library[0],hit);},hit);
  await page.fill('#prop-w','600');await page.locator('#prop-w').press('Tab'); await page.fill('#prop-h','180');await page.locator('#prop-h').press('Tab'); await page.evaluate(()=>studio.sync());
  report.wrap=await page.evaluate(()=>{const w=studio.allLayers.find(l=>l.id===studio.selected).wrap;return {faces:w.patches.reduce((n,p)=>n+p.faces.length,0),parts:[...new Set(w.patches.map(p=>p.part))]};});
  const computations=await viewer.evaluate(()=>{
    const w=parent.studio.allLayers.find(l=>l.id===parent.studio.selected).wrap,values=[];
    for(let i=0;i<35;i++){const t=performance.now();su7Bridge.reframeWrap(w,w.width*(1+i*.0001),w.height,i*.01);values.push(performance.now()-t);}return values;
  });report.metrics.curveRecomputeMs=stats(computations);
  const drawing=await page.evaluate(()=>{const values=[];for(let i=0;i<35;i++){const t=performance.now();const c=studio.createPanel('left',false);values.push(performance.now()-t);c.width=c.height=1;}return values;}); report.metrics.curvePanelDrawMs=stats(drawing);
  await dragMeasure('curvedDrag','[data-transform="e"]');
  report.rasterizer=await page.evaluate(()=>SU7WrapRasterizer.info);assert.ok(report.rasterizer.available&&report.rasterizer.draws>0,'GPU preview must actually be exercised');
  const saved=await timed('saveMs',()=>page.evaluate(()=>studio.save(true)));report.projectBytes=fs.statSync(path.join(data,'projects',saved.file)).size;
  report.memory=await app.evaluate(({app})=>app.getAppMetrics().map(p=>({type:p.type,workingSetKB:p.memory.workingSetSize,privateKB:p.memory.privateBytes})));
  assert.equal(report.errors.length,0); assert.ok(report.wrap.faces>100); assert.ok(report.metrics.curvedDrag.eventToNextFrameMs.samples>=20);
  // Product gates are interactive budgets on this reference machine, not universal claims.
  report.budgets={flatDragP95Ms:50,curvedDragP95Ms:100,curveRecomputeP95Ms:50,saveMs:1000,import4KMs:5000,firstModelLoadMs:15000};
  report.validTiming=report.metrics.flatDrag.frameIntervalMs.median<100;
  report.passed=report.validTiming&&report.metrics.flatDrag.eventToNextFrameMs.p95<=50&&report.metrics.flatDrag.frameIntervalMs.p95<=50&&report.metrics.curvedDrag.eventToNextFrameMs.p95<=100&&report.metrics.curvedDrag.frameIntervalMs.p95<=100&&report.metrics.curveRecomputeMs.p95<=50&&report.metrics.saveMs<=1000&&report.metrics.import4KMs<=5000&&report.metrics.firstModelLoadMs<=15000;
  if(process.argv.includes('--enforce')) assert.ok(report.passed,'Performance budgets exceeded; inspect report');
})().catch(e=>{report.errors.push(e.stack);console.error(e);process.exitCode=1;}).finally(async()=>{
  if(app)await app.close().catch(()=>{});
  const file=path.join(output,process.argv.includes('--baseline')?'baseline.json':'verification.json');fs.writeFileSync(file,JSON.stringify(report,null,2));console.log(file);console.log(JSON.stringify(report.metrics,null,2));
});
