// Verify the shipped root EXE, default root resolution and normal default port.
const {chromium}=require('../desktop/node_modules/playwright-core');
const fs=require('node:fs'),path=require('node:path'),net=require('node:net'),os=require('node:os'),assert=require('node:assert/strict');
const {spawn}=require('node:child_process');const root=path.resolve(__dirname,'..'),data=fs.mkdtempSync(path.join(os.tmpdir(),'itasha-launcher-'));
const sleep=ms=>new Promise(r=>setTimeout(r,ms));let browser,page;const report={time:new Date().toISOString(),checks:[],errors:[]};
async function freePort(){return new Promise(resolve=>{const s=net.createServer();s.listen(0,'127.0.0.1',()=>{const p=s.address().port;s.close(()=>resolve(p));});});}
(async()=>{
  const port=await freePort(),env={...process.env,ITASHA_DATA_DIR:data,ITASHA_PROFILE_DIR:path.join(data,'profile'),ITASHA_TEST_MODE:'1'};delete env.ITASHA_ROOT;delete env.ITASHA_PORT;
  const child=spawn(path.join(root,'痛车编辑器.exe'),['--itasha-debug='+port],{cwd:root,env,windowsHide:true,stdio:'ignore'});child.on('error',e=>{throw e;});
  for(let i=0;i<100;i++){try{browser=await chromium.connectOverCDP('http://127.0.0.1:'+port);break;}catch{await sleep(100);}}
  assert.ok(browser,'Root launcher must start Electron');
  for(let i=0;i<100;i++){page=browser.contexts()[0].pages().find(p=>p.url().startsWith('http://127.0.0.1:8878/'));if(page)break;await sleep(100);}
  assert.ok(page,'Default port must load the editor');await page.waitForFunction(()=>window.studio&&window.itashaHost);
  const info=await page.evaluate(async()=>({title:document.title,host:itashaHost.host,node:typeof require,backend:(await(await fetch('/__studio_health')).json()).backend}));
  assert.equal(info.host,'electron');assert.equal(info.backend,'rust');assert.equal(info.node,'undefined');report.checks.push('Root launcher resolves embedded Electron and Rust without ITASHA_ROOT','Default port 8878 loads the real editor','Renderer keeps Node isolation');
  const session=JSON.parse(fs.readFileSync(path.join(data,'.itasha-session.json'),'utf8'));await page.close();page=null;
  let running=true;for(let i=0;i<80;i++){try{process.kill(session.backendPid,0);}catch{running=false;break;}await sleep(100);}assert.equal(running,false,'Backend must exit after closing launched window');report.checks.push('Root-launched backend exits with its window');
  console.log('PASS '+report.checks.length+' launcher checks');
})().catch(e=>{report.errors.push(e.stack);console.error(e);process.exitCode=1;}).finally(async()=>{if(page&&!page.isClosed())await page.close().catch(()=>{});if(browser)await browser.close().catch(()=>{});const output=path.join(__dirname,'acceptance/launcher');fs.mkdirSync(output,{recursive:true});fs.writeFileSync(path.join(output,'verification.json'),JSON.stringify(report,null,2));});
