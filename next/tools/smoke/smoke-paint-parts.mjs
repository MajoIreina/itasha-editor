import { createRequire } from 'node:module';
import { spawn } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import net from 'node:net';
import assert from 'node:assert/strict';
import { fileURLToPath } from 'node:url';
const root=path.resolve(path.dirname(fileURLToPath(import.meta.url)),'../..');
const modelRoot=path.resolve(root,process.env.ITASHA_MODEL_ROOT||'../../3D');
const require=createRequire(import.meta.url);const {chromium}=require('playwright-core');
const report=path.join(root,'.reports','paint-parts-'+Date.now());const data=path.join(report,'data');
fs.mkdirSync(path.join(data,'projects'),{recursive:true});
const server=net.createServer();await new Promise(r=>server.listen(0,'127.0.0.1',r));const port=server.address().port;await new Promise(r=>server.close(r));
const child=spawn(path.join(root,'target/release/itasha-desktop.exe'),[],{cwd:root,windowsHide:true,env:{...process.env,ITASHA_TEST_DATA_DIR:data,WEBVIEW2_USER_DATA_FOLDER:path.join(report,'webview'),WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS:'--remote-debugging-port='+port}});
let browser;
try{
 for(let i=0;i<100&&!browser;i++){try{browser=await chromium.connectOverCDP('http://127.0.0.1:'+port)}catch{await new Promise(r=>setTimeout(r,200))}}
 assert(browser);const page=browser.contexts()[0].pages()[0];
 const ipc=(command,args)=>page.evaluate(({command,args})=>window.__TAURI_INTERNALS__.invoke(command,args),{command,args});
 await page.getByRole('button',{name:/新建设计/}).click();await page.getByRole('button',{name:/轿车.*SU7 Max/}).click();
 await page.getByRole('button',{name:'下一步：车型选配 →',exact:true}).click();await page.getByRole('button',{name:'完成配置，进入工作区 →',exact:true}).click();
 await page.locator('.workspace').waitFor();let view=await ipc('read_view');
 const settings={...view.settings,layout:'edit',paths:{...view.settings.paths,models:modelRoot}};
 await ipc('execute',{requestId:crypto.randomUUID(),action:{type:'settings',value:settings}});await page.reload();
 await page.getByRole('button',{name:/继续当前设计/}).click();view=await ipc('read_view');await ipc('viewport_load',{session:view.project.session});
 let status;for(let i=0;i<150;i++){status=await ipc('viewport_status');if(status.state==='error')throw Error(status.message);if(status.state==='ready')break;await page.waitForTimeout(200)}
 assert.equal(status.state,'ready');assert.equal(status.surfaces.length,20);
 assert.equal(status.surfaces.find(s=>s.id==='su7.charge-cover')?.name,'充电盖');
 await page.getByRole('button',{name:'单部件',exact:true}).click();
 const select=page.getByLabel('编辑部件');const out=[];
 for(const s of status.surfaces){await select.selectOption(s.id);await page.getByRole('button',{name:'适应',exact:true}).click();await page.waitForTimeout(60);
  const file=path.join(report,s.id.replaceAll('.','-')+'.png');await page.screenshot({path:file});out.push({id:s.id,name:s.name,triangles:s.triangles,file})}
 await select.selectOption('su7.charge-cover');
 await page.getByRole('button',{name:/矩形/}).click();
 await page.waitForFunction(()=>document.querySelectorAll('.layer-row').length===1);
 view=await ipc('read_view');const layer=view.project.editor.layers[0];
 assert.equal(layer.placement.surfaceId,'su7.charge-cover');
 const coverage=await ipc('viewport_coverage',{session:view.project.session,layer});
 assert(coverage.length>0);assert(coverage.every(fragment=>fragment.partId==='su7.charge-cover'));
 await page.getByRole('button',{name:'3D 视图',exact:true}).click();
 let native3d;for(let n=0;n<100;n++){native3d=await ipc('viewport_status');if(native3d.state==='ready'&&native3d.visible&&native3d.frames>0&&native3d.nativeVisible&&native3d.nativeFrontmost)break;await page.waitForTimeout(100)}
 assert(native3d.state==='ready'&&native3d.visible&&native3d.frames>0&&native3d.nativeVisible&&native3d.nativeFrontmost,JSON.stringify(native3d));
 out.push({id:'su7.charge-cover-artwork',name:'充电盖贴花覆盖',coverageFragments:coverage.length,native3dVisible:native3d.nativeVisible,native3dFrames:native3d.frames});
 fs.writeFileSync(path.join(report,'parts.json'),JSON.stringify({passed:true,parts:out},null,2));console.log(report);
}finally{if(browser)await browser.close();child.kill()}
