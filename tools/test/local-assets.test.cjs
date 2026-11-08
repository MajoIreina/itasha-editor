const {test}=require('node:test'),assert=require('node:assert/strict'),fs=require('node:fs'),path=require('node:path'),os=require('node:os');
const {assetPlan,restore}=require('../local-assets.cjs');
const definitions=[{id:'su7',manifest:'resources/manifest.json',assetRoot:'resources'}];
function fixture(t){
  const temp=fs.mkdtempSync(path.join(os.tmpdir(),'itasha-assets-test-'));t.after(()=>fs.rmSync(temp,{recursive:true,force:true}));
  const source=path.join(temp,'source'),target=path.join(temp,'target');
  const write=(relative,text)=>{const p=path.join(source,relative);fs.mkdirSync(path.dirname(p),{recursive:true});fs.writeFileSync(p,text);};
  write('resources/manifest.json',JSON.stringify({entries:[{file:'data/blob',bytes:4}]}));write('resources/data/blob','test');write('resources/ui/thumbnail.png','image');write('projects/private.su7design','private');
  fs.mkdirSync(target);return {source,target,write};
}
test('asset inventory follows manifests and excludes private projects',t=>{
  const f=fixture(t),plan=assetPlan(f.source,definitions);assert.deepEqual(plan.map(p=>p.relative),['resources/data/blob','resources/manifest.json','resources/ui/thumbnail.png']);
});
test('missing or traversal assets are rejected before import',t=>{
  const f=fixture(t);f.write('resources/manifest.json',JSON.stringify({entries:[{file:'../projects/private.su7design'}]}));assert.throws(()=>assetPlan(f.source,definitions),/无效/);
  f.write('resources/manifest.json',JSON.stringify({entries:[{file:'data/missing',bytes:4}]}));assert.throws(()=>assetPlan(f.source,definitions),/缺少/);assert.deepEqual(fs.readdirSync(f.target),[]);
});
test('dry run does not write; restore copies missing assets without private data',t=>{
  const f=fixture(t),plan=assetPlan(f.source,definitions);assert.equal(restore(f.source,f.target,plan,{dryRun:true}).missing,3);assert.deepEqual(fs.readdirSync(f.target),[]);
  assert.equal(restore(f.source,f.target,plan).copied,3);assert.equal(fs.existsSync(path.join(f.target,'projects')),false);assert.equal(restore(f.source,f.target,plan).copied,0);
});
test('a conflicting manifest stops the entire import before any copies',t=>{
  const f=fixture(t),plan=assetPlan(f.source,definitions);fs.mkdirSync(path.join(f.target,'resources'));const dest=path.join(f.target,'resources/manifest.json');fs.writeFileSync(dest,'conflicting');
  assert.throws(()=>restore(f.source,f.target,plan),/目标存在不同/);assert.equal(fs.readFileSync(dest,'utf8'),'conflicting');assert.equal(fs.existsSync(path.join(f.target,'resources/data')),false);
});
test('size mismatch never overwrites an existing asset',t=>{
  const f=fixture(t),plan=assetPlan(f.source,definitions);restore(f.source,f.target,plan);const dest=path.join(f.target,'resources/data/blob');fs.writeFileSync(dest,'changed');assert.throws(()=>restore(f.source,f.target,plan),/目标存在不同/);assert.equal(fs.readFileSync(dest,'utf8'),'changed');
});
