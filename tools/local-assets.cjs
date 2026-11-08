// Restore only offline vehicle assets from an existing full local distribution.
const fs=require('node:fs'),path=require('node:path');
const root=path.resolve(__dirname,'..');

function child(base,relative){
  if(typeof relative!=='string'||!relative||relative.includes('\\')||path.isAbsolute(relative)||relative.split('/').some(p=>!p||p==='..'||p==='.'||p.includes(':')))throw Error('无效的资源相对路径');
  const absolute=path.resolve(base,relative),inside=path.relative(base,absolute);
  if(inside.startsWith('..')||path.isAbsolute(inside))throw Error('资源路径超出目录');
  let current=absolute;
  while(current!==path.dirname(current)){
    if(fs.existsSync(current)&&fs.lstatSync(current).isSymbolicLink())throw Error('资源路径不接受符号链接：'+relative);
    if(current===base)break;current=path.dirname(current);
  }
  return absolute;
}

function assetPlan(source,definitions){
  source=path.resolve(source);const files=new Map();
  const add=(relative,metadata=false,expected)=>{
    const absolute=child(source,relative);
    if(!fs.existsSync(absolute)||!fs.statSync(absolute).isFile())throw Error('缺少离线资源：'+relative);
    const size=fs.statSync(absolute).size;
    if(Number.isInteger(expected)&&expected>=0&&size!==expected)throw Error('资源大小与清单不一致：'+relative);
    files.set(relative,{relative,size,metadata:metadata||files.get(relative)?.metadata||false});
  };
  const manifest=(relative,base)=>{
    add(relative,true);const data=JSON.parse(fs.readFileSync(child(source,relative),'utf8')),entries=Array.isArray(data)?data:data.entries;
    if(!Array.isArray(entries))throw Error('离线清单格式无效：'+relative);
    for(const entry of entries)if(entry.file){child(source,entry.file);add(base+'/'+entry.file,false,entry.bytes);}
  };
  for(const d of definitions){
    manifest(d.manifest,d.assetRoot);if(d.extras)manifest(d.extras,d.assetRoot);
    const captured='vehicles/'+d.id+'/manifest.json';
    if(captured!==d.manifest&&fs.existsSync(child(source,captured)))manifest(captured,'vehicles');
    for(const name of ['configuration.json','materials.json','ConfigTable.json','adapter.json','acceptance.json','source.json']){
      const relative='vehicles/'+d.id+'/'+name;if(fs.existsSync(child(source,relative)))add(relative,true);
    }
  }
  const ui='resources/ui';
  const walk=relative=>{const absolute=child(source,relative);for(const entry of fs.readdirSync(absolute,{withFileTypes:true})){const next=relative+'/'+entry.name;if(entry.isSymbolicLink())throw Error('资源目录不接受符号链接');if(entry.isDirectory())walk(next);else add(next);}};
  if(!fs.existsSync(child(source,ui)))throw Error('缺少离线资源：'+ui);walk(ui);
  return [...files.values()].sort((a,b)=>a.relative.localeCompare(b.relative));
}

function restore(source,target,plan,{dryRun=false}={}){
  source=path.resolve(source);target=path.resolve(target);if(source===target)throw Error('来源与目标不能相同；检查当前目录请使用 --check');
  const missing=[];
  // Validate all existing destinations before copying anything. Never overwrite.
  for(const file of plan){
    const to=child(target,file.relative),from=child(source,file.relative);
    if(!fs.existsSync(to)){missing.push(file);continue;}
    if(!fs.statSync(to).isFile()||fs.statSync(to).size!==file.size||(file.metadata&&!fs.readFileSync(to).equals(fs.readFileSync(from))))throw Error('目标存在不同的资源，请先核对存档：'+file.relative);
  }
  if(!dryRun)for(const file of missing){const to=child(target,file.relative);fs.mkdirSync(path.dirname(to),{recursive:true});fs.copyFileSync(child(source,file.relative),to,fs.constants.COPYFILE_EXCL);}
  return {files:plan.length,bytes:plan.reduce((n,f)=>n+f.size,0),missing:missing.length,copied:dryRun?0:missing.length,dryRun};
}

function main(args){
  const index=args.indexOf('--from'),check=args.includes('--check'),dryRun=args.includes('--dry-run');
  if((check&&index!==-1)||(!check&&index===-1)||args.some((arg,i)=>!['--from','--check','--dry-run'].includes(arg)&&i!==index+1))throw Error('用法：node tools/local-assets.cjs --check 或 --from <完整目录> [--dry-run]');
  if(index!==-1&&(!args[index+1]||args[index+1].startsWith('--')))throw Error('--from 需要完整来源目录');
  const source=check?root:path.resolve(args[index+1]),definitions=JSON.parse(fs.readFileSync(path.join(root,'config/vehicles.json'),'utf8')).vehicles,plan=assetPlan(source,definitions);
  const result=check?{files:plan.length,bytes:plan.reduce((n,f)=>n+f.size,0),verified:true}:restore(source,root,plan,{dryRun});
  console.log(JSON.stringify(result,null,2));
}
module.exports={assetPlan,restore};
if(require.main===module)try{main(process.argv.slice(2));}catch(error){console.error(error.message);process.exitCode=1;}
