import assert from 'node:assert/strict';
import {spawn} from 'node:child_process';
import {createHash, randomUUID} from 'node:crypto';
import fs from 'node:fs';
import {createRequire} from 'node:module';
import net from 'node:net';
import path from 'node:path';
import {fileURLToPath} from 'node:url';
import {validateFullAuditEvidence} from './whole-car-audit-evidence.mjs';
import {parsePaintMap} from './paint-map-json.mjs';

// Standalone native smoke. The runtime manifest and the independent metric
// audit choose the map and seam witnesses; no model-specific UV coordinates
// or historical triangle counts are copied into this test.
const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..');
const require = createRequire(import.meta.url);
const {chromium} = require('playwright-core');
const {PNG} = require('pngjs');
const reportDir = path.join(root, '.reports', 'whole-car-seams-' + Date.now());
const dataDir = path.join(reportDir, 'data');
fs.mkdirSync(reportDir, {recursive: true});
const pause = ms => new Promise(resolve => setTimeout(resolve, ms));
const save = (name, value) => fs.writeFileSync(path.join(reportDir, name), JSON.stringify(value, null, 2));
const sha256 = value => createHash('sha256').update(value).digest('hex');
const mutation = project => ({requestId: randomUUID(), session: project.session, expectedRevision: project.revision});
const pairKey = pair => [...pair].sort().join('|');
const preflightOnly = process.argv.includes('--preflight');
const result = {schema: 1, mode: preflightOnly ? 'preflight' : 'desktop', startedAt: new Date().toISOString(), ok: false, masks: [], cases: [], errors: []};
let child, browser, stderr = '';

function preflight() {
  const paintRoot = path.join(root, 'assets', 'paint');
  const manifest = JSON.parse(fs.readFileSync(path.join(paintRoot, 'manifest.json'), 'utf8'));
  const mapPath = path.resolve(paintRoot, manifest.runtimePaint);
  assert(mapPath.startsWith(paintRoot + path.sep), 'runtimePaint must stay inside assets/paint');
  const bytes = fs.readFileSync(mapPath);
  const record = manifest.records.find(record => record.path === manifest.runtimePaint);
  assert(record, 'manifest must record its runtime paint map');
  assert.equal(bytes.length, record.bytes, 'runtime map byte count differs from manifest');
  assert.equal(sha256(bytes), record.sha256, 'runtime map digest differs from manifest');
  const map = parsePaintMap(bytes);
  assert.equal(map.paintFingerprint, manifest.paintFingerprint);
  assert.equal(map.modelFingerprint, manifest.modelFingerprint);
  const auditPath = path.join(root, '.reports', 'seam-metric-audit.json');
  const audit = JSON.parse(fs.readFileSync(auditPath, 'utf8'));
  assert.equal(audit.paintFingerprint, manifest.paintFingerprint, 'regenerate the metric audit for the current runtime map');
  assert.equal(audit.triangles, map.triangles.length, 'audit triangle count is stale');

  const counts = new Map();
  const triangleParts = new Map();
  for (const triangle of map.triangles) {
    counts.set(triangle.partId, (counts.get(triangle.partId) || 0) + 1);
    // Exact u64 spellings must survive both triangle and connection parsing.
    const key = JSON.stringify(triangle.id);
    const previous = triangleParts.get(key);
    assert(!previous || previous === triangle.partId, 'ambiguous connection endpoint part');
    triangleParts.set(key, triangle.partId);
  }
  assert.equal(counts.size, 18, 'SU7 runtime must expose all 18 reviewed paint masks');
  const {connectedPairs, authoredSeams, queries} = validateFullAuditEvidence(map, audit, triangleParts);
  const groups = new Map();
  for (const seed of audit.representativeSeeds) {
    assert(connectedPairs.has(pairKey(seed.pair)), 'audit contains an unauthored part pair');
    const key = pairKey(seed.pair) + ':' + seed.source + '>' + seed.target;
    if (!groups.has(key)) groups.set(key, []);
    groups.get(key).push(seed);
  }
  assert.equal(groups.size, connectedPairs.size * 2, 'each authored part pair needs forward and reverse representative seeds');
  const seeds = [];
  for (const pair of [...connectedPairs].sort()) {
    const [a, b] = pair.split('|');
    for (const [source, target] of [[a, b], [b, a]]) {
      const candidates = groups.get(pair + ':' + source + '>' + target);
      assert(candidates?.length, 'missing direction: ' + source + ' -> ' + target);
      const valid = candidates.filter(seed => seed.crossingSucceeded && seed.sourceUnchanged && seed.finiteImageUv &&
        seed.sourceCenter?.length === 2 && seed.sourceCenter.every(Number.isFinite) && seed.targetAreaMm2 > 0);
      assert(valid.length, 'no successful audited seed: ' + source + ' -> ' + target);
      // Balance the two sides to improve the visible native witness. The
      // smoke itself rechecks the placement with a different aspect/angle.
      valid.sort((x, y) => Math.min(y.sourceAreaMm2, y.targetAreaMm2) - Math.min(x.sourceAreaMm2, x.targetAreaMm2));
      seeds.push(valid[0]);
    }
  }
  const modelRoot = path.resolve(root, process.env.ITASHA_MODEL_ROOT || '../../3D');
  assert(fs.existsSync(modelRoot), 'model root missing: ' + modelRoot);
  const executables = process.env.ITASHA_SMOKE_EXE
    ? [path.resolve(root, process.env.ITASHA_SMOKE_EXE)]
    : ['target/debug/itasha-desktop.exe', 'target/release/itasha-desktop.exe']
      .map(relative => path.join(root, relative)).filter(candidate => fs.existsSync(candidate))
      .sort((a, b) => fs.statSync(b).mtimeMs - fs.statSync(a).mtimeMs);
  const executable = executables[0];
  assert(executable && fs.existsSync(executable), 'build the desktop app or set ITASHA_SMOKE_EXE');
  result.runtime = {runtimePaint: manifest.runtimePaint, paintFingerprint: manifest.paintFingerprint,
    modelFingerprint: manifest.modelFingerprint, mapSha256: record.sha256, mapTriangles: map.triangles.length,
    partPairs: connectedPairs.size, authoredSeams, auditQueries: queries, directions: seeds.length, auditPath, modelRoot, executable,
    executableModified: fs.statSync(executable).mtime.toISOString(), dataDir};
  save('selected-seeds.json', seeds);
  return {manifest, counts, seeds, modelRoot, executable};
}

async function freePort() {
  const server = net.createServer();
  await new Promise((resolve, reject) => {server.once('error', reject); server.listen(0, '127.0.0.1', resolve);});
  const port = server.address().port;
  await new Promise(resolve => server.close(resolve));
  return port;
}

async function connect(executable) {
  const port = await freePort();
  child = spawn(executable, [], {cwd: root, windowsHide: true, env: {...process.env,
    ITASHA_TEST_DATA_DIR: dataDir, WEBVIEW2_USER_DATA_FOLDER: path.join(reportDir, 'webview'),
    WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: '--remote-debugging-port=' + port}});
  child.stderr.on('data', chunk => {stderr += chunk;});
  let spawnError;
  child.on('error', error => {spawnError = error;});
  for (let attempt = 0; attempt < 150; attempt++) {
    if (spawnError) throw spawnError;
    if (child.exitCode !== null) throw Error('desktop exited: ' + stderr);
    try {browser = await chromium.connectOverCDP('http://127.0.0.1:' + port); break;} catch {await pause(200);}
  }
  assert(browser, 'WebView2 debugging endpoint did not open');
  let page;
  for (let attempt = 0; attempt < 100; attempt++) {
    page = browser.contexts()[0]?.pages().find(page => !page.url().startsWith('devtools:'));
    if (page) break;
    await pause(100);
  }
  assert(page, 'WebView2 page missing');
  page.setDefaultTimeout(30_000);
  page.on('pageerror', error => result.errors.push(error.message));
  await page.getByText('桌面主框架就绪', {exact: true}).waitFor();
  return page;
}

async function coverageCheck(page, session, layer, target) {
  return page.evaluate(async ({session, layer, target}) => {
    const invoke = window.__TAURI_INTERNALS__.invoke;
    const cross = await invoke('viewport_coverage', {session, layer});
    const local = await invoke('viewport_coverage', {session,
      layer: {...layer, placement: {...layer.placement, crossSurface: false}}});
    const area = points => Math.abs(points.reduce((sum, p, i) => {
      const next = points[(i + 1) % points.length]; return sum + p[0] * next[1] - p[1] * next[0];
    }, 0)) / 2;
    const sorted = fragments => fragments.map(fragment => JSON.stringify(fragment)).sort();
    const source = sorted(cross.filter(fragment => fragment.partId === layer.placement.surfaceId));
    const sourceLocal = sorted(local);
    const size = layer.placement.widthMm * layer.placement.heightMm;
    const parts = {};
    let invalid = 0;
    for (const fragment of cross) {
      if (fragment.imageUv.length !== 3 || fragment.paintUv.length !== 3 ||
        ![...fragment.imageUv.flat(), ...fragment.paintUv.flat()].every(Number.isFinite) ||
        fragment.imageUv.flat().some(value => value < -0.001 || value > 1.001)) invalid++;
      const summary = parts[fragment.partId] ||= {fragments: 0, imageAreaMm2: 0};
      summary.fragments++;
      summary.imageAreaMm2 += area(fragment.imageUv) * size;
    }
    const candidates = {};
    for (const part of [layer.placement.surfaceId, target]) {
      const ranked = cross.filter(fragment => fragment.partId === part).map(fragment => ({
        point: [0, 1].map(axis => fragment.paintUv.reduce((sum, point) => sum + point[axis], 0) / 3),
        imageUv: [0, 1].map(axis => fragment.imageUv.reduce((sum, point) => sum + point[axis], 0) / 3),
        areaMm2: area(fragment.imageUv) * size,
      })).sort((a, b) => b.areaMm2 - a.areaMm2);
      candidates[part] = [];
      for (const candidate of ranked) {
        if (candidates[part].every(previous => Math.hypot(...previous.point.map((v, i) => v - candidate.point[i])) > 2))
          candidates[part].push(candidate);
        if (candidates[part].length >= 10) break;
      }
    }
    return {crossCount: cross.length, localCount: local.length, invalid, parts, candidates,
      sourceUnchanged: source.length === sourceLocal.length && source.every((value, index) => value === sourceLocal[index]),
      localOnlySource: local.every(fragment => fragment.partId === layer.placement.surfaceId)};
  }, {session, layer, target});
}

function sourceColor(image, uv, flipX) {
  let [u, v] = uv;
  if (flipX) u = 1 - u;
  const x = Math.max(0, Math.min(image.width - 1, Math.floor(u * image.width)));
  const y = Math.max(0, Math.min(image.height - 1, Math.floor(v * image.height)));
  const [r, g, b] = image.data.subarray((y * image.width + x) * 4, (y * image.width + x) * 4 + 3);
  if (r > 225 && g < 60 && b > 100 && b < 145) return 'magenta';
  if (r > 245 && g > 200 && b < 75) return 'yellow';
  if (r < 65 && g > 205 && b > 210) return 'cyan';
  return null;
}

function gpuMatches(kind, rgb) {
  const [r, g, b] = rgb;
  if (kind === 'magenta') return r > g * 1.3 && b > g * 1.15 && r > 35;
  if (kind === 'yellow') return r > b * 1.3 && g > b * 1.3 && g > 35;
  if (kind === 'cyan') return g > r * 1.3 && b > r * 1.3 && b > 35;
  return false;
}

async function probes(page, status, id, part) {
  return page.evaluate(async ({width, height, id, part}) => {
    const points = [];
    const seen = new Set();
    const add = (x, y) => {
      x = Math.floor(x) + 0.5; y = Math.floor(y) + 0.5;
      const key = x + ':' + y;
      if (x > 1 && y > 1 && x < width - 1 && y < height - 1 && !seen.has(key)) {points.push({x, y}); seen.add(key);}
    };
    // A centred focus witness must be sampled densely enough for narrow trim
    // and C-ring faces; the wider grid also records the surrounding body.
    for (let y = -32; y <= 32; y += 4) for (let x = -32; x <= 32; x += 4) add(width / 2 + x, height / 2 + y);
    for (let y = .15; y <= .85; y += .07) for (let x = .15; x <= .85; x += .07) add(width * x, height * y);
    const hits = [];
    for (let start = 0; start < points.length; start += 32) {
      const batch = await Promise.all(points.slice(start, start + 32).map(async point => ({...point,
        hit: await window.__TAURI_INTERNALS__.invoke('viewport_probe', point)})));
      hits.push(...batch.filter(point => point.hit?.layerId === id && point.hit.surfaceId === part));
    }
    return {tested: points.length, hits};
  }, {width: status.width, height: status.height, id, part});
}

async function main() {
  const {manifest, counts, seeds, modelRoot, executable} = preflight();
  const page = await connect(executable);
  const ipc = (command, args) => page.evaluate(({command, args}) => window.__TAURI_INTERNALS__.invoke(command, args), {command, args});
  const waitRenderer = async (predicate = () => true) => {
    let status;
    for (let attempt = 0; attempt < 200; attempt++) {
      status = await ipc('viewport_status');
      if (status.state === 'error') throw Error(status.message);
      if (status.state === 'ready' && status.visible && status.nativeVisible && status.nativeFrontmost &&
        status.frames > 0 && !status.temporalPending && predicate(status)) return status;
      await pause(150);
    }
    throw Error('renderer wait timed out: ' + JSON.stringify(status));
  };
  const edit = async operation => {
    const project = (await ipc('read_view')).project;
    await ipc('execute', {requestId: randomUUID(), action: {type: 'edit', mutation: mutation(project), operation}});
    return (await ipc('read_view')).project;
  };
  const settings = (await ipc('read_view')).settings;
  await ipc('execute', {requestId: randomUUID(), action: {type: 'settings', value: {...settings,
    layout: '3d', paths: {...settings.paths, models: modelRoot},
    performance: {...settings.performance, renderScale: 100}, scene: {...settings.scene, timeMinutes: 840}}}});
  await page.reload();
  await page.getByText('桌面主框架就绪', {exact: true}).waitFor();
  await page.getByRole('button', {name: /新建设计/}).click();
  await page.getByRole('button', {name: /轿车.*SU7 Max/}).click();
  await page.getByRole('button', {name: '下一步：车型选配 →', exact: true}).click();
  await page.getByRole('textbox', {name: '设计名称', exact: true}).fill('WHOLE_CAR_SEAMS_' + Date.now());
  await page.getByRole('button', {name: '完成配置，进入工作区 →', exact: true}).click();
  await page.locator('.workspace').waitFor();
  let status = await waitRenderer();
  assert.equal(status.stats.surfaceFingerprint, manifest.paintFingerprint, 'desktop binary embeds a different paint map');
  assert.equal(status.surfaces.length, counts.size);
  assert.deepEqual(status.surfaces.map(surface => surface.id).sort(), [...counts.keys()].sort());
  let project = (await ipc('read_view')).project;
  for (const surface of status.surfaces) {
    const mask = await page.evaluate(async ({session, id}) => {
      const data = new Float64Array(await window.__TAURI_INTERNALS__.invoke('viewport_surface', {session, id}));
      let nonfinite = 0, nonpositive = 0;
      for (const value of data) if (!Number.isFinite(value)) nonfinite++;
      for (let index = 0; index < data.length; index += 6)
        if (Math.abs((data[index + 2] - data[index]) * (data[index + 5] - data[index + 1]) -
          (data[index + 3] - data[index + 1]) * (data[index + 4] - data[index])) <= 0) nonpositive++;
      return {values: data.length, triangles: data.length / 6, nonfinite, nonpositive};
    }, {session: project.session, id: surface.id});
    assert.equal(mask.values % 6, 0);
    assert.equal(mask.triangles, counts.get(surface.id), surface.id + ' runtime mask differs from map');
    assert.equal(surface.triangles, counts.get(surface.id), surface.id + ' runtime status differs from map');
    assert.equal(mask.nonfinite, 0, surface.id + ' mask has nonfinite values');
    assert.equal(mask.nonpositive, 0, surface.id + ' runtime mask contains a degenerate triangle');
    assert(mask.triangles > 0);
    result.masks.push({id: surface.id, ...mask});
  }
  save('runtime-masks.json', result.masks);

  const generated = await page.evaluate(() => {
    const canvas = document.createElement('canvas'); canvas.width = 512; canvas.height = 320;
    const context = canvas.getContext('2d');
    const colors = ['#ed2679', '#ffd739', '#22dcf2'];
    for (let row = 0; row < 5; row++) for (let column = 0; column < 8; column++) {
      context.fillStyle = colors[(column + row * 2) % colors.length]; context.fillRect(column * 64, row * 64, 64, 64);
      context.fillStyle = '#111111'; context.font = 'bold 18px Arial'; context.fillText(String(row * 8 + column + 1), column * 64 + 6, row * 64 + 23);
    }
    context.fillStyle = '#ffffff'; context.beginPath(); context.moveTo(35, 141); context.lineTo(365, 141);
    context.lineTo(365, 104); context.lineTo(477, 169); context.lineTo(365, 235); context.lineTo(365, 192);
    context.lineTo(35, 192); context.closePath(); context.fill();
    context.fillStyle = '#111111'; context.font = 'bold 24px Arial'; context.fillText('START 01', 12, 305); context.fillText('END 40', 400, 305);
    return canvas.toDataURL('image/png').split(',')[1];
  });
  const sourcePath = path.join(reportDir, 'checker-numbers-arrow.png');
  fs.writeFileSync(sourcePath, Buffer.from(generated, 'base64'));
  const asset = await ipc('import_image_path', {mutation: mutation(project), path: sourcePath});
  project = (await ipc('read_view')).project;
  assert(project.editor.assets.some(value => value.id === asset.id), 'real image import must enter the document asset library');
  const first = seeds[0];
  const layerId = randomUUID(), guideId = randomUUID();
  const layer = {id: layerId, name: '棋盘数字方向箭头', visible: true, locked: false, opacity: 1,
    source: {kind: 'image', assetId: asset.id}, placement: {profileVersion: 2, surfaceId: first.source,
      xMm: first.sourceCenter[0], yMm: first.sourceCenter[1], widthMm: 540, heightMm: 360, rotationDeg: 31,
      crossSurface: true, flipX: false, aspectRatioLocked: false}};
  project = await edit({type: 'insert', layer, fingerprint: manifest.paintFingerprint});
  const guide = {id: guideId, name: '测试相机定位', visible: false, locked: false, opacity: 0,
    source: {kind: 'shape', shape: 'rectangle', fill: '#ffffff'}, placement: {...layer.placement,
      widthMm: 180, heightMm: 180, rotationDeg: 0, crossSurface: false}};
  project = await edit({type: 'insert', layer: guide, fingerprint: manifest.paintFingerprint});
  // Decode the asset returned by the real import path, then use the same raw
  // RGBA renderer command as useCanvasEditor. No synthetic renderer texture.
  const imported = await page.evaluate(async ({project, assetId, layerId}) => {
    const invoke = window.__TAURI_INTERNALS__.invoke;
    const bytes = await invoke('asset_bytes', {session: project.session, id: assetId});
    const url = URL.createObjectURL(new Blob([bytes], {type: 'image/png'}));
    try {
      const image = new Image(); image.src = url; await image.decode();
      const canvas = document.createElement('canvas'); canvas.width = image.width; canvas.height = image.height;
      const context = canvas.getContext('2d'); context.drawImage(image, 0, 0);
      await invoke('viewport_decal', context.getImageData(0, 0, canvas.width, canvas.height).data.buffer, {headers: {
        'x-project': project.session.projectId, 'x-session': project.session.sessionEpoch, 'x-revision': project.revision,
        'x-layer': layerId, 'x-width': String(canvas.width), 'x-height': String(canvas.height),
      }});
      return {width: canvas.width, height: canvas.height, png: canvas.toDataURL('image/png').split(',')[1]};
    } finally {URL.revokeObjectURL(url);}
  }, {project, assetId: asset.id, layerId});
  assert.equal(imported.width, 512); assert.equal(imported.height, 320);
  fs.writeFileSync(path.join(reportDir, 'imported-checker.png'), Buffer.from(imported.png, 'base64'));
  const sourceImage = PNG.sync.read(Buffer.from(imported.png, 'base64'));
  save('image-import.json', {asset, layerId, width: imported.width, height: imported.height,
    importedPngSha256: sha256(Buffer.from(imported.png, 'base64'))});

  const capturePath = path.join(dataDir, 'native-viewport.png');
  const capture = async (name, previousMtime) => {
    let lastError;
    for (let attempt = 0; attempt < 100; attempt++) {
      try {
        if (fs.statSync(capturePath).mtimeMs <= previousMtime) {await pause(75); continue;}
        const bytes = fs.readFileSync(capturePath);
        const image = PNG.sync.read(bytes);
        fs.writeFileSync(path.join(reportDir, name), bytes);
        return image;
      } catch (error) {lastError = error; await pause(75);}
    }
    throw Error('native GPU readback did not update: ' + (lastError?.message || name));
  };
  const visibleWitness = async (seed, coverage, index, part) => {
    const attempts = [];
    for (const [candidateIndex, candidate] of coverage.candidates[part].entries()) {
      const guideLayer = {...guide, placement: {...guide.placement, surfaceId: part,
        xMm: candidate.point[0], yMm: candidate.point[1]}};
      delete guideLayer.placement.anchor;
      project = await edit({type: 'update', layer: guideLayer});
      // Wait for this document to reach the GPU before using frame counts as a
      // focus barrier; an older document redraw is not a camera witness.
      const updated = await waitRenderer(value => value.renderedRevision === project.revision);
      await ipc('viewport_select', {session: project.session, id: guideId});
      const before = await waitRenderer(value => value.renderedRevision === project.revision &&
        value.selectedLayer === guideId && (updated.selectedLayer === guideId || value.frames > updated.frames));
      const previousMtime = fs.existsSync(capturePath) ? fs.statSync(capturePath).mtimeMs : 0;
      await ipc('viewport_focus');
      const focused = await waitRenderer(value => value.renderedRevision === project.revision && value.frames > before.frames && value.selectedLayer === guideId);
      const filename = String(index + 1).padStart(2, '0') + '-' + part + '-focus-' + candidateIndex + '.png';
      const image = await capture(filename, previousMtime);
      const queried = await probes(page, focused, layerId, part);
      const matches = [];
      for (const sample of queried.hits) {
        const kind = sourceColor(sourceImage, sample.hit.imageUv, layer.placement.flipX);
        const x = Math.min(image.width - 1, Math.floor(sample.x * image.width / focused.width));
        const y = Math.min(image.height - 1, Math.floor(sample.y * image.height / focused.height));
        const rgb = [...image.data.subarray((y * image.width + x) * 4, (y * image.width + x) * 4 + 3)];
        if (kind && gpuMatches(kind, rgb)) matches.push({...sample, sourceColor: kind, gpuRgb: rgb});
      }
      const evidence = {part, candidateIndex, candidate, screenshot: filename, tested: queried.tested,
        artworkHits: queried.hits.length, matchedColors: matches.length,
        colors: [...new Set(matches.map(value => value.sourceColor))], cameraEye: focused.cameraEye,
        cameraDistance: focused.cameraDistance, viewport: [focused.width, focused.height],
        gpuReadback: [image.width, image.height], samples: matches.slice(0, 24)};
      attempts.push(evidence);
      if (matches.length >= 3) return {...evidence, attempts: attempts.length};
    }
    save('failed-visible-' + index + '-' + part + '.json', {seed, attempts});
    throw Error('no real GPU/probe artwork witness on ' + part + ' for ' + seed.source + ' -> ' + seed.target);
  };

  for (const [index, seed] of seeds.entries()) {
    layer.placement = {...layer.placement, surfaceId: seed.source, xMm: seed.sourceCenter[0], yMm: seed.sourceCenter[1],
      // Alternate reflection as well as a non-square physical size and angle.
      widthMm: 540, heightMm: 360, rotationDeg: index % 2 ? -23 : 31, flipX: Boolean(index % 2)};
    delete layer.placement.anchor;
    project = await edit({type: 'update', layer});
    const actual = project.editor.layers.find(value => value.id === layerId);
    assert(actual.placement.anchor, 'real insert/update must persist a current stable surface anchor');
    assert.equal(actual.placement.anchor.paintSurfaceFingerprint, manifest.paintFingerprint);
    const coverage = await coverageCheck(page, project.session, actual, seed.target);
    const evidence = {index, pair: seed.pair, direction: seed.direction, source: seed.source, target: seed.target,
      seamId: seed.seamId, sourceTriangle: seed.sourceTriangle, layer: actual, coverage, visible: []};
    result.cases.push(evidence);
    assert.equal(coverage.invalid, 0, seed.source + ' -> ' + seed.target + ' has invalid coverage coordinates');
    assert(coverage.sourceUnchanged && coverage.localOnlySource, 'cross mode must preserve source coverage exactly');
    assert(coverage.parts[seed.source]?.imageAreaMm2 > 0, 'source coverage is empty');
    assert(coverage.parts[seed.target]?.imageAreaMm2 > 0, seed.source + ' -> ' + seed.target + ' has zero target area');
    for (const part of [seed.source, seed.target]) evidence.visible.push(await visibleWitness(seed, coverage, index, part));
    save('result.json', result);
    console.log(`${index + 1}/${seeds.length} ${seed.source} -> ${seed.target}: coverage and both native image witnesses passed`);
  }
  assert.equal(result.cases.length, result.runtime.partPairs * 2);
  assert(result.cases.every(value => value.visible.length === 2));
  assert.equal(result.errors.length, 0, 'desktop page errors: ' + result.errors.join('; '));
  save('final-project.json', (await ipc('read_view')).project);
  result.ok = true;
}

try {
  if (preflightOnly) {preflight(); result.ok = true;} else await main();
} catch (error) {
  result.failure = {message: error.message, stack: error.stack};
  process.exitCode = 1;
  console.error(error.stack || error);
} finally {
  result.finishedAt = new Date().toISOString();
  save('result.json', result);
  fs.writeFileSync(path.join(reportDir, 'stderr.txt'), stderr);
  if (browser) await browser.close().catch(() => {});
  if (child && child.exitCode === null) child.kill();
  console.log(JSON.stringify({ok: result.ok, masks: result.masks.length, directions: result.cases.length, reportDir}));
}
