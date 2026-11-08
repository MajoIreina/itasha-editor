const fs = require('node:fs');
const path = require('node:path');
const assert = require('node:assert/strict');
const { PNG } = require('../desktop/node_modules/pngjs');
const { BackendProcess } = require('../desktop/src/backend.cjs');
const root = path.resolve(__dirname, '..');
const output = path.join(__dirname, 'acceptance', 'rust-backend');
const data = fs.mkdtempSync(path.join(require('node:os').tmpdir(), 'itasha-rust-'));
const backend = new BackendProcess({ root, data, port: 0, gpuAtStartup: true });
const checks = [], failures = [];
const check = (name, condition) => { assert.ok(condition, name); checks.push(name); console.log('PASS ' + name); };

(async () => {
  const ready = await backend.start();
  const headers = { 'Content-Type': 'application/json', 'X-Studio-Request': '1', 'X-Itasha-Token': ready.token };
  const api = async (route, args = {}) => {
    const r = await fetch(ready.origin + '/__api/' + route, { method: 'POST', headers, body: JSON.stringify(args) });
    const result = await r.json(); if (!r.ok) throw new Error(result.error); return result;
  };
  const get = url => fetch(ready.origin + url, { headers: { 'X-Itasha-Token': ready.token } });
  check('Rust backend health is versioned', (await (await fetch(ready.origin + '/__studio_health')).json()).backend === 'rust');
  check('Unauthenticated requests cannot access projects', (await fetch(ready.origin + '/__api/projects', { method: 'POST', body: '{}' })).status === 403);
  const bootstrap = await fetch(ready.origin + '/?session=' + ready.token, { redirect: 'manual' });
  check('Desktop bootstrap creates an HttpOnly session cookie', bootstrap.status === 302 && /HttpOnly/.test(bootstrap.headers.get('set-cookie')));
  const p = { format: 'su7-livery', version: 1, name: 'Rust中文工程', panels: { left: { layers: [{ id: 'a', wrap: { version: 3, patches: [{ uv: [0, 0, 1, 0, 0, 1] }] } }] } }, assets: { a: { src: 'data:image/png;base64,AA==' } }, vehicle: { id: 'su7' } };
  const saved = await api('save', { project: p });
  assert.deepEqual((await api('open', { file: saved.file })).project, p); check('Existing version-one projects round-trip without geometry loss', true);
  p.panels.left.layers[0].wrap.patches[0].uv[0] = 0.125;
  await api('save', { project: p }); assert.deepEqual((await api('open', { file: saved.file })).project, p); check('Atomic save safely replaces an existing project', true);
  check('Project listing retains Chinese names', (await api('projects')).projects.some(v => v.file === saved.file));
  await assert.rejects(api('open', { file: '../outside.su7design' })); check('Project traversal is rejected', true);
  await assert.rejects(api('save', { project: { format: 'bad', version: 1 } })); check('Unknown project formats are rejected', true);
  const settings = await api('performance');
  await assert.rejects(api('performance-update', { multithreading: true, threads: 0, gpuAcceleration: true }));
  await assert.rejects(api('performance-update', { multithreading: true, threads: 1.5, gpuAcceleration: true }));
  await assert.rejects(api('performance-update', { multithreading: 1, threads: 1, gpuAcceleration: true })); check('Thread boundaries and boolean types are validated', true);
  const updated = await api('performance-update', { multithreading: true, threads: Math.min(2, settings.logicalProcessors), gpuAcceleration: false });
  check('Performance settings persist and GPU restart state is correct', updated.restartRequired && updated.effectiveWorkers === Math.min(2, settings.logicalProcessors) && JSON.parse(fs.readFileSync(path.join(data, 'performance-settings.json'))).gpuAcceleration === false);
  const vehicles = (await (await get('/__vehicles')).json()).vehicles;
  check('All seven existing vehicle definitions are preserved', vehicles.length === 7 && vehicles[0].available && vehicles.slice(1).every(v => !v.available));
  for (const vehicle of vehicles) {
    const catalogue = await (await get('/__catalog?vehicle=' + vehicle.id)).json();
    const adapter = JSON.parse(fs.readFileSync(path.join(root, 'vehicles', vehicle.id, 'backend-adapter.json')));
    const tabs = adapter.configuration.data.carSsuDetail.configurator.tabList;
    const options = tabs.filter(t => t.tabName === '选装').flatMap(t => t.groupList.flatMap(g => g.subGroupList.flatMap(s => s.itemList)));
    check(vehicle.id + ' loads its own configuration and defaults', catalogue.tabs.length === tabs.filter(t => t.tabName !== '选装').length && catalogue.options.length === options.length);
  }
  check('Unknown vehicle does not fall back to SU7', (await get('/__catalog?vehicle=unknown')).status === 400);
  const manifest = JSON.parse(fs.readFileSync(path.join(root, 'resources/manifest.json')));
  const model = manifest.entries.find(e => e.status === 200 && e.bytes > 1000 && /assets\.mi/.test(e.url));
  assert.ok(model, 'captured model fixture');
  const replay = await fetch(ready.origin + '/__archive', { method: 'POST', headers, body: JSON.stringify({ vehicle: 'su7', url: model.url, method: model.method, body: model.postData }) });
  const actual = Buffer.from(await replay.arrayBuffer()), expected = fs.readFileSync(path.join(root, 'resources', model.file));
  check('Encrypted model package is replayed byte-for-byte', replay.ok && actual.equals(expected));
  const script = manifest.entries.find(e => e.status === 200 && e.bytes > 10000 && /javascript/.test(e.headers['content-type'] || '') && fs.readFileSync(path.join(root, 'resources', e.file)).includes('this.render=function(a,t){if(void 0!==t'));
  assert.ok(script, 'engine hook fixture');
  const patched = await fetch(ready.origin + '/__archive', { method: 'POST', headers, body: JSON.stringify({ vehicle: 'su7', url: script.url, method: script.method, body: script.postData }) }).then(r => r.text());
  check('Captured renderer receives frame, viewer and material hooks', patched.includes('window.__su7Scenes') && patched.includes('su7Bridge.attachViewer') && patched.includes('su7Bridge.registerMaterial'));
  const fixtures = path.join(root, 'tools', 'fixtures', 'images');
  for (const name of fs.readdirSync(fixtures).filter(n => !/corrupt|json$/.test(n))) {
    const payload = fs.readFileSync(path.join(fixtures, name));
    const result = await api('decode-image', { name, data: payload.toString('base64') });
    if (name.endsWith('.svg')) assert.match(result.data, /^data:image\/svg\+xml/);
    else {
      const png = PNG.sync.read(Buffer.from(result.data.split(',')[1], 'base64'));
      assert.ok(png.width > 0 && png.height > 0);
      if (name === 'test.png') assert.equal(png.data[3], 175);
      if (name === 'rotated.jpg') assert.deepEqual([png.width, png.height], [80, 128]);
      if (name === 'merged.psd') assert.match(result.note, /合成图/);
      if (name === 'animated.gif') assert.match(result.note, /首帧/);
    }
    check('Native image decoder: ' + name, true);
  }
  await assert.rejects(api('decode-image', { name: 'corrupt.png', data: Buffer.from('not an image').toString('base64') })); check('Corrupt image produces a recoverable API error', true);
  const rgba = new PNG({ width: 3, height: 2 }); rgba.data.fill(0); for (let i = 0; i < rgba.data.length; i += 4) { rgba.data[i] = 235; rgba.data[i + 3] = 175; }
  const png = PNG.sync.write(rgba), encoded = 'data:image/png;base64,' + png.toString('base64');
  const print = { widthMm: 297, heightMm: 210, dpi: 300 };
  const exported = await api('export', { name: '测试制作.png', data: encoded, base64: true, print });
  const printed = fs.readFileSync(exported.path); let hasDensity = false;
  for (let offset = 8; offset < printed.length;) { const length = printed.readUInt32BE(offset); if (printed.toString('ascii', offset + 4, offset + 8) === 'pHYs') hasDensity = Math.abs(printed.readUInt32BE(offset + 8) * 0.0254 - 300) < 0.02; offset += length + 12; }
  check('PNG production export retains pixels, alpha and 300 DPI', hasDensity && PNG.sync.read(printed).data.equals(rgba.data));
  const pdf = await api('export', { name: '测试制作.pdf', data: encoded, base64: true, print });
  const pdfText = fs.readFileSync(pdf.path).toString('latin1');
  const box = pdfText.match(/\/MediaBox \[0 0 ([\d.]+) ([\d.]+)\]/);
  check('PDF has exact physical page dimensions', pdfText.startsWith('%PDF-1.6') && box && Math.abs(Number(box[1]) - 297 * 72 / 25.4) < 1e-6 && Math.abs(Number(box[2]) - 210 * 72 / 25.4) < 1e-6);
  const zip = await api('export', { name: '素材包.zip', files: [{ name: 'parts/左前门.svg', data: '<svg/>' }, { name: '原图.png', data: encoded, base64: true }] });
  const zipBytes = fs.readFileSync(zip.path);
  check('ZIP keeps nested component files and Chinese names', zipBytes.subarray(0, 2).toString() === 'PK' && zipBytes.includes(Buffer.from('parts/左前门.svg')));
  await assert.rejects(api('export', { name: 'x.zip', files: [{ name: '../escape.txt', data: 'x' }] })); check('ZIP traversal is rejected', true);
  const download = await get(exported.url); check('Export download returns the saved bytes', download.ok && Buffer.from(await download.arrayBuffer()).equals(printed));
  const forbidden = await fetch(ready.origin + '/__api/save', { method: 'POST', headers: { ...headers, Origin: 'https://invalid.example' }, body: JSON.stringify({ project: p }) });
  check('Cross-origin mutations are rejected', forbidden.status === 400);
})().catch(error => { failures.push(error.stack); console.error(error); process.exitCode = 1; }).finally(async () => {
  await backend.stop();
  fs.mkdirSync(output, { recursive: true });
  fs.writeFileSync(path.join(output, 'verification.json'), JSON.stringify({ time: new Date().toISOString(), backend: 'Rust', checks, failures, data }, null, 2));
  console.log(`Completed ${checks.length} checks, ${failures.length} failures`);
});
