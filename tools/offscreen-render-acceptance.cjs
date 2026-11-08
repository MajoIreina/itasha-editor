const { chromium } = require('../desktop/node_modules/playwright-core');
const assert = require('node:assert/strict');

(async () => {
  const browser = await chromium.launch({ channel: 'msedge', headless: true, args: ['--enable-webgl', '--ignore-gpu-blocklist', '--enable-unsafe-swiftshader'] });
  try {
    const page = await browser.newPage({ viewport: { width: 1440, height: 900 } });
    const errors = [];
    page.on('pageerror', error => errors.push(error.stack));
    await page.goto(process.env.STUDIO_URL || 'http://127.0.0.1:8877/');
    await page.click('#home-new');
    await page.fill('#new-project-name', '高精度渲染验收');
    await page.click('#new-project-create');
    await page.waitForFunction(() => studio?.ready, null, { timeout: 90000 });
    await page.click('#configuration-confirm');
    await page.waitForFunction(() => studio?.stage === 'edit');
    const frame = page.frames().find(candidate => candidate.url().includes('/configurator/'));
    await page.waitForFunction(() => document.querySelector('#high-res-render'));
    assert.equal(await page.locator('#high-res-render').isVisible(), true);
    const render = async yaw => frame.evaluate(yaw => su7Bridge.renderOffscreen({ yaw, pitch: 12, width: 1024, height: 768 }), yaw);
    const first = await render(-45);
    assert.match(first, /^data:image\/png;base64,/);
    const firstInfo = await page.evaluate(async src => { const image = new Image(); image.src = src; await image.decode(); const c = document.createElement('canvas'); c.width = image.width; c.height = image.height; const x = c.getContext('2d'); x.drawImage(image, 0, 0); const p = x.getImageData(0, 0, c.width, c.height).data; let visible = 0, colors = new Set(); for (let i = 0; i < p.length; i += 64) { if (p[i + 3]) visible++; colors.add(`${p[i]},${p[i + 1]},${p[i + 2]}`); } return { width: image.width, height: image.height, visible, colors: colors.size }; }, first);
    assert.deepEqual([firstInfo.width, firstInfo.height], [1024, 768]);
    assert.ok(firstInfo.visible > 500 && firstInfo.colors > 20, JSON.stringify(firstInfo));
    const second = await render(45);
    assert.notEqual(second, first, 'changing camera yaw changes the offscreen image');
    await assert.rejects(() => frame.evaluate(() => su7Bridge.renderOffscreen({ yaw: 0, pitch: 12, width: 8192, height: 8192 })), /16 MP/);
    await page.click('#high-res-render');
    await page.fill('#render-yaw', '-45');
    await page.fill('#render-pitch', '12');
    await page.fill('#render-width', '1024');
    await page.fill('#render-height', '768');
    await page.click('#render-start');
    await page.waitForFunction(() => document.querySelector('#render-result')?.hidden === false);
    assert.equal(await page.locator('#render-image').evaluate(image => image.naturalWidth), 1024);
    assert.equal(await page.locator('#render-status').textContent(), '渲染完成');
    assert.deepEqual(errors, []);
    console.log(JSON.stringify({ checks: ['toolbar entry', 'offscreen PNG dimensions and visible scene', 'camera angle changes result', '16 MP validation', 'dialog preview', 'no page errors'], image: firstInfo }));
  } finally {
    await browser.close();
  }
})().catch(error => { console.error(error); process.exitCode = 1; });
