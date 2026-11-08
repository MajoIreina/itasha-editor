// Install pinned desktop dependencies. Node/pnpm/Rust are build tools only.
const fs=require('node:fs'),path=require('node:path'),crypto=require('node:crypto');
const {execFileSync}=require('node:child_process');
const root=path.resolve(__dirname,'..'),desktop=path.join(root,'desktop');
const version='7.1.2-35',archiveHash='e7c629f3530ccb9d4918a15f0e928254feda28e84a7fa794b25873d152c075c1',exeHash='75d7b9fbaebab840ee71f9631df06659c0b39ff94bdc507cbac772023ff6ce5c';
const digest=f=>crypto.createHash('sha256').update(fs.readFileSync(f)).digest('hex');
const target=path.join(desktop,'native/imagemagick');
async function main(){
  if(!process.argv.includes('--native-only')&&!process.argv.includes('--check')){
    const cli=[process.env.ITASHA_PNPM_CLI,path.join(path.dirname(process.execPath),'node_modules/pnpm/bin/pnpm.cjs'),path.resolve(path.dirname(process.execPath),'../node_modules/pnpm/bin/pnpm.cjs')].find(p=>p&&fs.existsSync(p));
    if(!cli)throw Error('请将 ITASHA_PNPM_CLI 指向 pnpm 11.25.0 的 pnpm/bin/pnpm.cjs，见 docs/开发与构建.md');
    const actual=execFileSync(process.execPath,[cli,'--version'],{encoding:'utf8',windowsHide:true}).trim();if(actual!=='11.25.0')throw Error('构建要求 pnpm 11.25.0，当前为 '+actual);
    execFileSync(process.execPath,[cli,'--dir',desktop,'install','--frozen-lockfile'],{stdio:'inherit',windowsHide:true});
    if(!fs.existsSync(path.join(desktop,'node_modules/electron/dist/electron.exe')))execFileSync(process.execPath,[path.join(desktop,'node_modules/electron/install.js')],{stdio:'inherit',windowsHide:true});
  }
  const executable=path.join(target,'magick.exe');
  if(!fs.existsSync(executable)||digest(executable)!==exeHash){
    if(process.argv.includes('--check'))throw Error('ImageMagick 尚未安装或哈希不匹配');
    const downloads=path.join(root,'development/downloads');fs.mkdirSync(downloads,{recursive:true});const archive=path.join(downloads,'imagemagick-'+version+'.7z');
    if(!fs.existsSync(archive)||digest(archive)!==archiveHash){
      console.log('Downloading pinned ImageMagick '+version);
      const response=await fetch('https://github.com/ImageMagick/ImageMagick/releases/download/'+version+'/ImageMagick-'+version+'-portable-Q16-x64.7z');if(!response.ok)throw Error('下载失败：'+response.status);
      const bytes=Buffer.from(await response.arrayBuffer());if(crypto.createHash('sha256').update(bytes).digest('hex')!==archiveHash)throw Error('原生依赖 SHA256 不匹配');fs.writeFileSync(archive,bytes);
    }
    const native=path.join(desktop,'native');fs.mkdirSync(native,{recursive:true});const staging=fs.mkdtempSync(path.join(native,'imagemagick-staging-'));
    try{
      execFileSync(path.join(process.env.SystemRoot||'C:/Windows','System32/tar.exe'),['-xf',archive,'-C',staging],{stdio:'inherit',windowsHide:true});
      if(digest(path.join(staging,'magick.exe'))!==exeHash)throw Error('解压后的解码器 SHA256 不匹配');
      fs.mkdirSync(target,{recursive:true});for(const e of fs.readdirSync(staging,{withFileTypes:true})){if(e.isFile()&&(e.name==='magick.exe'||/\.(xml|icc|txt)$/i.test(e.name)))fs.copyFileSync(path.join(staging,e.name),path.join(target,e.name));}
    }finally{fs.rmSync(staging,{recursive:true,force:true});}
  }
  if(!process.argv.includes('--check')){
    for(const name of ['compare','composite','conjure','identify','mogrify','montage','stream']){const f=path.join(target,name+'.exe');if(fs.existsSync(f))fs.unlinkSync(f);}
    fs.copyFileSync(path.join(root,'config/imagemagick-policy.xml'),path.join(target,'policy.xml'));
  }
  console.log('ImageMagick '+version+' SHA256 verified.');
  if(!process.argv.includes('--native-only')){const pkg=JSON.parse(fs.readFileSync(path.join(desktop,'node_modules/electron/package.json'),'utf8'));if(pkg.version!=='45.2.3')throw Error('Electron 版本不匹配');console.log('Electron '+pkg.version+' verified.');}
}
main().catch(e=>{console.error(e.message);process.exitCode=1;});
