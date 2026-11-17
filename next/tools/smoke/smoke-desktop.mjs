import { createRequire } from 'node:module';
import { spawn } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import assert from 'node:assert/strict';
import net from 'node:net';
import { checkDoorCrossDisplay } from './check-door-cross-display.mjs';
import { checkOrientationTransport } from './check-orientation-transport.mjs';
import { checkEditorProperties } from './check-editor-properties.mjs';
const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..');
const modelRoot = path.resolve(root, process.env.ITASHA_MODEL_ROOT || '../../3D');
const require = createRequire(import.meta.url);
const { chromium } = require('playwright-core');
const { PNG } = require('pngjs');
const reportDir = path.join(root, '.reports', 'smoke-' + Date.now());
fs.mkdirSync(reportDir, { recursive: true });
// A synthetic unsupported project exercises warning placement without touching
// any real user project or depending on the machine's configured data folder.
const smokeDataDir = path.join(reportDir, 'data');
const legacyProjectId = crypto.randomUUID();
fs.mkdirSync(path.join(smokeDataDir, 'projects'), { recursive: true });
fs.writeFileSync(path.join(smokeDataDir, 'projects', legacyProjectId + '.json'), JSON.stringify({
  storageVersion: 1,
  savedAtUnixMs: '1',
  documentSha256: '0'.repeat(64),
  document: {
    format: 'itasha-project', documentVersion: 2, projectId: legacyProjectId,
    name: 'SMOKE_旧版本工程', revision: '0', contentId: crypto.randomUUID(),
    vehicleId: null, editor: { modelFingerprint: null, layers: [], assets: [] },
  },
}, null, 2));
const probe = net.createServer();
await new Promise(resolve => probe.listen(0, '127.0.0.1', resolve));
const port = probe.address().port;
await new Promise(resolve => probe.close(resolve));
const candidates = process.env.ITASHA_SMOKE_EXE
  ? [path.resolve(root, process.env.ITASHA_SMOKE_EXE)]
  : ['target/debug/itasha-desktop.exe', 'target/release/itasha-desktop.exe']
    .map(relative => path.resolve(root, relative))
    .filter(candidate => fs.existsSync(candidate))
    .sort((a, b) => fs.statSync(b).mtimeMs - fs.statSync(a).mtimeMs);
const executable = candidates[0];
assert(executable, 'Build the desktop app before running the desktop smoke test');
fs.writeFileSync(path.join(reportDir, 'binary.json'), JSON.stringify({ executable, bytes: fs.statSync(executable).size, modified: fs.statSync(executable).mtime.toISOString() }, null, 2));
const child = spawn(executable, [], {
  cwd: root, windowsHide: true,
  env: { ...process.env, ITASHA_TEST_DATA_DIR: smokeDataDir, WEBVIEW2_USER_DATA_FOLDER: path.join(reportDir, 'webview'), WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: '--remote-debugging-port=' + port },
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
  const waitTheme = theme => page.waitForFunction(theme => document.documentElement.dataset.theme === theme, theme);
  const selectTheme = async value => {
    await page.getByRole('button', { name: '⚙ 设置', exact: true }).click();
    await page.getByLabel('主题', { exact: true }).selectOption(value);
    await page.getByRole('button', { name: '应用设置', exact: true }).click();
    await page.getByRole('dialog', { name: '设置', exact: true }).waitFor({ state: 'hidden' });
  };
  const initialSettings = (await ipc('read_view')).settings;
  await ipc('execute', { requestId: crypto.randomUUID(), action: { type: 'settings', value: {
    ...initialSettings, paths: { ...initialSettings.paths, models: modelRoot },
  } } });
  await page.reload();
  await page.getByText('桌面主框架就绪', { exact: true }).waitFor();
  await waitTheme('dark');
  assert.equal((await ipc('read_view')).settings.theme, 'dark');
  await page.emulateMedia({ colorScheme: 'light' });
  await waitTheme('dark');
  await selectTheme('light');
  await waitTheme('light');
  assert.equal(await page.locator('body').evaluate(el => getComputedStyle(el).color), 'rgb(26, 26, 26)');
  await page.screenshot({ animations: 'disabled', path: path.join(reportDir, 'welcome-light.png') });
  await page.reload();
  await page.getByText('桌面主框架就绪', { exact: true }).waitFor();
  await waitTheme('light');
  assert.equal((await ipc('read_view')).settings.theme, 'light');
  await selectTheme('system');
  await waitTheme('light');
  await page.emulateMedia({ colorScheme: 'dark' });
  await waitTheme('dark');
  await page.emulateMedia({ colorScheme: 'light' });
  await waitTheme('light');
  await page.reload();
  await page.getByText('桌面主框架就绪', { exact: true }).waitFor();
  await waitTheme('light');
  assert.equal((await ipc('read_view')).settings.theme, 'system');
  await selectTheme('dark');
  await waitTheme('dark');
  await page.emulateMedia({ colorScheme: 'dark' });
  await page.emulateMedia({ colorScheme: 'light' });
  await waitTheme('dark');
  checks.push('dark default, light palette, persisted themes and live system changes; explicit dark ignores system');
  await page.getByRole('heading', { name: '让灵感，成为车身的一部分。', exact: true }).waitFor();
  assert.equal(await page.locator('.workspace').count(), 0);
  assert.equal(await page.getByText(/有 1 个工程未加载/).count(), 0,
    'an unsupported legacy project must not leave a persistent warning on the welcome screen');
  await page.screenshot({ animations: 'disabled', path: path.join(reportDir, 'welcome.png') });
  await page.getByRole('button', { name: /加载设计/ }).click();
  await page.getByRole('heading', { name: '设计库还是空的', exact: true }).waitFor();
  const warningDetails = page.locator('details').filter({ hasText: '未加载的工程' });
  await warningDetails.locator('summary').click();
  await page.getByText('不支持此工程版本', { exact: false }).waitFor();
  assert((await warningDetails.innerText()).includes(legacyProjectId + '.json'));
  assert.equal(await page.locator('.design-library button').count(), 0,
    'unsupported legacy files should remain diagnostics, not appear as openable designs');
  checks.push('legacy-project diagnostics stay in the design library and away from welcome page');
  await page.getByRole('button', { name: '新建设计', exact: true }).click();
  assert.equal(await page.locator('.vehicle-card').count(), 7);
  await page.getByRole('button', { name: 'SUV', exact: true }).click();
  assert.equal(await page.locator('.vehicle-card').count(), 2);
  await page.getByRole('button', { name: '全部', exact: true }).click();
  await page.getByRole('button', { name: /轿车.*SU7 Max/ }).click();
  await page.screenshot({ animations: 'disabled', path: path.join(reportDir, 'vehicles.png') });
  await page.getByRole('button', { name: '下一步：车型选配 →', exact: true }).click();
  await page.getByRole('textbox', { name: '设计名称', exact: true }).fill('未命名工程');
  await page.getByRole('button', { name: '← 返回车型', exact: true }).click();
  await page.getByRole('button', { name: '下一步：车型选配 →', exact: true }).click();
  assert.equal(await page.getByRole('textbox', { name: '设计名称', exact: true }).inputValue(), '未命名工程');
  await page.getByRole('tab', { name: '车漆', exact: true }).click();
  const redPaint = page.getByRole('button', { name: /赤霞红/ });
  const blackPaint = page.getByRole('button', { name: /曜石黑/ });
  assert.equal(await redPaint.isDisabled(), false, '赤霞红 must be selectable independently of interior');
  await redPaint.click();
  assert.equal(await redPaint.getAttribute('aria-pressed'), 'true');
  assert.equal(await page.getByRole('button', { name: /曜石黑/ }).isDisabled(), false);
  await blackPaint.click();
  assert.equal(await blackPaint.getAttribute('aria-pressed'), 'true');
  await redPaint.click();
  assert.equal(await redPaint.getAttribute('aria-pressed'), 'true');
  await page.getByRole('tab', { name: '轮毂', exact: true }).click();
  const riceWheel = page.getByRole('button', { name: /20英寸米型轮毂 长续航胎/ });
  assert.equal(await riceWheel.isDisabled(), false, '20-inch rice wheel must be selectable independently of paint');
  await riceWheel.click();
  assert.equal(await riceWheel.getAttribute('aria-pressed'), 'true');
  assert.equal(await page.getByRole('button', { name: /20英寸米型轮毂 长续航胎/ }).getAttribute('aria-pressed'), 'true');
  await page.getByRole('tab', { name: '内饰', exact: true }).click();
  await page.getByRole('button', { name: /砂陶米/ }).click();
  await page.getByRole('tab', { name: '方向盘', exact: true }).click();
  await page.getByRole('button', { name: /碳纤维运动方向盘/ }).click();
  await page.getByRole('textbox', { name: '设计名称', exact: true }).fill('\u0009');
  assert(await page.getByRole('button', { name: '完成配置，进入工作区 →', exact: true }).isDisabled());
  await page.getByRole('textbox', { name: '设计名称', exact: true }).fill('未命名工程');
  await page.screenshot({ animations: 'disabled', path: path.join(reportDir, 'configuration.png') });
  await page.getByRole('button', { name: '完成配置，进入工作区 →', exact: true }).click();
  await page.getByText('未命名工程 · 未保存', { exact: true }).waitFor();
  let view = await ipc('read_view');
  assert(view.project.dirty);
  assert.equal(view.project.vehicleId, 'su7');
  assert.equal(view.project.configuration.paint, '600061064');
  assert.equal(view.project.configuration.wheels, '600061081');
  assert.equal(view.project.configuration.interior, '600061089');
  assert.equal(view.project.configuration.steering, '600061070');
  checks.push('welcome, vehicle picker, four configuration categories, red/black paint and rice wheel selection, back navigation');
  await page.getByRole('button', { name: '← 欢迎页', exact: true }).click();
  await page.getByRole('dialog', { name: '当前设计尚未保存', exact: true }).waitFor();
  await page.getByRole('button', { name: '取消', exact: true }).click();
  await page.locator('.workspace').waitFor();
  checks.push('dirty workspace navigation requires save, discard or cancel');
  checks.push('native create and authoritative Rust snapshot');
  const renderer = await waitRenderer();
  assert.equal(renderer.stats.surfaceFingerprint, '109ba0aa146625f99b35050b2682edb6c0ad15b72e180c515c58867fa7d275d7');
  assert.equal(renderer.surfaces.length, 18, 'runtime must expose classified exterior parts plus the restored trunk');
  assert(renderer.surfaces.some(surface => surface.id === 'su7.trunk' && surface.triangles > 0),
    'restored trunk paint must reach the 2D canvas');
  const leftLower = renderer.surfaces.find(surface => surface.id === 'su7.front-left-lower-fender');
  assert(leftLower && leftLower.triangles === 1770, 'left lower fender must mirror all 1770 right-part triangles');
  assert(!renderer.surfaces.some(surface => ['su7.left-sill', 'su7.right-sill'].includes(surface.id)),
    'unclassified sill geometry must stay out of the paint surfaces');
  checks.push('runtime loads v24 mirrored map: 18 parts incl. trunk, left lower fender mirrored from the right, sills excluded');
  assert(renderer.nativeVisible, 'GPU child must be visible through its actual HWND');
  assert(renderer.nativeFrontmost, 'GPU child must cover WebView siblings at nine viewport sample points');
  assert.equal(renderer.state, 'ready', JSON.stringify(renderer));
  assert(renderer.stats.triangles > 500000);
  assert(renderer.stats.textures > 10);
  assert(renderer.adapter.length > 0);
  fs.writeFileSync(path.join(reportDir, 'renderer.json'), JSON.stringify(renderer, null, 2));
  fs.copyFileSync(path.join(reportDir, 'data/native-viewport.png'), path.join(reportDir, 'native-su7-custom.png'));
  // Real settings persistence and scaled GPU targets; project data stays unchanged.
  const originalProject = structuredClone((await ipc('read_view')).project);
  const renderDefaults = structuredClone((await ipc('read_view')).settings);
  assert.equal(renderer.antialiasing, 'fxaa');
  assert.equal(renderer.skybox, true);
  assert.equal(renderer.sceneTimeMinutes, 840);
  const aaSettings=structuredClone(renderDefaults);aaSettings.performance.antialiasing='off';
  await ipc('execute',{requestId:crypto.randomUUID(),action:{type:'settings',value:aaSettings}});
  await waitRenderer(s=>s.antialiasing==='off');
  fs.copyFileSync(path.join(reportDir,'data/native-viewport.png'),path.join(reportDir,'antialiasing-off.png'));
  await ipc('execute',{requestId:crypto.randomUUID(),action:{type:'settings',value:renderDefaults}});
  await waitRenderer(s=>s.antialiasing==='fxaa');
  fs.copyFileSync(path.join(reportDir,'data/native-viewport.png'),path.join(reportDir,'antialiasing-fxaa.png'));
  assert(!fs.readFileSync(path.join(reportDir,'antialiasing-off.png')).equals(fs.readFileSync(path.join(reportDir,'antialiasing-fxaa.png'))),'FXAA must affect rendered pixels');

  // Read the native renderer's GPU readback, not a WebView screenshot (which
  // cannot contain the sibling HWND). Temporal captures are saved on the final
  // accumulation frame, so status must report convergence before reading them.
  const aaBaseline = PNG.sync.read(fs.readFileSync(path.join(reportDir, 'antialiasing-off.png')));
  let stablePick;
  for (const ny of [0.55, 0.65, 0.45, 0.75]) {
    for (const nx of [0.5, 0.4, 0.6, 0.3, 0.7]) {
      const point = { x: renderer.width * nx, y: renderer.height * ny };
      const hit = await ipc('viewport_probe', point);
      if (hit) { stablePick = { point, hit }; break; }
    }
    if (stablePick) break;
  }
  assert(stablePick, 'AA regression requires a visible paint surface to verify stable picking');
  const aaReport = [];
  for (const mode of ['taa', 'taa2x', 'ssaa']) {
    const settings = structuredClone(renderDefaults);
    settings.performance.antialiasing = mode;
    const before = await ipc('viewport_status');
    const started = performance.now();
    await ipc('execute', { requestId: crypto.randomUUID(), action: { type: 'settings', value: settings } });
    const temporal = mode !== 'ssaa';
    const factor = mode === 'taa' ? 1 : 2;
    const status = await waitRenderer(s => s.antialiasing === mode && s.visible && s.nativeVisible
      && s.frames > before.frames && !s.temporalPending && s.temporalSamples === (temporal ? 8 : 0));
    const settleMs = performance.now() - started;
    assert.equal(status.renderWidth, Math.max(1, Math.floor(status.width * settings.performance.renderScale / 100)) * factor);
    assert.equal(status.renderHeight, Math.max(1, Math.floor(status.height * settings.performance.renderScale / 100)) * factor);
    assert.equal((await ipc('read_view')).settings.performance.antialiasing, mode);
    const capture = path.join(reportDir, `antialiasing-${mode}.png`);
    fs.copyFileSync(path.join(reportDir, 'data/native-viewport.png'), capture);
    const image = PNG.sync.read(fs.readFileSync(capture));
    assert.equal(image.width, status.width, 'supersampling must resolve to display width');
    assert.equal(image.height, status.height, 'supersampling must resolve to display height');
    assert.equal(image.width, aaBaseline.width);
    assert.equal(image.height, aaBaseline.height);
    let changedPixels = 0, low = 255, high = 0, redPixels = 0;
    for (let i = 0; i < image.data.length; i += 4) {
      const r = image.data[i], g = image.data[i + 1], b = image.data[i + 2];
      low = Math.min(low, r, g, b); high = Math.max(high, r, g, b);
      if (r > 50 && r > g * 1.25 && r > b * 1.2) redPixels++;
      if (Math.max(Math.abs(r - aaBaseline.data[i]), Math.abs(g - aaBaseline.data[i + 1]), Math.abs(b - aaBaseline.data[i + 2])) > 2) changedPixels++;
    }
    assert(high - low > 80 && redPixels > 100, `${mode} must retain the visible red vehicle, not output a blank resolve`);
    assert(changedPixels > 100, `${mode} must affect actual rendered pixels`);
    assert.deepEqual(await ipc('viewport_probe', stablePick.point), stablePick.hit,
      `${mode} jitter and supersampling must not change paint picking coordinates`);
    await new Promise(resolve => setTimeout(resolve, 350));
    const idleStatus = await ipc('viewport_status');
    assert.equal(idleStatus.frames, status.frames, `${mode} must stop rendering after convergence`);
    aaReport.push({ mode, renderWidth: status.renderWidth, renderHeight: status.renderHeight,
      outputWidth: image.width, outputHeight: image.height, temporalSamples: status.temporalSamples,
      submittedFrames: status.frames - before.frames, settleMs, lastSubmitCpuMs: status.lastSubmitCpuMs,
      changedPixels, redPixels });
    if (temporal) {
      // Reset goes through the native host; picking and temporal convergence
      // must still work after the host requests another camera frame.
      await ipc('viewport_reset');
      const reset = await waitRenderer(s => s.frames > status.frames && !s.temporalPending && s.temporalSamples === 8);
      assert.deepEqual(await ipc('viewport_probe', stablePick.point), stablePick.hit);
      await new Promise(resolve => setTimeout(resolve, 250));
      assert.equal((await ipc('viewport_status')).frames, reset.frames, `${mode} camera reset must converge and idle`);
    }
    if (mode === 'taa') {
      const midnightSettings = structuredClone(settings);
      midnightSettings.scene.timeMinutes = 0;
      const previous = await ipc('viewport_status');
      await ipc('execute', { requestId: crypto.randomUUID(), action: { type: 'settings', value: midnightSettings } });
      await waitRenderer(s => s.frames > previous.frames && s.sceneTimeMinutes === 0
        && !s.temporalPending && s.temporalSamples === 8);
      const darkCapture = path.join(reportDir, 'antialiasing-taa-midnight.png');
      fs.copyFileSync(path.join(reportDir, 'data/native-viewport.png'), darkCapture);
      const dark = PNG.sync.read(fs.readFileSync(darkCapture));
      const luminance = png => {
        let sum = 0;
        for (let i = 0; i < png.data.length; i += 4) sum += png.data[i] * .2126 + png.data[i + 1] * .7152 + png.data[i + 2] * .0722;
        return sum / (png.width * png.height);
      };
      assert(luminance(dark) < luminance(image) * .85, 'TAA history must not retain the daylight scene after switching to midnight');
      await ipc('execute', { requestId: crypto.randomUUID(), action: { type: 'settings', value: settings } });
      await waitRenderer(s => s.sceneTimeMinutes === settings.scene.timeMinutes && !s.temporalPending && s.temporalSamples === 8);
    }
    if (mode === 'taa2x') {
      const halfScale = structuredClone(settings);
      halfScale.performance.renderScale = 50;
      await ipc('execute', { requestId: crypto.randomUUID(), action: { type: 'settings', value: halfScale } });
      const resized = await waitRenderer(s => s.renderWidth === Math.max(1, Math.floor(s.width / 2)) * 2
        && s.renderHeight === Math.max(1, Math.floor(s.height / 2)) * 2 && !s.temporalPending && s.temporalSamples === 8);
      assert.equal(resized.antialiasing, 'taa2x');
      assert.deepEqual(await ipc('viewport_probe', stablePick.point), stablePick.hit,
        'resizing temporal targets must retain display-space paint picking');
      // Splitter gestures persist React's read model, so use the actual settings
      // UI first to prevent a stale pre-IPC snapshot from restoring FXAA.
      await page.getByRole('button', { name: '⚙ 设置', exact: true }).click();
      await page.getByRole('tab', { name: '性能', exact: true }).click();
      await page.getByLabel('抗锯齿', { exact: true }).selectOption('taa2x');
      await page.getByLabel('渲染分辨率', { exact: true }).selectOption('100');
      await page.getByRole('button', { name: '应用设置', exact: true }).click();
      await page.getByRole('dialog', { name: '设置', exact: true }).waitFor({ state: 'hidden' });
      await waitRenderer(s => s.antialiasing === 'taa2x' && s.renderWidth === s.width * 2
        && s.renderHeight === s.height * 2 && !s.temporalPending && s.temporalSamples === 8);
      const aaSplitter = page.getByRole('separator', { name: '调整双视图比例', exact: true });
      const savedPercent = Number(await aaSplitter.getAttribute('aria-valuenow'));
      const vertical = renderDefaults.workspace.direction === 'vertical';
      const dragTemporalSplit = async percent => {
        const handle = await aaSplitter.boundingBox();
        const workspace = await page.locator('.workspace').boundingBox();
        const start = { x: handle.x + handle.width / 2, y: vertical ? handle.y + 4 : handle.y + handle.height / 2 };
        const end = vertical
          ? { x: start.x, y: workspace.y + 16 + (workspace.height - 32) * percent / 100 }
          : { x: workspace.x + 16 + (workspace.width - 32) * percent / 100, y: start.y };
        await page.mouse.move(start.x, start.y); await page.mouse.down();
        for (let step = 1; step <= 5; step++) {
          await page.mouse.move(start.x + (end.x - start.x) * step / 5, start.y + (end.y - start.y) * step / 5);
          const during = await ipc('viewport_status');
          assert.equal(during.visible, true, 'TAA 2x resize must keep the 3D viewport visible');
          assert.equal(during.nativeVisible, true, 'TAA 2x resize must keep the HWND visible');
          assert.notEqual(during.state, 'error');
        }
        await page.mouse.up();
        await page.waitForFunction(percent => Number(document.querySelector('.splitter').getAttribute('aria-valuenow')) === percent, percent);
        return waitRenderer(s => s.antialiasing === 'taa2x' && s.visible && s.nativeVisible
          && s.renderWidth === s.width * 2 && s.renderHeight === s.height * 2 && !s.temporalPending && s.temporalSamples === 8);
      };
      const dragged = await dragTemporalSplit(savedPercent === 55 ? 60 : 55);
      fs.copyFileSync(path.join(reportDir, 'data/native-viewport.png'), path.join(reportDir, 'antialiasing-taa2x-resized.png'));
      await new Promise(resolve => setTimeout(resolve, 250));
      assert.equal((await ipc('viewport_status')).frames, dragged.frames, 'resized TAA 2x targets must converge and idle');
      const restored = await dragTemporalSplit(savedPercent);
      assert.equal(restored.width, status.width);
      assert.equal(restored.height, status.height);
      checks.push('TAA 2x split dragging keeps 3D visible, recreates supersampled history targets, converges and restores the original layout');
    }
  }
  assert(!fs.readFileSync(path.join(reportDir, 'antialiasing-taa.png')).equals(fs.readFileSync(path.join(reportDir, 'antialiasing-taa2x.png'))),
    'TAA 2x must produce its own supersampled resolve');
  assert(!fs.readFileSync(path.join(reportDir, 'antialiasing-taa2x.png')).equals(fs.readFileSync(path.join(reportDir, 'antialiasing-ssaa.png'))),
    'TAA 2x history accumulation must differ from spatial SSAA');
  fs.writeFileSync(path.join(reportDir, 'antialiasing.json'), JSON.stringify({
    note: 'Convergence elapsed and CPU submit times are not GPU frame timings. 2x means twice width and height.', modes: aaReport,
  }, null, 2));
  await ipc('execute', { requestId: crypto.randomUUID(), action: { type: 'settings', value: renderDefaults } });
  await waitRenderer(s => s.antialiasing === 'fxaa' && !s.temporalPending && s.temporalSamples === 0);
  checks.push('TAA, TAA 2x and SSAA: actual nonblank GPU pixels, supersampled dimensions, stable paint picking, bounded temporal convergence and idle rendering');

  await page.getByRole('button', { name: '⚙ 设置', exact: true }).click();
  await page.getByRole('tab', { name: '性能', exact: true }).click();
  assert.deepEqual(await page.getByLabel('抗锯齿', { exact: true }).locator('option').evaluateAll(options => options.map(option => option.value)),
    ['off', 'fxaa', 'taa', 'taa2x', 'ssaa'], 'all implemented AA modes must be available in settings');
  await page.getByLabel('抗锯齿', {exact:true}).selectOption('off');
  await page.getByLabel('渲染分辨率', {exact:true}).selectOption('50');
  await page.getByLabel('目标帧率', {exact:true}).selectOption('30');
  await page.getByRole('tab', { name:'场景',exact:true }).click();
  await page.getByRole('button', {name:'午夜',exact:true}).click();
  await page.screenshot({path:path.join(reportDir,'scene-settings.png')});
  await page.getByRole('button', {name:'应用设置',exact:true}).click();
  let rendered = await waitRenderer(s=>s.visible && s.sceneTimeMinutes===0 && s.antialiasing==='off' && s.renderWidth===Math.max(1,Math.floor(s.width/2)) && s.renderHeight===Math.max(1,Math.floor(s.height/2)));
  assert.equal(rendered.targetFps,30);
  fs.copyFileSync(path.join(reportDir,'data/native-viewport.png'),path.join(reportDir,'sky-midnight.png'));
  assert.equal((await ipc('read_view')).settings.scene.timeMinutes,0);
  assert.deepEqual((await ipc('read_view')).project,originalProject);
  const applyScene = async (minutes, skybox=true) => {
    const settings=(await ipc('read_view')).settings;
    settings.scene={timeMinutes:minutes,skybox};
    await ipc('execute',{requestId:crypto.randomUUID(),action:{type:'settings',value:settings}});
    await waitRenderer(s=>s.sceneTimeMinutes===minutes && s.skybox===skybox);
  };
  const pacing=[];
  for (const fps of [30,60]) {
    const settings=(await ipc('read_view')).settings; settings.performance.targetFps=fps;
    await ipc('execute',{requestId:crypto.randomUUID(),action:{type:'settings',value:settings}});
    await waitRenderer(s=>s.targetFps===fps);
    const begin=await ipc('viewport_status'), start=performance.now(), samples=[];
    while(performance.now()-start<1100) {
      await ipc('viewport_reset');
      const frame=await ipc('viewport_status');
      if(frame.frames!==samples.at(-1)?.frame) samples.push({frame:frame.frames,cpuMs:frame.lastSubmitCpuMs});
      await new Promise(resolve=>setTimeout(resolve,3));
    }
    const end=await ipc('viewport_status');
    pacing.push({targetFps:fps,elapsedMs:performance.now()-start,submittedFrames:end.frames-begin.frames,samples});
  }
  fs.writeFileSync(path.join(reportDir,'render-pacing.json'),JSON.stringify(pacing,null,2));
  await applyScene(720);
  fs.copyFileSync(path.join(reportDir,'data/native-viewport.png'),path.join(reportDir,'sky-noon.png'));
  assert(!fs.readFileSync(path.join(reportDir,'sky-midnight.png')).equals(fs.readFileSync(path.join(reportDir,'sky-noon.png'))),'time must change actual rendered pixels');
  await applyScene(1080);
  fs.copyFileSync(path.join(reportDir,'data/native-viewport.png'),path.join(reportDir,'sky-sunset.png'));
  await applyScene(720,false);
  fs.copyFileSync(path.join(reportDir,'data/native-viewport.png'),path.join(reportDir,'sky-disabled.png'));
  // Restore through UI so React's read model also observes the test's direct IPC changes.
  await page.getByRole('button', {name:'⚙ 设置',exact:true}).click();
  await page.getByRole('tab', {name:'性能',exact:true}).click();
  await page.getByLabel('抗锯齿',{exact:true}).selectOption(renderDefaults.performance.antialiasing);
  await page.getByLabel('渲染分辨率',{exact:true}).selectOption(String(renderDefaults.performance.renderScale));
  await page.getByLabel('目标帧率',{exact:true}).selectOption(String(renderDefaults.performance.targetFps));
  await page.getByRole('tab', {name:'场景',exact:true}).click();
  await page.getByLabel('天空盒',{exact:true}).check();
  await page.getByLabel('具体时间',{exact:true}).fill('14:00');
  await page.getByRole('button', {name:'应用设置',exact:true}).click();
  await page.getByRole('dialog',{name:'设置',exact:true}).waitFor({state:'hidden'});
  await waitRenderer(s=>s.sceneTimeMinutes===840 && s.antialiasing==='fxaa' && s.renderWidth===s.width && s.renderHeight===s.height);
  assert.deepEqual((await ipc('read_view')).project,originalProject);
  checks.push('FXAA defaults, 50% GPU targets, 30 FPS pacing configuration, skybox toggle and midnight/noon/sunset lighting; settings persisted without changing project');
  // Real 2D gestures commit exactly once; projections remain on their selected outer part.
  const waitProject = async predicate => {
    for (let i = 0; i < 100; i++) {
      const current = (await ipc('read_view')).project;
      if (predicate(current)) return current;
      await new Promise(resolve => setTimeout(resolve, 100));
    }
    throw Error('Canvas project state timed out');
  };
  assert.equal(new Set(renderer.surfaces.map(s => s.id)).size, renderer.surfaces.length);
  assert(!renderer.surfaces.some(surface => surface.id === 'su7.rear-trim'), 'fully occluded rear trim must not be editable');
  assert(renderer.surfaces.every(surface => surface.triangles > 0), 'empty exterior masks must not be exposed');
  for (const id of ['su7.front-left-lower-fender', 'su7.front-right-lower-fender']) {
    assert(renderer.surfaces.find(surface => surface.id === id)?.triangles > 0, `${id} component mask missing`);
  }
  const maskBytes = await page.evaluate(async session => (await window.__TAURI_INTERNALS__.invoke('viewport_surface', { session, id: 'su7.front-left-door' })).byteLength, view.project.session);
  assert(maskBytes > 1000, 'native binary surface mask missing');
  await page.getByRole('button', { name: '2D 画布', exact: true }).click();
  const maskAudit = [];
  for (const surface of renderer.surfaces) {
    const audit = await page.evaluate(async ({session, surface}) => {
      const values = new Float32Array(await window.__TAURI_INTERNALS__.invoke('viewport_surface', {session, id:surface.id}));
      const canvas = document.createElement('canvas'); canvas.width=900;canvas.height=600;
      const ctx=canvas.getContext('2d');ctx.fillStyle='#202020';ctx.fillRect(0,0,900,600);
      const scale=Math.min(860/surface.widthMm,540/surface.heightMm);
      ctx.translate((900-surface.widthMm*scale)/2,35+(540-surface.heightMm*scale)/2);
      ctx.scale(surface.widthMm*scale,surface.heightMm*scale);
      const path=new Path2D();
      for(let i=0;i<values.length;i+=6){path.moveTo(values[i],values[i+1]);path.lineTo(values[i+2],values[i+3]);path.lineTo(values[i+4],values[i+5]);path.closePath();}
      ctx.save();ctx.resetTransform();
      const maskCheck=surface.id==='su7.front-left-door'||surface.id==='su7.front-right-door'?(()=>{
        const inside=(x,y)=>ctx.isPointInPath(path,x,y);
        const samples=[];
        for(let i=0;i<values.length;i+=6){
          const ax=values[i],ay=values[i+1],bx=values[i+2],by=values[i+3],cx=values[i+4],cy=values[i+5];
          const area=Math.abs((bx-ax)*(cy-ay)-(by-ay)*(cx-ax))*.5;
          const x=(ax+bx+cx)/3,y=(ay+by+cy)/3;
          if(area>1e-10&&inside(x,y))samples.push({x,y,area});
        }
        samples.sort((a,b)=>b.area-a.area);
        const primary=samples[0],secondary=primary&&samples.find(p=>Math.hypot(p.x-primary.x,p.y-primary.y)>.04);
        let bounds=[Infinity,Infinity,-Infinity,-Infinity];
        for(let i=0;i<values.length;i+=2){bounds[0]=Math.min(bounds[0],values[i]);bounds[1]=Math.min(bounds[1],values[i+1]);bounds[2]=Math.max(bounds[2],values[i]);bounds[3]=Math.max(bounds[3],values[i+1]);}
        const raster=document.createElement('canvas');raster.width=512;raster.height=512;
        const rasterCtx=raster.getContext('2d');
        rasterCtx.setTransform(512/(bounds[2]-bounds[0]),0,0,512/(bounds[3]-bounds[1]),-bounds[0]*512/(bounds[2]-bounds[0]),-bounds[1]*512/(bounds[3]-bounds[1]));
        rasterCtx.fill(path);
        const pixels=rasterCtx.getImageData(0,0,512,512).data;
        const seen=new Uint8Array(512*512),queue=new Int32Array(512*512);let aperture=false;
        for(let start=0;start<seen.length&&!aperture;start++){
          if(seen[start]||pixels[start*4+3]!==0)continue;
          let head=0,tail=0,touchesEdge=false;queue[tail++]=start;seen[start]=1;
          while(head<tail){
            const at=queue[head++],x=at%512,y=(at/512)|0;
            if(x===0||y===0||x===511||y===511)touchesEdge=true;
            for(const next of [x>0?at-1:-1,x<511?at+1:-1,y>0?at-512:-1,y<511?at+512:-1]){
              if(next>=0&&!seen[next]&&pixels[next*4+3]===0){seen[next]=1;queue[tail++]=next;}
            }
          }
          aperture=!touchesEdge&&tail>=8;
        }
        return {primaryPaintVisible:!!primary,secondaryPaintVisible:!!secondary,interiorApertureExcluded:aperture};
      })():null;
      ctx.restore();
      ctx.fillStyle='#abb8c6';ctx.fill(path);ctx.resetTransform();ctx.fillStyle='white';ctx.font='16px Segoe UI';ctx.fillText(surface.id,20,24);
      return {id:surface.id,triangles:values.length/6,finite:[...values].every(Number.isFinite),maskCheck,png:canvas.toDataURL('image/png').split(',')[1]};
    },{session:view.project.session,surface});
    assert(audit.finite && audit.triangles>0,'invalid mask: '+surface.id);
    fs.writeFileSync(path.join(reportDir,'mask-'+surface.id+'.png'),Buffer.from(audit.png,'base64'));
    delete audit.png; maskAudit.push(audit);
    if(audit.maskCheck)assert(Object.values(audit.maskCheck).every(Boolean),'door paint mask/aperture regression: '+JSON.stringify(audit));
  }
  fs.writeFileSync(path.join(reportDir,'mask-audit.json'),JSON.stringify(maskAudit,null,2));
  await page.getByLabel('编辑部件', { exact: true }).selectOption('su7.front-left-door');
  await page.getByRole('button', { name: /□.*矩形/ }).click();
  let canvasProject = await waitProject(p => p.editor.layers.length === 1);
  const initialLayer = canvasProject.editor.layers[0];
  assert(initialLayer.placement.crossSurface, 'new artwork should default to adjacent paint coverage');
  const initialRevision = BigInt(canvasProject.revision);
  const canvasBox = await page.getByLabel('车身贴花画布', { exact: true }).boundingBox();
  const surfaceInfo = renderer.surfaces.find(s => s.id === 'su7.front-left-door');
  const fit = Math.min(Math.max(1, canvasBox.width - 56) / surfaceInfo.widthMm, Math.max(1, canvasBox.height - 56) / surfaceInfo.heightMm);
  const chartOrigin = {
    x: canvasBox.x + (canvasBox.width - surfaceInfo.widthMm * fit) / 2,
    y: canvasBox.y + (canvasBox.height - surfaceInfo.heightMm * fit) / 2,
  };
  const center = {
    x: chartOrigin.x + initialLayer.placement.xMm * fit,
    y: chartOrigin.y + initialLayer.placement.yMm * fit,
  };
  const dragDelta = { x: -20, y: -12 };
  await ipc('viewport_transport',{session:canvasProject.session,layer:initialLayer,surfaceId:initialLayer.placement.surfaceId,point:[initialLayer.placement.xMm-20/fit,initialLayer.placement.yMm-12/fit],offset:[0,0]});
  await page.mouse.move(center.x, center.y); await page.mouse.down();
  await page.mouse.move(center.x + dragDelta.x, center.y + dragDelta.y, { steps: 8 }); await page.mouse.up();
  canvasProject = await waitProject(p => BigInt(p.revision) > initialRevision);
  assert.equal(BigInt(canvasProject.revision), initialRevision + 1n);
  assert(Math.abs(canvasProject.editor.layers[0].placement.xMm - initialLayer.placement.xMm - dragDelta.x / fit) < 2, JSON.stringify({before:initialLayer.placement,after:canvasProject.editor.layers[0].placement,fit,dragDelta}));
  const movedLayer = structuredClone(canvasProject.editor.layers[0]);
  const cancelledRevision = canvasProject.revision;
  await page.mouse.move(center.x + dragDelta.x, center.y + dragDelta.y); await page.mouse.down();
  await page.mouse.move(center.x + dragDelta.x + 20, center.y + dragDelta.y + 10, { steps: 4 }); await page.keyboard.press('Escape'); await page.mouse.up();
  assert.equal((await ipc('read_view')).project.revision, cancelledRevision);
  await page.getByRole('button', { name: '撤销', exact: true }).click();
  canvasProject = await waitProject(p => p.editor.layers[0].placement.xMm === initialLayer.placement.xMm);
  await page.getByRole('button', { name: '重做', exact: true }).click();
  await waitProject(p => p.editor.layers[0].placement.xMm === movedLayer.placement.xMm);
  const corner = { x: center.x + dragDelta.x + movedLayer.placement.widthMm * fit / 2, y: center.y + dragDelta.y + movedLayer.placement.heightMm * fit / 2 };
  const beforeResize = (await ipc('read_view')).project.revision;
  await page.mouse.move(corner.x, corner.y); await page.mouse.down();
  await page.mouse.move(corner.x + 20, corner.y + 12, { steps: 5 }); await page.mouse.up();
  canvasProject = await waitProject(p => p.revision !== beforeResize);
  assert(canvasProject.editor.layers[0].placement.widthMm > movedLayer.placement.widthMm + 10);
  const resized = canvasProject.editor.layers[0];
  const knob = { x: center.x + dragDelta.x + 10, y: center.y + dragDelta.y + 6 - resized.placement.heightMm * fit / 2 - 22 };
  await page.mouse.move(knob.x, knob.y); await page.mouse.down();
  await page.mouse.move(knob.x + 30, knob.y + 12, { steps: 5 }); await page.mouse.up();
  await waitProject(p => Math.abs(p.editor.layers[0].placement.rotationDeg) > 5);
  await page.getByLabel('图层旋转', { exact: true }).fill('35');
  await page.getByLabel('图层旋转', { exact: true }).press('Tab');
  await waitProject(p => p.editor.layers[0].placement.rotationDeg === 35);
  assert.equal((await ipc('read_view')).project.editor.layers[0].name,'图层1');
  await page.getByRole('button', { name: '重命名', exact: true }).click();
  await page.getByLabel('重命名图层',{exact:true}).fill('车门贴花');
  await page.getByLabel('重命名图层',{exact:true}).press('Enter');
  await waitProject(p=>p.editor.layers[0].name==='车门贴花');
  await page.getByRole('button', { name: '重命名', exact: true }).click();
  await page.getByLabel('重命名图层',{exact:true}).fill('取消名称');
  await page.getByLabel('重命名图层',{exact:true}).press('Escape');
  assert.equal((await ipc('read_view')).project.editor.layers[0].name,'车门贴花');
  await page.getByRole('button', { name: '撤销', exact: true }).click();
  await waitProject(p=>p.editor.layers[0].name==='图层1');
  await page.getByRole('button', { name: '锁定图层1', exact: true }).click();
  await waitProject(p => p.editor.layers[0].locked);
  assert(await page.getByRole('button', { name: '删除图层', exact: true }).isDisabled());
  await page.getByRole('button', { name: '解锁图层1', exact: true }).click();
  await waitProject(p => !p.editor.layers[0].locked);
  await waitRenderer(s => s.paintedSurfaces.includes('su7.front-left-door'));
  assert(!(await ipc('viewport_status')).paintedSurfaces.includes('su7.front-right-door'));
  await page.getByLabel('编辑部件', { exact: true }).selectOption('su7.front-right-door');
  await page.getByLabel('插入文字内容', { exact: true }).fill('曲面贴花 SU7');
  await page.getByRole('button', { name: '＋ 插入文字', exact: true }).click();
  await waitProject(p => p.editor.layers.length === 2);
  await waitRenderer(s => s.paintedSurfaces.includes('su7.front-right-door'));
  await page.screenshot({ animations: 'disabled', path: path.join(reportDir, 'canvas-right-door.png') });
  assert.equal((await ipc('read_view')).project.editor.layers[1].name,'图层2');
  await page.getByRole('button', { name: '隐藏图层2', exact: true }).click();
  await waitProject(p => !p.editor.layers[1].visible);
  await waitRenderer(s => !s.paintedSurfaces.includes('su7.front-right-door') && s.paintedSurfaces.includes('su7.front-left-door'));
  await page.getByRole('button', { name: '显示图层2', exact: true }).click();
  await waitProject(p => p.editor.layers[1].visible);
  await page.getByRole('button', { name: '双视图', exact: true }).click();
  await waitRenderer(s => s.visible && s.nativeFrontmost && s.paintedSurfaces.length === 2 && s.frames > 0);
  await new Promise(resolve => setTimeout(resolve, 300));
  fs.copyFileSync(path.join(reportDir, 'data/native-viewport.png'), path.join(reportDir, 'native-with-decals.png'));
  await applyScene(0);
  fs.copyFileSync(path.join(reportDir,'data/native-viewport.png'),path.join(reportDir,'decals-midnight.png'));
  await applyScene(840);
  checks.push('night lighting also affects artwork while selection edges and handles remain visible');

  fs.writeFileSync(path.join(reportDir, 'canvas-renderer.json'), JSON.stringify(await ipc('viewport_status'), null, 2));
  checks.push('part masks, 2D drag single commit, Escape cancellation, undo/redo, rotation property, lock and independent left/right curved decals');
  // Exercise artwork invalidation while temporal accumulation is active. Keep
  // the UI read model current so subsequent property edits retain TAA settings.
  await page.getByRole('button', { name: '⚙ 设置', exact: true }).click();
  await page.getByRole('tab', { name: '性能', exact: true }).click();
  await page.getByLabel('抗锯齿', { exact: true }).selectOption('taa');
  await page.getByRole('button', { name: '应用设置', exact: true }).click();
  await page.getByRole('dialog', { name: '设置', exact: true }).waitFor({ state: 'hidden' });
  const temporalArtwork = await waitRenderer(s => s.antialiasing === 'taa' && s.visible
    && s.controlPoints.length === 9 && !s.temporalPending && s.temporalSamples === 8);
  const temporalLayer = (await ipc('read_view')).project.editor.layers.find(l => l.id === temporalArtwork.selectedLayer);
  assert(temporalLayer, 'TAA artwork regression requires a selected layer');
  const temporalAngle = temporalLayer.placement.rotationDeg;
  await page.getByLabel('图层旋转', { exact: true }).fill(String(temporalAngle + 5));
  await page.getByLabel('图层旋转', { exact: true }).press('Tab');
  await waitProject(p => p.editor.layers.find(l => l.id === temporalLayer.id).placement.rotationDeg === temporalAngle + 5);
  const temporalEdited = await waitRenderer(s => s.antialiasing === 'taa' && s.frames > temporalArtwork.frames
    && s.visible && s.nativeVisible && !s.temporalPending && s.temporalSamples === 8);
  fs.copyFileSync(path.join(reportDir, 'data/native-viewport.png'), path.join(reportDir, 'antialiasing-taa-artwork-edited.png'));
  await new Promise(resolve => setTimeout(resolve, 250));
  assert.equal((await ipc('viewport_status')).frames, temporalEdited.frames, 'TAA artwork property edits must converge and idle');
  await page.getByLabel('图层旋转', { exact: true }).fill(String(temporalAngle));
  await page.getByLabel('图层旋转', { exact: true }).press('Tab');
  await waitProject(p => p.editor.layers.find(l => l.id === temporalLayer.id).placement.rotationDeg === temporalAngle);
  await waitRenderer(s => s.frames > temporalEdited.frames && !s.temporalPending && s.temporalSamples === 8);
  // Native controls use the same input path as the child HWND; IPC supplies physical client coordinates.
  let nativeControls=await waitRenderer(s=>s.visible&&s.controlPoints.length===9&&s.selectedLayer);
  const nativeLayerId=nativeControls.selectedLayer;
  const beforeFocus=nativeControls.frames;
  await page.getByRole('button',{name:'定位选中素材',exact:true}).click();
  nativeControls=await waitRenderer(s=>s.frames>beforeFocus&&s.visible&&s.controlPoints.length===9&&s.selectedLayer===nativeLayerId);
  const beforeNative=(await ipc('read_view')).project;
  // Use the horizontal midpoint control: a corner also shifts the chart anchor
  // vertically, which can intentionally hit an irregular classified mask edge.
  let h=nativeControls.controlPoints[3];
  const leftWidthHandle=nativeControls.controlPoints[7];
  const widthAxis={x:h[0]-leftWidthHandle[0],y:h[1]-leftWidthHandle[1]};
  const widthLength=Math.hypot(widthAxis.x,widthAxis.y);
  assert(widthLength>1,'focused width controls must have a measurable screen span');
  // A drag relative to the projected width exercises scaling at every zoom.
  const resizeDelta={x:widthAxis.x*.1,y:widthAxis.y*.1};
  await ipc('viewport_pointer',{input:{kind:'down',x:h[0],y:h[1]}});
  await ipc('viewport_pointer',{input:{kind:'move',x:h[0]+resizeDelta.x,y:h[1]+resizeDelta.y}});
  assert.equal((await ipc('read_view')).project.revision,beforeNative.revision,'3D preview must not commit each pointer move');
  await ipc('viewport_pointer',{input:{kind:'up',x:h[0]+resizeDelta.x,y:h[1]+resizeDelta.y}});
  const afterNativeInput=(await ipc('read_view')).project;
  let nativeProject=await waitProject(p=>p.revision!==beforeNative.revision);
  fs.writeFileSync(path.join(reportDir,'native-resize-debug.json'),JSON.stringify({
    before:beforeNative.editor.layers.find(l=>l.id===nativeLayerId)?.placement,
    after:nativeProject.editor.layers.find(l=>l.id===nativeLayerId)?.placement,
    beforeRevision:beforeNative.revision,afterRevision:nativeProject.revision,
    inputReturnedRevision:afterNativeInput.revision,
    controls:nativeControls.controlPoints,handle:h,widthLength,resizeDelta,status:await ipc('viewport_status'),
  },null,2));
  assert.equal(BigInt(nativeProject.revision),BigInt(beforeNative.revision)+1n);
  assert.notEqual(nativeProject.editor.layers.find(l=>l.id===nativeLayerId).placement.widthMm,beforeNative.editor.layers.find(l=>l.id===nativeLayerId).placement.widthMm,JSON.stringify({before:beforeNative.editor.layers.find(l=>l.id===nativeLayerId).placement,after:nativeProject.editor.layers.find(l=>l.id===nativeLayerId).placement,handles:nativeControls.controlPoints,resizeDelta}));
  await new Promise(r=>setTimeout(r,100));
  nativeControls=await waitRenderer(s=>s.controlPoints.length===9&&s.selectedLayer===nativeLayerId);
  h=nativeControls.controlPoints[8];
  const beforeRotate=nativeProject.revision;
  await ipc('viewport_pointer',{input:{kind:'down',x:h[0],y:h[1]}});
  await ipc('viewport_pointer',{input:{kind:'move',x:h[0]+26,y:h[1]+12}});
  await ipc('viewport_pointer',{input:{kind:'up',x:h[0]+26,y:h[1]+12}});
  nativeProject=await waitProject(p=>p.revision!==beforeRotate);
  const cancelRevision=nativeProject.revision;
  await new Promise(r=>setTimeout(r,100));
  nativeControls=await waitRenderer(s=>s.controlPoints.length===9);
  h=nativeControls.controlPoints[0];
  await ipc('viewport_pointer',{input:{kind:'down',x:h[0],y:h[1]}});
  await ipc('viewport_pointer',{input:{kind:'move',x:h[0]-14,y:h[1]-8}});
  await ipc('viewport_pointer',{input:{kind:'cancel',x:h[0],y:h[1]}});
  await new Promise(r=>setTimeout(r,100));
  assert.equal((await ipc('read_view')).project.revision,cancelRevision);
  const afterTemporalHandles = await waitRenderer(s => s.antialiasing === 'taa' && s.visible
    && s.nativeVisible && s.controlPoints.length === 9 && !s.temporalPending && s.temporalSamples === 8);
  fs.copyFileSync(path.join(reportDir, 'data/native-viewport.png'), path.join(reportDir, 'antialiasing-taa-native-handles.png'));
  await new Promise(resolve => setTimeout(resolve, 250));
  assert.equal((await ipc('viewport_status')).frames, afterTemporalHandles.frames, 'TAA native gesture cancellation must converge and idle');
  await page.getByRole('button', { name: '⚙ 设置', exact: true }).click();
  await page.getByRole('tab', { name: '性能', exact: true }).click();
  await page.getByLabel('抗锯齿', { exact: true }).selectOption(renderDefaults.performance.antialiasing);
  await page.getByRole('button', { name: '应用设置', exact: true }).click();
  await page.getByRole('dialog', { name: '设置', exact: true }).waitFor({ state: 'hidden' });
  await waitRenderer(s => s.antialiasing === 'fxaa' && !s.temporalPending && s.temporalSamples === 0);
  checks.push('TAA artwork updates from 2D properties and native 3D resize/rotate/cancel keep accurate controls, commit once and converge to idle');
  checks.push('native 3D eight handles and rotation: preview is transient, release commits once, cancel leaves document unchanged');
  // C rings use dedicated meshes and masks; place artwork over a real triangle, not the central opening.
  for (const id of ['su7.front-c-ring', 'su7.rear-c-ring']) {
    const info = renderer.surfaces.find(s => s.id === id);
    assert(info && info.triangles > 0 && info.widthMm > 0 && info.heightMm > 0);
    const uv = await page.evaluate(async ({session, id}) => {
      const buffer = await window.__TAURI_INTERNALS__.invoke('viewport_surface', {session, id});
      const values = new Float32Array(buffer);
      return {count: values.length, x: (values[0]+values[2]+values[4])/3, y: (values[1]+values[3]+values[5])/3};
    }, {session: view.project.session, id});
    assert.equal(uv.count, info.triangles * 6);
    await page.getByLabel('编辑部件', {exact: true}).selectOption(id);
    const count = (await ipc('read_view')).project.editor.layers.length;
    await page.getByRole('button', {name: /□.*矩形/}).click();
    await waitProject(p => p.editor.layers.length === count + 1);
    const ring=(await ipc('read_view')).project.editor.layers.at(-1);
    assert(ring.placement.widthMm<info.widthMm&&ring.placement.heightMm<info.heightMm,'local insertion must fit narrow C ring paint');
    await waitRenderer(s => s.paintedSurfaces.includes(id));
    await page.screenshot({animations: 'disabled', path: path.join(reportDir, id + '.png')});
  }
  checks.push('front and rear C rings: independent paint mesh, nonempty triangle masks, committed artwork uploads and save/reopen');
  // Dedicated paint charts re-anchor one layer; coverage uses explicit adjacency.
  await page.getByLabel('编辑部件',{exact:true}).selectOption('su7.front-right-door');
  await page.locator('.design-canvas').getByLabel('跨相邻漆面',{exact:true}).check();
  const crossCount=(await ipc('read_view')).project.editor.layers.length;
  await page.getByRole('button',{name:/□.*矩形/}).click();
  let crossProject=await waitProject(p=>p.editor.layers.length===crossCount+1);
  const crossId=crossProject.editor.layers.at(-1).id;
  assert(crossProject.editor.layers.at(-1).placement.crossSurface);
  for(const [label,key,value]of [['图层宽度','widthMm',900],['图层高度','heightMm',400]]){
    await page.getByLabel(label,{exact:true}).fill(String(value));await page.getByLabel(label,{exact:true}).press('Tab');
    await waitProject(p=>p.editor.layers.at(-1).placement[key]===value);
  }
  const beforeCross=await ipc('read_view').then(view=>view.project);
  const crossLayer=beforeCross.editor.layers.find(l=>l.id===crossId);
  const nextCross=structuredClone(crossLayer);
  const targetSurface=renderer.surfaces.find(s=>s.id==='su7.front-right-lower-fender');
  nextCross.placement.surfaceId='su7.front-right-lower-fender';
  nextCross.placement.xMm=targetSurface.widthMm/2;
  nextCross.placement.yMm=targetSurface.heightMm/2;
  const targetCoverage=await ipc('viewport_coverage',{session:beforeCross.session,layer:nextCross});
  assert(targetCoverage.length>0,'target part center must resolve to paint coverage');
  const targetUv=targetCoverage.find(f=>f.partId==='su7.front-right-lower-fender').paintUv;
  nextCross.placement.xMm=targetUv.reduce((sum,p)=>sum+p[0],0)/targetUv.length;
  nextCross.placement.yMm=targetUv.reduce((sum,p)=>sum+p[1],0)/targetUv.length;
  nextCross.placement.widthMm=900;nextCross.placement.heightMm=400;
  const crossCoverage=await ipc('viewport_coverage',{session:beforeCross.session,layer:nextCross});
  assert(crossCoverage.some(f=>f.partId==='su7.front-right-lower-fender'));
  assert(crossCoverage.some(f=>f.partId!== 'su7.front-right-lower-fender'), 'reviewed lower-fender seams must reach an adjacent panel');
  assert(crossCoverage.every(f=>!f.partId.includes('-left-')&&f.partId!=='su7.rear-spoiler'));
  const lowerLocal=await ipc('viewport_coverage',{session:beforeCross.session,layer:{...nextCross,placement:{...nextCross.placement,crossSurface:false}}});
  assert(lowerLocal.length>0&&lowerLocal.every(f=>f.partId==='su7.front-right-lower-fender'));
  await ipc('execute',{requestId:crypto.randomUUID(),action:{type:'edit',mutation:{requestId:crypto.randomUUID(),session:beforeCross.session,expectedRevision:beforeCross.revision},operation:{type:'update',layer:nextCross}}});
  crossProject=await waitProject(p=>p.revision!==beforeCross.revision);
  assert.equal(crossProject.editor.layers.length,crossCount+1);
  const crossed=crossProject.editor.layers.find(l=>l.id===crossId);
  assert.equal(crossed.placement.surfaceId,'su7.front-right-lower-fender');
  assert.equal(crossed.placement.widthMm,900);assert.equal(crossed.placement.heightMm,400);
  assert.equal(BigInt(crossProject.revision),BigInt(beforeCross.revision)+1n);
  checks.push('one-layer cross-part mapping update: target coverage, single revision and dimensions preserved');
  checks.push('2D and 3D consume the same versioned paint map; layer identity and chart anchor persist');

  await checkOrientationTransport({ipc, reportDir, checks});
  await checkDoorCrossDisplay({page, ipc, waitProject, waitRenderer, reportDir, checks});
  await checkEditorProperties({page, ipc, waitProject, waitRenderer, reportDir, checks});

  // Restore the selected layer state so existing generic textbox checks stay unambiguous.
  await page.getByLabel('编辑部件', { exact: true }).selectOption('su7.front-left-door');
  if (process.env.ITASHA_RENDER_BENCH === '1') {
    const samples = [];
    for (let sample = 0; sample < 5; sample++) {
      const previous = await ipc('viewport_status');
      await ipc('viewport_load', { session: view.project.session, force: true });
      const measured = await waitRenderer(s => s.generation > previous.generation && s.state === 'ready' && s.frames > 0);
      assert.equal(measured.stats.fingerprint, renderer.stats.fingerprint);
      samples.push(measured);
    }
    fs.writeFileSync(path.join(reportDir, 'renderer-reloads.json'), JSON.stringify({ mode: 'Debug, warm process and filesystem cache; CPU readiness/submit, not GPU-present latency', samples }, null, 2));
  }
  await new Promise(resolve => setTimeout(resolve, 500));
  const idle = await ipc('viewport_status');
  await new Promise(resolve => setTimeout(resolve, 500));
  assert.equal((await ipc('viewport_status')).frames, idle.frames, 'idle viewport should stop submitting frames');
  const cachedGeneration = (await ipc('viewport_status')).generation;
  await page.getByRole('button', { name: '2D 画布', exact: true }).click();
  await page.locator('.layout-edit').waitFor();
  await waitRenderer(s => !s.visible && !s.nativeVisible);
  await page.getByRole('button', { name: '双视图', exact: true }).click();
  await waitRenderer();
  assert.equal((await ipc('viewport_status')).generation, cachedGeneration, 'view switching must reuse the loaded GPU scene');
  checks.push('SU7 modular GLB, chosen wheel/steering, interior textures and native GPU first submit');
  if (process.env.ITASHA_VISUAL_REVIEW === '1') {
    console.log(JSON.stringify({ review: true, port, pid: child.pid, reportDir, renderer }));
    const until = Date.now() + 240000;
    while (!fs.existsSync(path.join(reportDir, 'continue')) && Date.now() < until) await new Promise(resolve => setTimeout(resolve, 500));
  }
  await selectTheme('light');
  await waitTheme('light');
  await page.screenshot({ animations: 'disabled', path: path.join(reportDir, 'workspace-light.png') });
  await page.getByRole('button', { name: '⚙ 设置', exact: true }).click();
  await waitRenderer(status => !status.visible && !status.nativeVisible);
  await page.screenshot({ animations: 'disabled', path: path.join(reportDir, 'settings-light.png') });
  await page.getByRole('button', { name: '关闭设置', exact: true }).click();
  await waitRenderer();
  await selectTheme('dark');
  await waitTheme('dark');
  await waitRenderer();
  await page.evaluate(()=>{window.__libraryEvents=[];for(const type of ['pointerdown','pointermove','pointerup','pointercancel']) document.querySelector('.library-resizer').addEventListener(type,e=>window.__libraryEvents.push({type,x:e.clientX,buttons:e.buttons}));});
  const libraryHandle = page.getByRole('separator', { name: '调整工程库宽度', exact: true });
  let libraryBox = await libraryHandle.boundingBox();
  await page.mouse.move(libraryBox.x + 3, libraryBox.y + 100);
  await page.mouse.down();
  await page.mouse.move(290, libraryBox.y + 100, { steps: 8 });
  await page.mouse.up();
  await page.waitForFunction(() => document.querySelector('.project-library').getBoundingClientRect().width === 290);
  libraryBox = await libraryHandle.boundingBox();
  await page.mouse.move(libraryBox.x + 3, libraryBox.y + 100);
  await page.mouse.down();
  await page.mouse.move(20, libraryBox.y + 100, { steps: 8 });
  await page.mouse.up();
  await page.getByRole('button', { name: '展开工程库', exact: true }).waitFor();
  assert.equal((await ipc('read_view')).settings.workspace.libraryMinimized, true);
  await page.getByRole('button', { name: '展开工程库', exact: true }).click();
  await page.waitForFunction(() => document.querySelector('.project-library').getBoundingClientRect().width === 290);
  await page.getByRole('button', { name: '最小化工程库', exact: true }).click();
  await page.getByRole('button', { name: '展开工程库', exact: true }).waitFor();
  checks.push('project library pointer resize, edge snap and restore previous width');
  for (const title of ['素材库', '图层管理', '插入图形', '插入文字']) {
    const panel = page.getByRole('region', { name: title, exact: true });
    await panel.scrollIntoViewIfNeeded();
    await page.getByRole('button', { name: '最小化' + title, exact: true }).click();
    await page.getByRole('button', { name: '恢复' + title, exact: true }).waitFor();
    assert.equal(await panel.locator('.dock-panel-body').isVisible(), false);
    await page.getByRole('button', { name: '恢复' + title, exact: true }).click();
    await panel.locator('.dock-panel-body').waitFor();
  }
  const resize = page.getByRole('separator', { name: '调整素材库高度', exact: true });
  await resize.focus();
  await page.keyboard.press('Home');
  await page.waitForFunction(() => document.querySelector('[aria-label="调整素材库高度"]').getAttribute('aria-valuenow') === '100');
  await resize.scrollIntoViewIfNeeded();
  const handle = await resize.boundingBox();
  await page.mouse.move(handle.x + handle.width / 2, handle.y + 4);
  await page.mouse.down();
  await page.mouse.move(handle.x + handle.width / 2, handle.y + 64, { steps: 6 });
  await page.mouse.up();
  await page.waitForFunction(() => document.querySelector('[aria-label="调整素材库高度"]').getAttribute('aria-valuenow') === '160');
  await page.getByRole('button', { name: '最小化插入文字', exact: true }).click();
  checks.push('four dock panels minimize/restore and support keyboard and pointer resizing');
  const blocked = await page.evaluate(async () => {
    try { await window.__TAURI_INTERNALS__.invoke('close_clean'); return false; }
    catch (e) { return e; }
  });
  assert.equal(blocked.code, 'unsaved-changes', JSON.stringify(blocked));
  checks.push('Rust close guard rejects dirty session');
  await page.getByRole('button', { name: '保存工程', exact: true }).click();
  await page.getByText('已保存', { exact: true }).waitFor();
  const original = await ipc('read_view');
  await page.getByRole('textbox', { name: '工程名称', exact: true }).fill('阶段一验收工程');
  await page.getByRole('button', { name: '应用名称', exact: true }).click();
  await page.getByText('阶段一验收工程 · 未保存', { exact: true }).waitFor();
  await page.getByRole('button', { name: '撤销', exact: true }).click();
  await page.getByText('已保存', { exact: true }).waitFor();
  checks.push('rename and undo return to saved content');
  await page.getByRole('button', { name: '重做', exact: true }).click();
  await page.getByText('阶段一验收工程 · 未保存', { exact: true }).waitFor();
  await page.getByRole('button', { name: '保存工程', exact: true }).click();
  await page.getByText('已保存', { exact: true }).waitFor();
  await page.getByRole('button', { name: '3D 视图', exact: true }).click();
  await page.locator('.layout-3d').waitFor();
  await waitRenderer();
  await page.getByRole('button', { name: '双视图', exact: true }).click();
  await page.locator('.layout-both').waitFor();
  checks.push('layout settings persisted through Rust');
  await page.getByRole('button', { name: '左右', exact: true }).click();
  await page.locator('.direction-horizontal').waitFor();
  await waitRenderer();
  let a = await page.locator('.viewport').boundingBox();
  let b = await page.locator('.canvas').boundingBox();
  assert(b.x > a.x + a.width && Math.abs(a.y - b.y) < 2, 'panels must be side by side');
  const separator = page.getByRole('separator', { name: '调整双视图比例', exact: true });
  await separator.focus();
  await page.keyboard.press('Home');
  await page.waitForFunction(() => document.querySelector('.splitter').getAttribute('aria-valuenow') === '25');
  await page.getByRole('button', { name: '上下', exact: true }).click();
  await page.locator('.direction-vertical').waitFor();
  await waitRenderer();
  a = await page.locator('.viewport').boundingBox();
  b = await page.locator('.canvas').boundingBox();
  assert(b.y > a.y + a.height && Math.abs(a.x - b.x) < 2, 'panels must be stacked');
  const splitterBox = await separator.boundingBox();
  const workspaceBox = await page.locator('.workspace').boundingBox();
  const dragStart = { x: splitterBox.x + splitterBox.width / 2, y: splitterBox.y + 4 };
  const dragEnd = { x: dragStart.x, y: workspaceBox.y + 16 + (workspaceBox.height - 32) * .60 };
  await page.mouse.move(dragStart.x, dragStart.y);
  await page.mouse.down();
  for (let step = 1; step <= 8; step++) {
    await page.mouse.move(dragStart.x, dragStart.y + (dragEnd.y - dragStart.y) * step / 8);
    const duringResize = await ipc('viewport_status');
    assert.equal(duringResize.visible, true, '3D viewport must stay visible during split-ratio dragging');
    assert.equal(duringResize.nativeVisible, true, 'native 3D window must stay visible during split-ratio dragging');
  }
  await page.mouse.up();
  await page.waitForFunction(() => document.querySelector('.splitter').getAttribute('aria-valuenow') === '60');
  await page.getByRole('button', { name: '左右', exact: true }).click();
  await page.locator('.direction-horizontal').waitFor();
  const horizontalSeparator = await separator.boundingBox();
  const horizontalWorkspace = await page.locator('.workspace').boundingBox();
  const horizontalStart = { x: horizontalSeparator.x + horizontalSeparator.width / 2, y: horizontalSeparator.y + horizontalSeparator.height / 2 };
  const horizontalEnd = { x: horizontalWorkspace.x + 16 + (horizontalWorkspace.width - 32) * .42, y: horizontalStart.y };
  await page.mouse.move(horizontalStart.x, horizontalStart.y);
  await page.mouse.down();
  for (let step = 1; step <= 8; step++) {
    await page.mouse.move(horizontalStart.x + (horizontalEnd.x - horizontalStart.x) * step / 8, horizontalStart.y);
    const duringResize = await ipc('viewport_status');
    assert.equal(duringResize.visible, true, '3D viewport must stay visible while dragging the horizontal split');
    assert.equal(duringResize.nativeVisible, true, 'native 3D window must stay visible while dragging the horizontal split');
  }
  await page.mouse.up();
  await page.waitForFunction(() => document.querySelector('.splitter').getAttribute('aria-valuenow') === '42');
  await page.screenshot({ animations: 'disabled', path: path.join(reportDir, 'horizontal.png') });
  checks.push('both split directions resize live while the native 3D view remains visible');
  await waitRenderer();
  const viewportGeometry = await page.locator('.native-viewport-host').boundingBox();
  const scale = await page.evaluate(() => devicePixelRatio);
  const nativeGeometry = await waitRenderer(s=>Math.abs(s.width-viewportGeometry.width*scale)<=1&&Math.abs(s.height-viewportGeometry.height*scale)<=1&&s.nativeFrontmost);
  assert(Math.abs(nativeGeometry.width - viewportGeometry.width * scale) <= 1);
  assert(Math.abs(nativeGeometry.height - viewportGeometry.height * scale) <= 1);
  checks.push('native HWND covers WebView at nine points in 3D/both split directions, hides under settings and submits no idle frames');
  await page.getByRole('button', { name: '⚙ 设置', exact: true }).click();
  const settings = page.getByRole('dialog', { name: '设置', exact: true });
  await settings.waitFor();
  await page.getByRole('tab', { name: '快捷键', exact: true }).click();
  await page.getByRole('textbox', { name: '保存工程快捷键', exact: true }).focus();
  await page.keyboard.press('Control+Z');
  await page.getByRole('button', { name: '应用设置', exact: true }).click();
  await page.getByText('快捷键存在冲突，请为每项设置不同的组合。', { exact: true }).waitFor();
  await page.getByRole('textbox', { name: '保存工程快捷键', exact: true }).focus();
  await page.keyboard.press('Control+Shift+S');
  await page.getByRole('tab', { name: '性能', exact: true }).click();
  await page.getByLabel('目标帧率', { exact: true }).selectOption('30');
  await page.getByLabel('减少界面动态效果', { exact: false }).check();
  await page.getByRole('tab', { name: '路径', exact: true }).click();
  await page.locator('.path-setting').filter({ hasText: '默认导出目录' }).locator('input').fill(path.join(reportDir, 'missing-folder'));
  await page.getByRole('button', { name: '应用设置', exact: true }).click();
  await settings.getByText('请选择已存在的绝对目录路径。', { exact: true }).waitFor();
  assert.equal((await ipc('read_view')).settings.shortcuts.save, 'Ctrl+S');
  await page.locator('.path-setting').filter({ hasText: '默认导出目录' }).locator('input').fill(reportDir);
  await page.getByRole('button', { name: '应用设置', exact: true }).click();
  await settings.waitFor({ state: 'hidden' });
  view = await ipc('read_view');
  assert.equal(view.settings.performance.targetFps, 30);
  assert.equal(view.settings.paths.exports, reportDir);
  assert.equal(view.settings.shortcuts.save, 'Ctrl+Shift+S');
  await page.locator('.reduced-motion').waitFor();
  await page.keyboard.press('Control+Comma');
  await settings.waitFor();
  await page.getByRole('tab', { name: '性能', exact: true }).click();
  await page.screenshot({ animations: 'disabled', path: path.join(reportDir, 'settings.png') });
  await page.getByRole('button', { name: '关闭设置', exact: true }).click();
  checks.push('settings conflict validation, performance/path persistence and settings shortcut');
  await page.getByRole('textbox', { name: '工程名称', exact: true }).fill('快捷键保存验收');
  await page.getByRole('button', { name: '应用名称', exact: true }).click();
  await page.getByText('快捷键保存验收 · 未保存', { exact: true }).waitFor();
  await page.locator('.splitter').focus();
  await page.keyboard.press('Control+Shift+S');
  await page.getByText('已保存', { exact: true }).waitFor();
  await page.getByRole('textbox', { name: '工程名称', exact: true }).fill('阶段一验收工程');
  await page.getByRole('button', { name: '应用名称', exact: true }).click();
  await page.getByText('阶段一验收工程 · 未保存', { exact: true }).waitFor();
  await page.getByRole('button', { name: '保存工程', exact: true }).click();
  await page.getByText('已保存', { exact: true }).waitFor();
  checks.push('custom save shortcut invokes Rust command');
  await page.screenshot({ animations: 'disabled', path: path.join(reportDir, 'desktop.png') });
  await page.reload();
  await page.getByText('桌面主框架就绪', { exact: true }).waitFor();
  await page.getByRole('heading', { name: '让灵感，成为车身的一部分。', exact: true }).waitFor();
  await page.getByRole('button', { name: /加载设计/ }).click();
  await page.getByRole('button', { name: /阶段一验收工程.*打开设计/ }).click();
  await page.getByRole('button', { name: '展开工程库', exact: true }).waitFor();
  assert.equal((await ipc('read_view')).settings.workspace.libraryWidth, 290);
  await page.getByRole('button', { name: '展开工程库', exact: true }).click();
  view = await ipc('read_view');
  assert.equal(view.settings.workspace.direction, 'horizontal');
  assert.equal(view.settings.workspace.splitPercent, 42);
  assert.equal(view.settings.performance.targetFps, 30);
  assert.equal(view.settings.shortcuts.save, 'Ctrl+Shift+S');
  assert.equal(view.project.session.projectId, original.project.session.projectId);
  assert.equal(view.project.name, '阶段一验收工程');
  assert.deepEqual(view.project.configuration, original.project.configuration);
  assert.deepEqual(view.project.editor, original.project.editor, 'layers, placements and model fingerprint survive reopen');
  assert(!view.project.dirty);
  checks.push('saved draft reopens with original project identity');
  await page.getByRole('button', { name: '← 欢迎页', exact: true }).click();
  await page.getByRole('heading', { name: '让灵感，成为车身的一部分。', exact: true }).waitFor();
  let originalRecent = page.locator('.recent-designs > button').filter({ hasText: '阶段一验收工程' });
  await originalRecent.focus();
  await page.keyboard.press('Shift+F10');
  await page.getByRole('menu').waitFor();
  await page.keyboard.press('Escape');
  await page.getByRole('menu').waitFor({ state: 'hidden' });
  assert(await originalRecent.evaluate(element => document.activeElement === element), 'Escape should restore focus to the recent-design card');
  await originalRecent.click({ button: 'right' });
  await page.getByRole('menuitem', { name: '置顶', exact: true }).click();
  view = await ipc('read_view');
  const pinnedOriginal = view.projects.find(item => item.projectId === original.project.session.projectId);
  assert.equal(pinnedOriginal.pinned, true, 'pin should persist in the library summary');
  assert.equal(view.projects[0].projectId, original.project.session.projectId, 'pinned designs should sort before unpinned designs');
  await page.reload();
  await page.getByRole('heading', { name: '让灵感，成为车身的一部分。', exact: true }).waitFor();
  view = await ipc('read_view');
  assert.equal(view.projects.find(item => item.projectId === original.project.session.projectId).pinned, true,
    'pin state should survive application reload');
  originalRecent = page.locator('.recent-designs > button').filter({ hasText: '阶段一验收工程' });
  await originalRecent.click({ button: 'right' });
  await page.getByRole('menuitem', { name: '取消置顶', exact: true }).click();
  view = await ipc('read_view');
  assert.equal(view.projects.find(item => item.projectId === original.project.session.projectId).pinned, false);

  originalRecent = page.locator('.recent-designs > button').filter({ hasText: '阶段一验收工程' });
  await originalRecent.click({ button: 'right' });
  await page.getByRole('menuitem', { name: '复制', exact: true }).click();
  view = await ipc('read_view');
  const duplicate = view.projects.find(item => item.name === '阶段一验收工程 副本');
  assert(duplicate, 'duplicate should create a new project identity');
  assert.equal(duplicate.name, '阶段一验收工程 副本');
  assert.notEqual(duplicate.projectId, original.project.session.projectId);
  const duplicateCard = page.locator('.recent-designs > button').filter({ hasText: duplicate.name });
  await duplicateCard.click();
  await page.locator('.workspace').waitFor();
  view = await ipc('read_view');
  assert.equal(view.project.session.projectId, duplicate.projectId);
  assert.deepEqual(view.project.configuration, original.project.configuration);
  assert.deepEqual(view.project.editor, original.project.editor, 'duplicate retains saved options and artwork');
  await page.getByRole('button', { name: '← 欢迎页', exact: true }).click();
  await page.getByRole('heading', { name: '让灵感，成为车身的一部分。', exact: true }).waitFor();
  const duplicateCardForDelete = page.locator('.recent-designs > button').filter({ hasText: duplicate.name });
  await duplicateCardForDelete.click({ button: 'right' });
  await page.getByRole('menuitem', { name: '删除', exact: true }).click();
  view = await ipc('read_view');
  assert(!view.projects.some(item => item.projectId === duplicate.projectId), 'deleted copy must leave the visible library');
  assert(view.projects.some(item => item.projectId === original.project.session.projectId), 'deleting a copy must preserve the original');
  assert(fs.readdirSync(path.join(smokeDataDir, 'trash', 'projects')).some(name => name.startsWith(duplicate.projectId + '-')),
    'delete should archive the project file in reversible trash');
  await page.locator('.recent-designs > button').filter({ hasText: '阶段一验收工程' }).click();
  await page.locator('.workspace').waitFor();
  view = await ipc('read_view');
  assert.equal(view.project.session.projectId, original.project.session.projectId);
  checks.push('recent-design right-click pin persists, copy gets a new identity with saved content, and delete archives only the copy');
  await page.getByRole('textbox', { name: '工程名称', exact: true }).fill('切换保存验收');
  await page.getByRole('button', { name: '应用名称', exact: true }).click();
  await page.getByText('切换保存验收 · 未保存', { exact: true }).waitFor();
  await page.getByRole('button', { name: '← 欢迎页', exact: true }).click();
  await page.getByRole('button', { name: '保存并继续', exact: true }).click();
  await page.getByRole('heading', { name: '让灵感，成为车身的一部分。', exact: true }).waitFor();
  await page.getByRole('button', { name: /切换保存验收.*修订/ }).click();
  await page.locator('.workspace').waitFor();
  assert.equal((await ipc('read_view')).settings.workspace.panels.assets.height, 160);
  assert.equal((await ipc('read_view')).settings.workspace.panels.text.minimized, true);
  await page.getByRole('textbox', { name: '工程名称', exact: true }).fill('应被放弃的名称');
  await page.getByRole('button', { name: '应用名称', exact: true }).click();
  await page.getByText('应被放弃的名称 · 未保存', { exact: true }).waitFor();
  await page.getByRole('button', { name: '← 欢迎页', exact: true }).click();
  await page.getByRole('button', { name: '放弃并继续', exact: true }).click();
  await page.getByRole('heading', { name: '让灵感，成为车身的一部分。', exact: true }).waitFor();
  await page.getByRole('button', { name: /切换保存验收.*修订/ }).click();
  await page.locator('.workspace').waitFor();
  view = await ipc('read_view');
  assert.equal(view.project.name, '切换保存验收');
  assert(!view.project.dirty);
  checks.push('save-and-continue and discard-and-continue preserve saved designs');
  // A second assembly verifies that modules and material choices are not fixed to the first design.
  await page.getByRole('button', { name: '＋ 新建设计', exact: true }).click();
  await page.getByRole('button', { name: /轿车.*SU7 Max/ }).click();
  await page.getByRole('button', { name: '下一步：车型选配 →', exact: true }).click();
  await page.getByRole('tab', { name: '车漆', exact: true }).click();
  await page.getByRole('button', { name: /曜石黑/ }).click();
  await page.getByRole('tab', { name: '轮毂', exact: true }).click();
  await page.getByRole('button', { name: /20英寸米型轮毂 长续航胎/ }).click();
  await page.getByRole('button', { name: '完成配置，进入工作区 →', exact: true }).click();
  const secondProject = (await ipc('read_view')).project;
  assert.equal(secondProject.configuration.paint, '600061063');
  assert.equal(secondProject.configuration.wheels, '600061081');
  const secondRenderer = await waitRenderer(s => s.sessionEpoch === secondProject.session.sessionEpoch && s.state === 'ready' && s.frames > 0);
  assert.notEqual(secondRenderer.stats.fingerprint, renderer.stats.fingerprint);
  fs.writeFileSync(path.join(reportDir, 'renderer-default.json'), JSON.stringify(secondRenderer, null, 2));
  fs.copyFileSync(path.join(reportDir, 'data/native-viewport.png'), path.join(reportDir, 'native-su7-black-rice-wheel.png'));
  const staleLoad = await page.evaluate(async session => {
    try { await window.__TAURI_INTERNALS__.invoke('viewport_load', { session, force: false }); return null; }
    catch (error) { return error; }
  }, original.project.session);
  assert.equal(staleLoad?.code, 'stale-session', JSON.stringify(staleLoad));
  await page.getByRole('button', { name: '← 欢迎页', exact: true }).click();
  await page.getByRole('button', { name: '放弃并继续', exact: true }).click();
  await page.getByRole('button', { name: '新建设计', exact: false }).first().click();
  await page.getByRole('button', { name: /SUV.*YU7 长续航版/ }).click();
  await page.getByRole('button', { name: '下一步：车型选配 →', exact: true }).click();
  await page.getByRole('button', { name: '完成配置，进入工作区 →', exact: true }).click();
  await page.getByLabel('原生 3D 预览').getByText('当前只接入 SU7 Max 模块化资产；此车型的专用模型尚未接入。', { exact: true }).waitFor();
  await waitRenderer(s => !s.visible);
  await page.getByRole('button', { name: '← 欢迎页', exact: true }).click();
  await page.getByRole('button', { name: '放弃并继续', exact: true }).click();
  await page.getByRole('button', { name: /切换保存验收.*修订/ }).click();
  await page.locator('.workspace').waitFor();
  checks.push('second SU7 assembly differs; stale sessions and unsupported vehicles cannot display an unrelated model');
  // Ask the named Tauri main window to close; this raises its native CloseRequested event.
  await ipc('plugin:window|close', { label: 'main' });
  await page.getByRole('dialog').waitFor();
  await page.getByRole('button', { name: '取消', exact: true }).click();
  await page.getByRole('dialog').waitFor({ state: 'hidden' });
  checks.push('Windows close request shows cancellable dialog');
  for(const name of fs.readdirSync(reportDir).filter(n=>n.endsWith('.png'))){
    const bytes=fs.readFileSync(path.join(reportDir,name));
    assert(bytes.length>100&&bytes.subarray(0,8).equals(Buffer.from([137,80,78,71,13,10,26,10]))&&bytes.subarray(-8,-4).toString()==='IEND','complete PNG evidence: '+name);
  }
  assert.deepEqual(errors, []);
  assert.equal(await page.locator('iframe').count(), 0);
  await ipc('close_clean').catch(error => {
    if (!String(error).includes('closed')) throw error;
  });
  await new Promise(resolve => { if (child.exitCode !== null) resolve(); else { child.once('exit', resolve); setTimeout(resolve, 5000); } });
  assert.equal(child.exitCode, 0);
  checks.push('clean close exits desktop process');
  fs.writeFileSync(path.join(reportDir, 'result.json'), JSON.stringify({ passed: true, checks, scope: 'Real desktop IPC, native HWND visibility/sibling hit-tests and GPU submissions; GPU PNGs are readbacks, not OS composition screenshots' }, null, 2));
  console.log(JSON.stringify({ passed: true, checks, reportDir }, null, 2));
} catch(error) {
  try { const page=browser?.contexts()[0]?.pages().find(p=>!p.url().startsWith('devtools:')); if(page){await page.screenshot({path:path.join(reportDir,'failure.png')});fs.writeFileSync(path.join(reportDir,'failure.json'),JSON.stringify(await page.evaluate(()=>({events:window.__libraryEvents,width:document.querySelector('.project-library')?.getBoundingClientRect().width})),null,2));} } catch{}
  throw error;
} finally {
  if (browser) await browser.close().catch(() => {});
  if (child.exitCode === null) child.kill();
  if (stderr) fs.writeFileSync(path.join(reportDir, 'stderr.log'), stderr);
}
