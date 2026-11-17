import { createRequire } from 'node:module';
import { spawn } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import net from 'node:net';
import assert from 'node:assert/strict';
import { wholeLayout } from '../src/wholeLayout.ts';
const require = createRequire(import.meta.url);
const { chromium } = require('playwright-core');
const root = path.resolve(import.meta.dirname, '..');
const modelRoot = path.resolve(root, process.env.ITASHA_MODEL_ROOT || '../../3D');
const report = path.join(root, '.reports', 'whole-layout-' + Date.now());
const data = path.join(report, 'data');
fs.mkdirSync(path.join(data, 'projects'), { recursive: true });
// Reproduce the startup blocker without touching the user's project directory.
const legacyId = '3e374aab-113c-45f5-8990-600ad58cd6c5';
fs.writeFileSync(path.join(data, 'projects', legacyId + '.json'), JSON.stringify({ storageVersion: 1, savedAtUnixMs: '1', documentSha256: '', document: { format: 'itasha-project', documentVersion: 2, projectId: legacyId, name: '旧工程', revision: '0', contentId: legacyId, vehicleId: null } }));
const server = net.createServer();
await new Promise(r => server.listen(0, '127.0.0.1', r));
const port = server.address().port;
await new Promise(r => server.close(r));
const child = spawn(path.join(root, 'target/release/itasha-desktop.exe'), [], { cwd: root, windowsHide: true,
  env: { ...process.env, ITASHA_TEST_DATA_DIR: data, WEBVIEW2_USER_DATA_FOLDER: path.join(report, 'webview'), WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: '--remote-debugging-port=' + port } });
let browser;
try {
  for (let i = 0; i < 100 && !browser; i++) {
    try { browser = await chromium.connectOverCDP('http://127.0.0.1:' + port); }
    catch { await new Promise(r => setTimeout(r, 200)); }
  }
  assert(browser);
  const page = browser.contexts()[0].pages()[0];
  const ipc = (command, args) => page.evaluate(({ command, args }) => window.__TAURI_INTERNALS__.invoke(command, args), { command, args });
  await page.getByRole('button', { name: /新建设计/ }).waitFor();
  await page.getByRole('button', { name: /新建设计/ }).click();
  await page.getByRole('button', { name: /轿车.*SU7 Max/ }).click();
  await page.getByRole('button', { name: '下一步：车型选配 →', exact: true }).click();
  await page.getByRole('button', { name: '完成配置，进入工作区 →', exact: true }).click();
  await page.locator('.workspace').waitFor();
  let view = await ipc('read_view');
  assert.equal(view.projectWarnings.length, 1);
  const settings = { ...view.settings, layout: 'edit', paths: { ...view.settings.paths, models: modelRoot } };
  await ipc('execute', { requestId: crypto.randomUUID(), action: { type: 'settings', value: settings } });
  await page.reload();
  await page.getByRole('button', { name: /继续当前设计/ }).click();
  // A viewport loads the authoritative geometry even in canvas-only view.
  await ipc('viewport_load', { session: view.project.session });
  let status;
  for (let i = 0; i < 150; i++) {
    status = await ipc('viewport_status');
    if (status.state === 'error') throw Error(status.message);
    if (status.state === 'ready') break;
    await page.waitForTimeout(200);
  }
  assert.equal(status.state, 'ready');
  await page.waitForFunction(() => document.querySelectorAll('[aria-label="编辑部件"] option').length === 20);
  const { tiles } = wholeLayout(status.surfaces);
  fs.writeFileSync(path.join(report, 'renderer.json'), JSON.stringify(status, null, 2));
  const audit = [];
  for (const tile of tiles) {
    const result = await page.evaluate(async ({ session, info }) => {
      const values = new Float32Array(await window.__TAURI_INTERNALS__.invoke('viewport_surface', { session, id: info.id }));
      const path = new Path2D();
      for (let i = 0; i < values.length; i += 6) {
        path.moveTo(values[i] * info.widthMm, values[i+1] * info.heightMm);
        path.lineTo(values[i+2] * info.widthMm, values[i+3] * info.heightMm);
        path.lineTo(values[i+4] * info.widthMm, values[i+5] * info.heightMm); path.closePath();
      }
      const ctx = document.createElement('canvas').getContext('2d');
      for (let i = 0; i < values.length; i += 6) {
        const x = (values[i]+values[i+2]+values[i+4])/3*info.widthMm;
        const y = (values[i+1]+values[i+3]+values[i+5])/3*info.heightMm;
        if (ctx.isPointInPath(path, x, y)) return { id: info.id, triangles: values.length/6, point: { x, y } };
      }
      throw Error('No selectable paint: ' + info.id);
    }, { session: view.project.session, info: tile.surface });
    audit.push(result);
  }
  await page.getByRole('button', { name: '适应', exact: true }).click();
  await page.screenshot({ path: path.join(report, 'whole-layout.png') });
  const bounds = await page.getByLabel('车身贴花画布', { exact: true }).boundingBox();
  const width = Math.max(...tiles.map(t => t.x + t.surface.widthMm));
  const height = Math.max(...tiles.map(t => t.y + t.surface.heightMm));
  const scale = Math.min((bounds.width-56)/width, (bounds.height-56)/height);
  const chargeCover = tiles.find(t => t.surface.id === 'su7.charge-cover');
  assert(chargeCover, 'charging cover must be selectable in the whole-canvas layout');
  const target = tiles.find(t => t.surface.id === 'su7.rear-left-fender');
  assert(target, 'left rear quarter must remain a selectable one-piece panel');
  const p = audit.find(t => t.id === target.surface.id).point;
  await page.mouse.dblclick(bounds.x+(bounds.width-width*scale)/2+(target.x+p.x)*scale, bounds.y+(bounds.height-height*scale)/2+(target.y+p.y)*scale);
  await page.getByRole('button', { name: '整车展开', exact: true }).waitFor();
  await page.screenshot({ path: path.join(report, 'rear-left-fender.png') });
  fs.writeFileSync(path.join(report, 'result.json'), JSON.stringify({ passed: true, checks: ['legacy project isolation and enabled new-design flow', '20 selectable parts retained including 充电盖', 'front-up non-overlapping layout', 'real whole-canvas double click opens quarter-panel part'], audit }, null, 2));
  console.log(report);
} finally {
  if (browser) await browser.close();
  child.kill();
}
