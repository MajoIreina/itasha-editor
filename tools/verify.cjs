const { chromium } = require(process.env.PLAYWRIGHT_PATH || '../desktop/node_modules/playwright-core');
const fs=require('node:fs'),path=require('node:path'),assert=require('node:assert/strict');
const root=path.resolve(__dirname,'..'),base=process.env.STUDIO_URL||'http://127.0.0.1:8877';
(async()=>{
 const browser=process.env.STUDIO_CDP?await chromium.connectOverCDP(process.env.STUDIO_CDP):await chromium.launch({channel:'msedge',headless:true,proxy:{server:'http://127.0.0.1:9',bypass:'127.0.0.1,localhost'},args:['--enable-webgl','--ignore-gpu-blocklist','--enable-unsafe-swiftshader']});
 try {
 const context=process.env.STUDIO_CDP?browser.contexts()[0]:await browser.newContext({viewport:{width:1600,height:1000}}),page=process.env.STUDIO_CDP?context.pages()[0]:await context.newPage(),errors=[],checks=[];
 page.on('pageerror',e=>errors.push(e.stack));page.on('console',m=>{if(m.type()==='error')errors.push(m.text().slice(0,1800));});
 const check=(name,condition)=>{assert.ok(condition,name);checks.push(name);};
 const ready=()=>page.waitForFunction(()=>window.studio?.ready,null,{timeout:60000});
 await page.goto(base+'/');await page.waitForFunction(()=>window.studio);check('home starts without loading 3D',await page.evaluate(()=>studio.stage==='home'&&!studio.ready));await page.click('#home-new');await page.fill('#new-project-name','验收临时工程');await page.click('#new-project-create');await ready();check('new project starts with vehicle setup',await page.evaluate(()=>studio.stage==='configure'&&getComputedStyle(document.getElementById('editor-pane')).display==='none'));await page.click('#configuration-confirm');await page.waitForFunction(()=>studio.stage==='edit');await page.waitForTimeout(1200);
 check('offline renderer loaded',await page.evaluate(()=>studio.ready));
 await page.click('#sample-design');await page.fill('#project-name','验收示例-原厂蓝涂装');await page.locator('#project-name').dispatchEvent('change');
 await page.click('#import-path');await page.fill('#path-input',path.join(root,'tools','fixtures','test-artwork.png'));await page.click('#path-submit');await page.waitForFunction(()=>Object.keys(studio.project.assets).length===1);
 const data=fs.readFileSync(path.join(root,'tools','fixtures','test-artwork.png')).toString('base64');
 await page.evaluate(b64=>{const dt=new DataTransfer();dt.items.add(new File([Uint8Array.from(atob(b64),c=>c.charCodeAt(0))],'拖入素材.png',{type:'image/png'}));window.dispatchEvent(new DragEvent('drop',{dataTransfer:dt,bubbles:true,cancelable:true}));},data);
 await page.waitForFunction(()=>Object.keys(studio.project.assets).length===2);
 check('path and drag imports retain originals',await page.evaluate(()=>Object.values(studio.project.assets).every(a=>a.width===3000&&a.height===1500)));
 const before=await page.evaluate(()=>studio.project.panels.left.layers.at(-1).x),r=await page.locator('#design-canvas').boundingBox();
 await page.mouse.move(r.x+r.width/2,r.y+r.height/2);await page.mouse.down();await page.mouse.move(r.x+r.width/2+24,r.y+r.height/2+9,{steps:5});await page.mouse.up();
 check('canvas drag transforms a layer',await page.evaluate(()=>studio.project.panels.left.layers.at(-1).x)!==before);
 await page.fill('#prop-rotation','15');await page.locator('#prop-rotation').dispatchEvent('change');await page.fill('#prop-opacity','65');await page.locator('#prop-opacity').dispatchEvent('change');
 await page.click('#duplicate-layer');check('duplicate',await page.evaluate(()=>studio.project.panels.left.layers.length)===5);await page.click('#undo');check('undo',await page.evaluate(()=>studio.project.panels.left.layers.length)===4);await page.click('#redo');check('redo',await page.evaluate(()=>studio.project.panels.left.layers.length)===5);
 await page.click('#config-toggle');let f=page.frames().find(f=>f.url().includes('/configurator/'));
 await page.click('#config-tabs button:nth-child(2)');await page.click('[data-config-id="600138252"]');await page.waitForFunction(()=>!document.querySelector('#configuration-confirm').disabled);
 let ids=await f.evaluate(()=>su7Bridge.readConfiguration().map(v=>JSON.parse(v.params).nr_car_sku_id));check('sport wheel without black kit',ids.includes(600138252)&&ids.includes(600061085));
 await page.click('#config-tabs button:first-child');await page.click('[data-config-id="600138251"]');await page.waitForFunction(()=>!document.querySelector('#configuration-confirm').disabled);
 ids=await f.evaluate(()=>su7Bridge.readConfiguration().map(v=>JSON.parse(v.params).nr_car_sku_id));check('independent wheel and black kit',ids.includes(600138252)&&ids.includes(600138251));
 await page.click('[data-config-id="600061064"]');await page.waitForFunction(()=>!document.querySelector('#configuration-confirm').disabled);await page.click('#configuration-confirm');await page.waitForTimeout(600);
 try{await f.waitForFunction(()=>su7Bridge.info.factoryColor!=='00659e',null,{timeout:15000});}catch(e){console.log('FACTORY_DEBUG '+JSON.stringify(await f.evaluate(()=>({info:su7Bridge.info,paint:Object.values(__su7Scenes).flatMap(r=>{const a=[];r.scene.traverse(m=>{if(m.isMesh&&m.name.startsWith("carPaint_"))a.push({name:m.name,color:m.material.color?.getHexString(),source:m.userData.studioSource?.color?.getHexString(),native:m.userData.studioNativeMaterial?.color?.getHexString()});});return a;}),materials:su7Bridge.factoryMaterials.map(m=>({name:m.name,color:m.color.getHexString(),owner:m.userData.studioOwner,id:m.uuid})),selected:su7Bridge.readConfiguration()}))));console.log('ERRORS '+JSON.stringify(errors));await page.screenshot({path:path.join(root,'tools','factory-failed.png')});throw e;}check('factory paint changes beneath artwork',true);
 check('prices removed',!(await f.locator('body').innerText()).match(/[¥￥]/));
 await page.click('#settings-open');const qualities=[];
 for(const level of ['high','medium','low']){await page.selectOption('#setting-quality',level);await page.waitForTimeout(400);qualities.push(await f.evaluate(()=>su7Bridge.info));}
 check('quality changes real atlas sizes',qualities.map(q=>q.atlas.width).join(',')==='6144,3072,1536');
 check('quality changes pixel ratio',qualities.map(q=>q.pixelRatio).join(',')==='2,1,0.75');
 await page.locator('#settings-dialog button.primary').click();check('optional equipment hidden',await f.evaluate(()=>[...document.querySelectorAll('[data-studio-option]')].every(n=>getComputedStyle(n).display==='none')));
 const catalogue=await page.evaluate(()=>fetch('/__catalog').then(r=>r.json()));ids=await f.evaluate(()=>su7Bridge.readConfiguration().map(v=>JSON.parse(v.params).nr_car_sku_id));check('all optional equipment enabled',catalogue.options.every(id=>ids.includes(id)));
 await page.click('#export-open');await page.click('[data-export-tab="print"]');for(const [id,value] of [['print-width','300'],['print-height','90'],['print-dpi','150'],['print-bleed','5']])await page.fill('#'+id,value);await page.check('#print-calibrated');
 for(const ext of ['svg','png','pdf']){await page.click(`[data-export="print-${ext}"]`);await page.waitForFunction(ext=>document.querySelector('#export-status').textContent.includes('制作.'+ext),ext);}
 await page.locator('#export-dialog .dialog-actions button').click();checks.push('SVG PNG PDF export');
 await page.click('#extract-textures');await page.waitForFunction(()=>!document.querySelector('#extract-textures').disabled,null,{timeout:60000});
 check('original UV and material export',fs.existsSync(path.join(root,'exports','验收示例-原厂蓝涂装_UV与材质.zip')));
 await page.evaluate(()=>studio.save(true));const layers=await page.evaluate(()=>studio.project.panels.left.layers.length);
 const cdp=await context.newCDPSession(page);await cdp.send('HeapProfiler.collectGarbage');const active=await cdp.send('Runtime.getHeapUsage');
 await page.click('[data-layout="edit"]');await page.waitForTimeout(500);await cdp.send('HeapProfiler.collectGarbage');const edit=await cdp.send('Runtime.getHeapUsage');
 check('edit only unloads renderer',await page.evaluate(()=>!studio.ready&&document.querySelector('iframe').src==='about:blank'));
 await page.reload();await page.click('#home-resume');await page.waitForFunction(n=>window.studio?.project.panels.left.layers.length===n,layers,{timeout:30000});
 check('project and layout recovery',await page.evaluate(()=>studio.settings.layout==='edit'&&!studio.ready&&studio.settings.quality==='low'&&!studio.settings.options));
 await page.click('[data-layout="3d"]');await ready();await page.waitForTimeout(1800);f=page.frames().find(f=>f.url().includes('/configurator/'));
 check('3D only layout',await page.locator('#editor-pane').evaluate(e=>getComputedStyle(e).display==='none'));
 check('factory paint recovery',(await f.evaluate(()=>su7Bridge.info.factoryColor))!=='00659e');
 await page.click('[data-layout="both"]');await page.waitForTimeout(400);check('split layout',await page.locator('#editor-pane').isVisible());
 check('no page errors',errors.length===0);
 const result={time:new Date().toISOString(),checks,errors,offline:true,qualities:qualities.map(q=>({quality:q.quality,pixelRatio:q.pixelRatio,atlas:q.atlas,gpu:q.gpu})),heapMiB:{active:Math.round(active.usedSize/1048576),edit:Math.round(edit.usedSize/1048576),note:'JavaScript heap only; not total process or GPU memory.'}};
 fs.writeFileSync(path.join(root,'tools','verification.json'),JSON.stringify(result,null,2));await page.screenshot({path:path.join(root,'tools','verified-workbench.png')});
 console.log(JSON.stringify(result));
 } finally { if(!process.env.STUDIO_CDP)await browser.close(); }
})().catch(e=>{console.error(e);process.exitCode=1;}).finally(()=>{if(process.env.STUDIO_CDP)process.exit(process.exitCode||0);});
