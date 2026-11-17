import { createRequire } from 'node:module';
import { spawn } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import assert from 'node:assert/strict';
import net from 'node:net';
const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..');
const modelRoot = path.resolve(root, process.env.ITASHA_MODEL_ROOT || '../../3D');
const require = createRequire(import.meta.url);
const { chromium } = require('playwright-core');
const reportDir = path.join(root, '.reports', 'paint-v2-' + Date.now());
fs.mkdirSync(reportDir, { recursive: true });
const probe = net.createServer();
await new Promise(resolve => probe.listen(0, '127.0.0.1', resolve));
const port = probe.address().port;
await new Promise(resolve => probe.close(resolve));
const executable = path.resolve(root, process.env.ITASHA_SMOKE_EXE || 'target/release/itasha-desktop.exe');
fs.writeFileSync(path.join(reportDir, 'binary.json'), JSON.stringify({ executable, bytes: fs.statSync(executable).size, modified: fs.statSync(executable).mtime.toISOString() }, null, 2));
const child = spawn(executable, [], {
  cwd: root, windowsHide: true,
  env: { ...process.env, ITASHA_TEST_DATA_DIR: path.join(reportDir, 'data'), WEBVIEW2_USER_DATA_FOLDER: path.join(reportDir, 'webview'), WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: '--remote-debugging-port=' + port },
});
let stderr = '';
child.stderr.on('data', chunk => { stderr += chunk; });
let browser;
const checks = [];
try {
  for (let attempt = 0; attempt < 100; attempt++) {
    try { browser = await chromium.connectOverCDP('http://127.0.0.1:' + port); break; }
    catch { if (child.exitCode !== null) throw Error('Desktop exited: ' + stderr); await new Promise(resolve => setTimeout(resolve, 200)); }
  }
  assert(browser, 'WebView2 debugging endpoint did not open');
  const context = browser.contexts()[0];
  let page;
  for (let attempt = 0; attempt < 100; attempt++) {
    page = context.pages().find(p => !p.url().startsWith('devtools:'));
    if (page) break;
    await new Promise(resolve => setTimeout(resolve, 100));
  }
  assert(page, 'WebView2 page missing');
  const errors = [];
  page.on('pageerror', error => errors.push(error.message));
  await page.getByText('桌面主框架就绪', { exact: true }).waitFor();
  const ipc = (command, args) => page.evaluate(({ command, args }) => window.__TAURI_INTERNALS__.invoke(command, args), { command, args });
  const waitRenderer = async (predicate = value => value.state === 'ready' && value.visible && value.frames > 0 && value.nativeVisible && value.nativeFrontmost) => {
    let status;
    for (let attempt = 0; attempt < 150; attempt++) {
      status = await ipc('viewport_status');
      if (status.state === 'error') throw Error(status.message);
      if (predicate(status)) return status;
      await new Promise(resolve => setTimeout(resolve, 200));
    }
    throw Error('Renderer wait timed out: ' + JSON.stringify(status));
  };

  let view=await ipc('read_view');
  const settings={...view.settings,paths:{...view.settings.paths,models:modelRoot}};
  await ipc('execute',{requestId:crypto.randomUUID(),action:{type:'settings',value:settings}});
  const vehicle=view.vehicles.find(v=>v.id==='su7');
  await ipc('execute',{requestId:crypto.randomUUID(),action:{type:'create-configured',name:'漆面 V2 验收',vehicleId:'su7',configuration:vehicle.defaults}});
  await page.reload();
  await page.getByRole('button',{name:/继续当前设计/}).click();
  await page.locator('.workspace').waitFor();
  const status=await waitRenderer();
  assert(status.surfaces.every(s=>s.profileVersion===2));
  fs.writeFileSync(path.join(reportDir,'renderer.json'),JSON.stringify(status,null,2));
  checks.push('release native GPU pipeline loads 20 dedicated paintUv parts including 充电盖');
  await page.getByRole('button',{name:/矩形/}).click();
  await page.waitForFunction(()=>document.querySelectorAll('.layer-row').length===1);
  view=await ipc('read_view');
  let original=view.project.editor.layers[0];
  assert.equal(original.placement.profileVersion,2);
  assert.equal(original.placement.anchor.partId,original.placement.surfaceId);
  assert.equal(original.placement.anchor.paintSurfaceFingerprint,status.stats.surfaceFingerprint);
  assert.equal(typeof original.placement.anchor.sourceTriangleId,'string');
  const coverage=await ipc('viewport_coverage',{session:view.project.session,layer:original});
  assert(coverage.length>0);assert(coverage.every(f=>f.partId===original.placement.surfaceId));

  // Actual Canvas pointer drag, then verify one authoritative commit and undo.
  await page.getByRole('button',{name:'单部件',exact:true}).click();
  const bounds=await page.getByLabel('车身贴花画布',{exact:true}).boundingBox();
  const info=status.surfaces.find(s=>s.id===original.placement.surfaceId);
  const k=Math.min((bounds.width-56)/info.widthMm,(bounds.height-56)/info.heightMm);
  const cx=bounds.x+(bounds.width-info.widthMm*k)/2+original.placement.xMm*k;
  const cy=bounds.y+(bounds.height-info.heightMm*k)/2+original.placement.yMm*k;
  await page.mouse.move(cx,cy);await page.mouse.down();await page.mouse.move(cx+3,cy+2,{steps:3});
  assert.equal((await ipc('read_view')).project.revision,view.project.revision);
  await page.mouse.up();
  for(let n=0;n<50;n++){const v=await ipc('read_view');if(v.project.revision!==view.project.revision){view=v;break;}await new Promise(r=>setTimeout(r,100));}
  assert.notDeepEqual(view.project.editor.layers[0].placement,original.placement);
  await page.getByRole('button',{name:'撤销',exact:true}).click();
  await page.waitForTimeout(200);
  view=await ipc('read_view');original=view.project.editor.layers[0];
  checks.push('actual 2D pointer drag previews without history and commits once');
  const epoch=view.project.session.sessionEpoch,revision=view.project.revision;
  const stamp={sessionEpoch:epoch,baseRevision:revision,gestureId:crypto.randomUUID(),mappingVersion:2};
  const gesture=(kind,sequence,layer=null)=>ipc('viewport_gesture',{input:{kind,stamp,sequence,layer}});
  await gesture('begin',0,original);
  await assert.rejects(()=>ipc('viewport_gesture',{input:{kind:'begin',stamp:{...stamp,gestureId:crypto.randomUUID()},sequence:0,layer:original}}));
  const transformed={...original,placement:{...original.placement,rotationDeg:25}};
  await gesture('preview',2,transformed);
  await assert.rejects(()=>gesture('preview',1,original));
  let during=await ipc('read_view');assert.equal(during.project.revision,revision);
  await gesture('cancel',2);
  assert.deepEqual((await ipc('read_view')).project.editor.layers[0],original);
  checks.push('shared Rust gesture ownership, stale sequence rejection, preview without history, cancel');
  await gesture('begin',0,original);await gesture('preview',1,transformed);await gesture('commit',1);
  for(let n=0;n<50;n++){view=await ipc('read_view');if(view.project.revision!==revision)break;await new Promise(r=>setTimeout(r,100));}
  assert.equal(BigInt(view.project.revision),BigInt(revision)+1n);assert.equal(view.project.editor.layers[0].placement.rotationDeg,25);
  const mutate=async type=>{
    const current=(await ipc('read_view')).project;
    await ipc('execute',{requestId:crypto.randomUUID(),action:{type,mutation:{requestId:crypto.randomUUID(),session:current.session,expectedRevision:current.revision}}});
  };
  await mutate('undo');assert.deepEqual((await ipc('read_view')).project.editor.layers[0],original);
  await mutate('redo');await mutate('save');
  view=await ipc('read_view');const saved=view.project.editor;
  await ipc('execute',{requestId:crypto.randomUUID(),action:{type:'open',id:view.project.session.projectId}});
  assert.deepEqual((await ipc('read_view')).project.editor,saved);
  checks.push('one gesture one undo, redo and new schema save/reopen preserve source anchor');
  await page.reload();await page.getByRole('button',{name:/继续当前设计/}).click();await page.locator('.workspace').waitFor();await waitRenderer();
  await page.getByRole('button',{name:'单部件',exact:true}).click();
  await page.screenshot({path:path.join(reportDir,'part.png')});
  await page.getByRole('button',{name:'整车展开',exact:true}).click();
  await page.screenshot({path:path.join(reportDir,'whole.png')});
  assert.deepEqual((await ipc('read_view')).project.editor,saved);
  checks.push('whole-car and part modes preserve placement and source identity');
  if(fs.existsSync(path.join(reportDir,'data/native-viewport.png')))fs.copyFileSync(path.join(reportDir,'data/native-viewport.png'),path.join(reportDir,'native.png'));
  assert.deepEqual(errors,[]);
  await ipc('close_clean').catch(e=>{if(!String(e).includes('closed'))throw e;});
  await new Promise(resolve=>{if(child.exitCode!==null)resolve();else{child.once('exit',resolve);setTimeout(resolve,5000);}});
  assert.equal(child.exitCode,0);
  fs.writeFileSync(path.join(reportDir,'result.json'),JSON.stringify({passed:true,checks,scope:'Actual release native HWND/GPU pipeline and IPC; provisional semantic partition, no physical seam certification'},null,2));
  console.log(JSON.stringify({passed:true,checks,reportDir},null,2));
} catch(error) {
  try {const page=browser?.contexts()[0]?.pages()[0];if(page)await page.screenshot({path:path.join(reportDir,'failure.png')});}catch{}
  console.error('Report:',reportDir);throw error;
} finally {
 if(browser)await browser.close().catch(()=>{});
 if(child.exitCode===null)child.kill();
 if(stderr)fs.writeFileSync(path.join(reportDir,'stderr.log'),stderr);
}
