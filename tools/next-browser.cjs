const {chromium}=require('../desktop/node_modules/playwright-core');
const fs=require('node:fs'),readline=require('node:readline');
(async()=>{
 const browser=await chromium.connectOverCDP('http://127.0.0.1:9233');
 const context=browser.contexts()[0],page=context.pages().find(p=>p.url().startsWith('http://127.0.0.1:'))||context.pages()[0];
 const errors=[];page.on('pageerror',e=>errors.push(e.stack));page.on('console',m=>{if(m.type()==='error')errors.push(m.text().slice(0,2000));});
 console.log('READY '+JSON.stringify({url:page.url()}));
 const rl=readline.createInterface({input:process.stdin,crlfDelay:Infinity});
 for await(const line of rl){try{const cmd=JSON.parse(line);if(cmd.close){await browser.close();break;}const result=await eval(`(async()=>{${cmd.code}\n})()`);console.log('RESULT '+JSON.stringify(result));}catch(e){console.log('ERROR '+e.stack);}}
})().catch(e=>{console.error(e);process.exitCode=1;});
