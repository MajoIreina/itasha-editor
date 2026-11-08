// Run the existing detailed artwork/real-vehicle regression in a hidden,
// isolated Electron session. No test projects are written to user projects.
const { _electron: electron } = require('../desktop/node_modules/playwright-core');
const fs = require('node:fs');
const path = require('node:path');
const net = require('node:net');
const { spawn } = require('node:child_process');
const root = path.resolve(__dirname, '..');
const data = fs.mkdtempSync(path.join(require('node:os').tmpdir(), 'itasha-full-'));
const output = path.join(__dirname, 'acceptance', 'electron-full');
fs.mkdirSync(output, { recursive: true });
async function freePort() { return new Promise(resolve => { const s = net.createServer(); s.listen(0, '127.0.0.1', () => { const port = s.address().port; s.close(() => resolve(port)); }); }); }
let app;
(async () => {
  const port = await freePort();
  app = await electron.launch({ executablePath: path.join(root, 'desktop/electron/痛车编辑器.exe'), args: ['--itasha-debug=' + port], cwd: root, env: { ...process.env, ITASHA_ROOT: root, ITASHA_DATA_DIR: data, ITASHA_PROFILE_DIR: path.join(data, 'profile'), ITASHA_PORT: '0', ITASHA_TEST_MODE: '1' }, timeout: 60000 });
  const page = await app.firstWindow(); await page.waitForFunction(() => window.studio);
  await app.evaluate(({BrowserWindow})=>{const w=BrowserWindow.getAllWindows()[0];w.setPosition(-30000,-30000,false);w.showInactive();});
  await new Promise((resolve, reject) => {
    const child = spawn(process.execPath, [path.join(__dirname, 'artwork-transform-acceptance.cjs')], { cwd: root, env: { ...process.env, STUDIO_CDP: 'http://127.0.0.1:' + port, STUDIO_RUNTIME: 'Electron + Rust', STUDIO_VERIFICATION: path.join(output, 'verification.json') }, stdio: 'inherit', windowsHide: true });
    child.on('error', reject); child.on('exit', code => code === 0 ? resolve() : reject(new Error('Detailed editor regression failed: ' + code)));
  });
  console.log('Full Electron regression passed; isolated data: ' + data);
})().catch(e => { console.error(e); process.exitCode = 1; }).finally(async () => { if (app) await app.close().catch(() => {}); });
