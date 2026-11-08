// Real-model decal seam and resizable inspector regression.
const { _electron: electron } = require('../desktop/node_modules/playwright-core');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const assert = require('node:assert/strict');

const root = path.resolve(__dirname, '..');
const data = fs.mkdtempSync(path.join(os.tmpdir(), 'itasha-wrap-panels-'));
const output = path.join(__dirname, 'acceptance', 'surface-wrap-panels');
fs.mkdirSync(output, { recursive: true });
let app;

(async () => {
  app = await electron.launch({
    executablePath: path.join(root, 'desktop/electron/痛车编辑器.exe'),
    cwd: root,
    env: { ...process.env, ITASHA_ROOT: root, ITASHA_DATA_DIR: data, ITASHA_PROFILE_DIR: path.join(data, 'profile'), ITASHA_PORT: '0', ITASHA_TEST_MODE: '1' },
    timeout: 60000
  });
  const page = await app.firstWindow();
  await app.evaluate(({ BrowserWindow }) => { const w = BrowserWindow.getAllWindows()[0]; w.setPosition(-30000, -30000, false); w.showInactive(); });
  await page.setViewportSize({ width: 1600, height: 900 });
  await page.waitForFunction(() => window.studio);
  await page.evaluate(() => localStorage.setItem('su7-studio-settings', JSON.stringify({ layout: 'both', quality: 'medium', autosave: false, live: false })));
  await page.reload();
  await page.waitForFunction(() => window.studio);
  await page.click('#home-new');
  await page.fill('#new-project-name', '真实车型跨区验收');
  await page.click('#new-project-create');
  await page.waitForFunction(() => studio.ready, null, { timeout: 120000 });
  await page.click('#configuration-confirm');
  await page.waitForFunction(() => studio.stage === 'edit');
  const rect = async selector => page.locator(selector).evaluate(el => { const r = el.getBoundingClientRect(); return { x: r.x, y: r.y, width: r.width, height: r.height }; });
  const checkPanelBounds = async () => {
    const workspace = await rect('#workspace'), inspector = await rect('.editor-detail'), core = await rect('.inspector-core'), assets = await rect('#asset-library'), layers = await rect('.layer-section'), properties = await rect('.properties');
    const inside = (a, b) => a.x >= b.x - 1 && a.y >= b.y - 1 && a.x + a.width <= b.x + b.width + 1 && a.y + a.height <= b.y + b.height + 1;
    assert.ok(inside(inspector, workspace), 'inspector stays inside the workspace');
    assert.ok(inside(assets, inspector), 'asset area stays inside the inspector');
    assert.ok(inside(layers, core) && inside(properties, core), 'layer and property areas stay inside the core');
    assert.ok(layers.y + layers.height <= properties.y + 1, 'layer and property areas do not overlap');
  };
  const drag = async (selector, dx, dy) => {
    const r = await rect(selector);
    await page.mouse.move(r.x + r.width / 2, r.y + r.height / 2);
    await page.mouse.down();
    await page.mouse.move(r.x + r.width / 2 + dx, r.y + r.height / 2 + dy, { steps: 5 });
    await page.mouse.up();
  };
  const startPanels = { inspector: await rect('.editor-detail'), assets: await rect('#asset-library'), layers: await rect('.layer-section'), properties: await rect('.properties') };
  await drag('#inspector-width-splitter', -36, 0);
  await drag('#asset-panel-splitter', -28, 0);
  await drag('#layer-properties-splitter', 0, 30);
  const resizedPanels = { inspector: await rect('.editor-detail'), assets: await rect('#asset-library'), layers: await rect('.layer-section'), properties: await rect('.properties') };
  assert.ok(resizedPanels.inspector.width > startPanels.inspector.width + 25, 'outer divider resizes the inspector width');
  assert.ok(resizedPanels.assets.width > startPanels.assets.width + 20, 'asset divider resizes the asset library independently');
  assert.ok(resizedPanels.layers.height > startPanels.layers.height + 20, 'layer divider resizes the layer list height');
  await checkPanelBounds();
  await page.screenshot({ path: path.join(output, 'desktop-layout.png') });
  let storedLayout = await page.evaluate(() => JSON.parse(localStorage.getItem('su7-studio-settings')));
  assert.ok(storedLayout.inspectorWidth >= 350 && storedLayout.assetWidth > 180 && storedLayout.layerHeight > 220, 'panel dimensions persist in local studio settings');
  await page.setViewportSize({ width: 900, height: 650 });
  await page.waitForTimeout(150);
  assert.equal(await page.locator('#asset-panel-splitter').getAttribute('data-axis'), 'horizontal', 'asset splitter follows the stacked mobile layout');
  const narrowAsset = await rect('#asset-library'), narrowLayer = await rect('.layer-section');
  await drag('#asset-panel-splitter', 0, -22);
  await drag('#layer-properties-splitter', 0, -18);
  assert.ok((await rect('#asset-library')).height > narrowAsset.height + 15, 'asset area remains resizable when stacked');
  assert.ok((await rect('.layer-section')).height < narrowLayer.height - 10, 'layer height remains resizable at narrow widths');
  await checkPanelBounds();
  await page.screenshot({ path: path.join(output, 'compact-layout.png') });
  await page.setViewportSize({ width: 720, height: 520 });
  await page.waitForTimeout(150);
  await page.locator('#inspector-width-splitter').focus();
  const narrowInspector = await rect('.editor-detail');
  await page.keyboard.press('ArrowRight');
  assert.ok((await rect('.editor-detail')).width < narrowInspector.width, 'outer divider supports keyboard resizing at compact widths');
  await checkPanelBounds();
  storedLayout = await page.evaluate(() => JSON.parse(localStorage.getItem('su7-studio-settings')));
  assert.ok(storedLayout.assetHeight > 190 && storedLayout.layerHeight < narrowLayer.height, 'stacked panel dimensions persist independently');
  await page.click('[data-layout="3d"]');
  assert.ok((await rect('.editor-detail')).width < 250 && await page.locator('.inspector-core').isHidden() && await page.locator('#asset-panel-splitter').isHidden(), 'preview-only mode keeps a compact asset pane');
  await page.click('[data-layout="both"]');
  console.log('PASS inspector, asset, layer, and property splitters resize and persist across desktop and compact layouts');
  const frame = page.frames().find(f => f.url().includes('/configurator/'));
  assert.ok(frame, 'the real SU7 configurator loaded');

  const diagnostics = await frame.evaluate(() => {
    const sceneRuntime = Object.values(__su7Scenes).find(r => r.scene.getObjectByName('carPaint_external_4'));
    const meshes = [];
    sceneRuntime.scene.traverse(mesh => { if (mesh.isMesh && mesh.userData.studioFaces?.length) meshes.push(mesh); });
    const graph = new SU7SurfaceWrap(meshes, sceneRuntime.Vector3);
    const labels = new Map();
    for (const face of graph.faces) for (const id of face.ids) {
      let parts = labels.get(id);
      if (!parts) labels.set(id, parts = new Set());
      parts.add(face.part);
    }
    const originalLinks = graph.vertices.map(() => new Set());
    for (const face of graph.faces) for (let i = 0; i < 3; i++) {
      const a = face.ids[i], b = face.ids[(i + 1) % 3];
      originalLinks[a].add(b); originalLinks[b].add(a);
    }
    const component = new Int32Array(graph.vertices.length).fill(-1), componentParts = [];
    for (let root = 0; root < graph.vertices.length; root++) {
      if (component[root] >= 0) continue;
      const id = componentParts.length, queue = [root], parts = new Set();
      component[root] = id;
      for (let head = 0; head < queue.length; head++) {
        const vertex = queue[head];
        for (const face of graph.vertices[vertex].faces) parts.add(graph.faces[face].part);
        for (const next of originalLinks[vertex]) if (component[next] < 0) { component[next] = id; queue.push(next); }
      }
      componentParts.push(parts);
    }
    const cell = .065, key = p => [p.x, p.y, p.z].map(v => Math.floor(v / cell)).join(':');
    const grid = new Map();
    for (let id = 0; id < graph.vertices.length; id++) {
      const k = key(graph.vertices[id].p);
      if (!grid.has(k)) grid.set(k, []);
      grid.get(k).push(id);
    }
    const targets = [['body', 'bonnet'], ['frontBumper', 'bonnet']];
    const best = new Map();
    let trunkIsland = null;
    for (let a = 0; a < graph.vertices.length; a++) {
      const va = graph.vertices[a], ka = key(va.p).split(':').map(Number);
      for (let x = -1; x <= 1; x++) for (let y = -1; y <= 1; y++) for (let z = -1; z <= 1; z++) {
        for (const b of grid.get([ka[0] + x, ka[1] + y, ka[2] + z].join(':')) || []) {
          if (a >= b) continue;
          const vb = graph.vertices[b], d = va.p.distanceTo(vb.p), normal = va.n.clone().add(vb.n).normalize();
          if (d < 1e-4 || d >= cell || Math.abs(vb.p.clone().sub(va.p).dot(normal)) >= .025 || va.n.dot(vb.n) < .4) continue;
          const ca = component[a], cb = component[b];
          if (ca !== cb && componentParts[ca].has('trunk') && componentParts[cb].has('trunk') && (!trunkIsland || d < trunkIsland.distance)) {
            const faceId = graph.vertices[a].faces.find(id => graph.faces[id].part === 'trunk');
            trunkIsland = { distance: d, start: a, faceId, components: [ca, cb] };
          }
          for (const pa of labels.get(a) || []) for (const pb of labels.get(b) || []) {
            if (pa === pb) continue;
            const pair = targets.find(([p, q]) => p === pa && q === pb || p === pb && q === pa);
            if (!pair) continue;
            const name = pair.join(':');
            if (!best.has(name) || d < best.get(name).distance) {
              const start = pa === pair[0] ? a : b;
              const faceId = graph.vertices[start].faces.find(id => graph.faces[id].part === pair[0]);
              best.set(name, { distance: d, start, faceId, parts: pair });
            }
          }
        }
      }
    }
    const coverage = {};
    for (const [name, seam] of best) {
      const face = graph.faces[seam.faceId], point = graph.vertices[seam.start].p;
      const hit = { mesh: face.mesh.name, face: face.index, point: point.toArray(), normal: face.normal.toArray(), region: face.region };
      const tangent = graph.frame(hit, face.region);
      const wrap = graph.make({ ...hit, ...tangent, width: .5, height: .5, at: [.5, .5] });
      coverage[name] = { distance: seam.distance, region: face.region, mesh: face.mesh.name, point: point.toArray(), parts: [...new Set(wrap.patches.map(p => p.part))], patches: wrap.patches.length };
    }
    if (trunkIsland) {
      const face = graph.faces[trunkIsland.faceId], point = graph.vertices[trunkIsland.start].p;
      const hit = { mesh: face.mesh.name, face: face.index, point: point.toArray(), normal: face.normal.toArray(), region: face.region };
      const tangent = graph.frame(hit, face.region);
      const wrap = graph.make({ ...hit, ...tangent, width: .36, height: .36, at: [.5, .5] });
      const included = new Set();
      for (const patch of wrap.patches) for (const index of patch.faces) {
        const faceId = graph.byMesh.get(patch.mesh)[index];
        included.add(component[graph.faces[faceId].ids[0]]);
      }
      coverage.trunkIslands = {
        distance: trunkIsland.distance,
        part: face.part,
        region: face.region,
        mesh: face.mesh.name,
        point: point.toArray(),
        components: trunkIsland.components,
        includedComponents: trunkIsland.components.filter(id => included.has(id)),
        parts: [...new Set(wrap.patches.map(p => p.part))]
      };
    }
    return {
      meshes: meshes.map(m => ({ name: m.name, faces: m.userData.studioFaces.length, parts: [...new Set(m.userData.studioFaces.map(f => f.part))] })),
      seams: graph.seams,
      coverage
    };
  });
  fs.writeFileSync(path.join(output, 'real-model-diagnostics.json'), JSON.stringify(diagnostics, null, 2));
  for (const name of ['body:bonnet', 'frontBumper:bonnet']) {
    const result = diagnostics.coverage[name];
    assert.ok(result, `real SU7 model exposes the ${name} seam`);
    for (const part of name.split(':')) assert.ok(result.parts.includes(part), `${name} decal covers ${part}`);
    for (const part of ['rearBumper', 'trunk', 'doorLB', 'doorRB']) assert.ok(!result.parts.includes(part), `${name} decal does not leak onto ${part}`);
    console.log(`PASS real SU7 decal crosses ${name} without reaching unrelated panels`);
  }
  const trunk = diagnostics.coverage.trunkIslands;
  assert.ok(trunk, 'real SU7 trunk contains a nearby split paint island');
  assert.equal(trunk.includedComponents.length, 2, 'trunk decal covers both sides of the split paint island');
  assert.deepEqual(trunk.parts, ['trunk'], 'trunk decal stays on the selected painted panel');
  console.log('PASS real SU7 trunk decal covers both split islands without leaking to another panel');
  assert.ok(diagnostics.seams > 0, 'surface graph builds the body seam network');
  console.log(JSON.stringify(diagnostics, null, 2));
})().catch(error => { console.error(error); process.exitCode = 1; })
  .finally(async () => { if (app) await app.close().catch(() => {}); });
