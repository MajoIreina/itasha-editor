import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import {createRequire} from 'node:module';
const {PNG} = createRequire(import.meta.url)('pngjs');

function checkControlRectangle(points) {
  assert.equal(points.length, 9);
  assert(points.flat().every(Number.isFinite));
  const d=(a,b)=>[a[0]-b[0],a[1]-b[1]], dot=(a,b)=>a[0]*b[0]+a[1]*b[1];
  const top=d(points[2],points[0]),right=d(points[4],points[2]);
  assert(Math.abs(dot(top,right)) / Math.max(1, Math.hypot(...top)*Math.hypot(...right)) < 1e-4, '3D frame must stay rectangular at a seam');
  for(const [a,b] of [[0,4],[2,6],[1,5],[3,7]]) {
    assert(Math.hypot(points[a][0]+points[b][0]-points[0][0]-points[4][0],points[a][1]+points[b][1]-points[0][1]-points[4][1])<.05, 'opposite controls share the same center');
  }
  assert(Math.hypot(...top)>8&&Math.hypot(...right)>8);
}

// Exercise one asymmetric image on two reviewed door charts. Moving a layer's
// anchor to another part is deliberately not accepted as cross-part coverage.
export async function checkDoorCrossDisplay({page, ipc, waitProject, waitRenderer, reportDir, checks}) {
  // Refresh React after the preceding smoke scenario's direct IPC mutation.
  await page.getByRole('button', {name: '3D 视图', exact: true}).click();
  await page.locator('.layout-3d').waitFor();
  const before = (await ipc('read_view')).project;
  const originallyVisible = before.editor.layers.filter(layer => layer.visible);
  for (const layer of originallyVisible) {
    await page.getByRole('button', {name: '隐藏' + layer.name, exact: true}).click();
    await waitProject(p => !p.editor.layers.find(l => l.id === layer.id).visible);
  }
  const cross = page.locator('.native-preview').getByLabel('跨相邻漆面', {exact: true});
  await cross.check();
  await waitRenderer();
  const png = await page.evaluate(() => {
    const canvas = document.createElement('canvas'); canvas.width = canvas.height = 256;
    const ctx = canvas.getContext('2d');
    ctx.fillStyle = '#ed2679'; ctx.fillRect(0, 0, 128, 256);
    ctx.fillStyle = '#ffd739'; ctx.fillRect(128, 0, 128, 256);
    ctx.fillStyle = '#ffffff';
    ctx.font = 'bold 44px Arial'; ctx.fillText('A', 20, 58); ctx.fillText('2', 191, 228);
    ctx.beginPath(); ctx.moveTo(35, 103); ctx.lineTo(167, 103); ctx.lineTo(167, 74);
    ctx.lineTo(229, 128); ctx.lineTo(167, 182); ctx.lineTo(167, 153); ctx.lineTo(35, 153); ctx.closePath(); ctx.fill();
    return canvas.toDataURL('image/png').split(',')[1];
  });
  const imagePath = path.join(reportDir, 'door-cross-source.png');
  fs.writeFileSync(imagePath, Buffer.from(png, 'base64'));
  const mutation = p => ({requestId: crypto.randomUUID(), session: p.session, expectedRevision: p.revision});
  let snapshot = (await ipc('read_view')).project;
  const guideOriginal = snapshot.editor.layers.at(-1);
  const guide = structuredClone(guideOriginal);
  guide.placement = {...guide.placement, surfaceId: 'su7.rear-left-door', xMm: 422.30735, yMm: 160.68070,
    widthMm: 400, heightMm: 400, rotationDeg: 0, crossSurface: true};
  delete guide.placement.anchor;
  await ipc('execute', {requestId: crypto.randomUUID(), action: {type: 'edit', mutation: mutation(snapshot),
    operation: {type: 'update', layer: guide}}});
  snapshot = (await ipc('read_view')).project;
  const asset = await ipc('import_image_path', {mutation: mutation(snapshot), path: imagePath});
  // Refresh the imported library entry, then use the real pointer drop handler.
  await page.getByRole('button', {name: '2D 画布', exact: true}).click();
  await page.locator('.layout-edit').waitFor();
  await page.getByRole('button', {name: '3D 视图', exact: true}).click();
  await page.locator('.layout-3d').waitFor();
  await ipc('viewport_select', {session: before.session, id: guide.id});
  await ipc('viewport_focus');
  const dropView = await waitRenderer(s => s.visible && s.selectedLayer === guide.id && s.cameraDistance < 1.4);
  const dropPoint = await page.evaluate(async ({width, height}) => {
    const points = [];
    for (let y = .35; y <= .65; y += .025) for (let x = .35; x <= .65; x += .025)
      points.push({x: x * width, y: y * height});
    const results = await Promise.all(points.map(async p => ({...p,
      hit: await window.__TAURI_INTERNALS__.invoke('viewport_probe', p)})));
    return results.filter(p => p.hit?.surfaceId === 'su7.rear-left-door').sort((a, b) =>
      Math.hypot(a.hit.localPoint[0] - 422.30735, a.hit.localPoint[1] - 160.68070) -
      Math.hypot(b.hit.localPoint[0] - 422.30735, b.hit.localPoint[1] - 160.68070))[0];
  }, {width: dropView.width, height: dropView.height});
  assert(dropPoint, 'left rear door must be visible at the native drop target');
  const thumbnail = page.getByTitle('插入 ' + asset.name, {exact: true});
  await thumbnail.scrollIntoViewIfNeeded();
  const thumb = await thumbnail.boundingBox(), host = await page.locator('.native-viewport-host').boundingBox();
  const dpr = await page.evaluate(() => devicePixelRatio);
  await page.mouse.move(thumb.x + thumb.width / 2, thumb.y + thumb.height / 2);
  await page.mouse.down();
  await page.mouse.move(host.x + dropPoint.x / dpr, host.y + dropPoint.y / dpr, {steps: 10});
  await page.mouse.up();
  let project = await waitProject(p => p.editor.layers.length === before.editor.layers.length + 1);
  const id = project.editor.layers.at(-1).id;
  assert(project.editor.layers.at(-1).placement.crossSurface, 'image drop must inherit enabled cross mode');
  for (const [label, key] of [['图层宽度', 'widthMm'], ['图层高度', 'heightMm']]) {
    await page.getByLabel(label, {exact: true}).fill('400');
    await page.getByLabel(label, {exact: true}).press('Tab');
    project = await waitProject(p => p.editor.layers.find(l => l.id === id).placement[key] === 400);
  }
  const layer = project.editor.layers.find(l => l.id === id);
  const fragments = await ipc('viewport_coverage', {session: project.session, layer});
  const counts = Object.fromEntries(['su7.front-left-door', 'su7.rear-left-door'].map(part =>
    [part, fragments.filter(f => f.partId === part).length]));
  fs.writeFileSync(path.join(reportDir, 'door-cross-placement.json'), JSON.stringify({layer, counts, dropPoint}, null, 2));
  assert(Object.values(counts).every(count => count > 100), JSON.stringify(counts));
  assert(fragments.every(f => ['su7.front-left-door', 'su7.rear-left-door'].includes(f.partId)));
  const local = await ipc('viewport_coverage', {session: project.session,
    layer: {...layer, placement: {...layer.placement, crossSurface: false}}});
  const stableFragments = values => values.map(f => JSON.stringify(f)).sort();
  assert.deepEqual(stableFragments(fragments.filter(f => f.partId === layer.placement.surfaceId)),
    stableFragments(local), 'crossing a seam must not fold back and change artwork on the anchor door');
  const controls = await waitRenderer(s => s.visible && s.selectedLayer === id && s.controlPoints.length === 9);
  checkControlRectangle(controls.controlPoints);
  await page.getByRole('button', {name: '定位选中素材', exact: true}).click();
  const focused = await waitRenderer(s => s.visible && s.frames > controls.frames && s.selectedLayer === id && s.controlPoints.length === 9);
  checkControlRectangle(focused.controlPoints);
  const hits = await page.evaluate(async ({width, height, id, controls}) => {
    const points = [];
    const corners=controls.slice(0,8),xmin=Math.max(1,Math.min(...corners.map(p=>p[0]))),xmax=Math.min(width-2,Math.max(...corners.map(p=>p[0])));
    const ymin=Math.max(1,Math.min(...corners.map(p=>p[1]))),ymax=Math.min(height-2,Math.max(...corners.map(p=>p[1])));
    const step=Math.max(8,Math.max(xmax-xmin,ymax-ymin)/80);
    for (let y=ymin+step/2; y<ymax; y+=step) for(let x=xmin+step/2;x<xmax;x+=step)points.push({x,y});
    const results = await Promise.all(points.map(async point => ({...point,
      hit: await window.__TAURI_INTERNALS__.invoke('viewport_probe', point)})));
    return results.filter(p => p.hit?.layerId === id);
  }, {width: focused.width, height: focused.height, id, controls: focused.controlPoints});
  fs.writeFileSync(path.join(reportDir,'door-cross-hit-samples.json'),JSON.stringify({focused,hits},null,2));
  const samples = Object.fromEntries(Object.keys(counts).map(part => [part, hits.find(p => p.hit.surfaceId === part)]));
  assert(Object.values(samples).every(Boolean), 'same image must be hit on both real door meshes: ' + JSON.stringify(samples));
  await new Promise(resolve => setTimeout(resolve, 250));
  fs.copyFileSync(path.join(reportDir, 'data/native-viewport.png'), path.join(reportDir, 'doors-cross-enabled-3d.png'));
  const gpuImage = PNG.sync.read(fs.readFileSync(path.join(reportDir, 'doors-cross-enabled-3d.png')));
  const sourceImage = PNG.sync.read(fs.readFileSync(imagePath));
  const colors = {magenta: 0, yellow: 0};
  const direction = {};
  for(const part of Object.keys(counts)) {
    // Classified chart axes may be rotated 90 degrees. Fit both image axes
    // against the same exterior world (x,y) basis rather than assuming u=x.
    const onPart=hits.filter(p=>p.hit.surfaceId===part).map(p=>[...p.hit.world.slice(0,2),...p.hit.imageUv]);
    assert(onPart.length>8,'need exterior samples spanning a two-dimensional patch on '+part);
    const mean=onPart.reduce((a,p)=>a.map((v,i)=>v+p[i]/onPart.length),[0,0,0,0]);
    let xx=0,xy=0,yy=0,xu=0,yu=0,xv=0,yv=0;
    for(const p of onPart) {
      const [x,y,u,v]=p.map((q,i)=>q-mean[i]);
      xx+=x*x;xy+=x*y;yy+=y*y;xu+=x*u;yu+=y*u;xv+=x*v;yv+=y*v;
    }
    const determinant=xx*yy-xy*xy;assert(determinant>1e-9,'world sample patch must not collapse to a line');
    const u=[(yy*xu-xy*yu)/determinant,(xx*yu-xy*xu)/determinant],v=[(yy*xv-xy*yv)/determinant,(xx*yv-xy*xv)/determinant];
    direction[part]={u,v,jacobian:u[0]*v[1]-u[1]*v[0],samples:onPart.length};
  }
  const [frontDirection,rearDirection]=Object.values(direction);
  assert(frontDirection.jacobian*rearDirection.jacobian>0,'image handedness must agree in a shared world basis: '+JSON.stringify(direction));
  for(const axis of ['u','v']) {
    const a=frontDirection[axis],b=rearDirection[axis];
    assert((a[0]*b[0]+a[1]*b[1])/(Math.hypot(...a)*Math.hypot(...b))>.7,'source '+axis+' direction must continue across the seam: '+JSON.stringify(direction));
  }
  for(const p of hits) {
    const [u,v]=p.hit.imageUv,si=(Math.min(255,Math.max(0,Math.floor(v*256)))*256+Math.min(255,Math.max(0,Math.floor(u*256))))*4;
    const source=sourceImage.data.subarray(si,si+3);
    const gi=(Math.round(p.y)*gpuImage.width+Math.round(p.x))*4,gpu=gpuImage.data.subarray(gi,gi+3);
    if(source[0]===237&&source[1]===38&&source[2]===121&&gpu[0]>gpu[1]*1.3&&gpu[2]>gpu[1]*1.15)colors.magenta++;
    if(source[0]===255&&source[1]===215&&source[2]===57&&gpu[0]>gpu[2]*1.3&&gpu[1]>gpu[2]*1.3)colors.yellow++;
  }
  assert(colors.magenta>2&&colors.yellow>2,'native GPU colors must follow the same source-image ordering: '+JSON.stringify(colors));
  const foreign=hits.find(p=>p.hit.surfaceId!==layer.placement.surfaceId&&focused.controlPoints.every(h=>Math.hypot(h[0]-p.x,h[1]-p.y)>20*dpr));
  assert(foreign,'must be able to select an adjacent-door fragment away from handles');
  const noMove=(await ipc('read_view')).project;
  await ipc('viewport_pointer',{input:{kind:'down',x:foreign.x,y:foreign.y}});
  await ipc('viewport_pointer',{input:{kind:'move',x:foreign.x,y:foreign.y}});
  await ipc('viewport_pointer',{input:{kind:'up',x:foreign.x,y:foreign.y}});
  await new Promise(resolve=>setTimeout(resolve,150));
  const unchanged=(await ipc('read_view')).project;
  assert.equal(unchanged.revision,noMove.revision,'clicking a propagated fragment without moving must not re-anchor or create undo');
  assert.deepEqual(unchanged.editor.layers.find(l=>l.id===id),noMove.editor.layers.find(l=>l.id===id));
  await cross.uncheck();
  await waitProject(p => !p.editor.layers.find(l => l.id === id).placement.crossSurface);
  await waitRenderer(s => s.frames > focused.frames);
  const front = samples['su7.front-left-door'], rear = samples['su7.rear-left-door'];
  assert.notEqual((await ipc('viewport_probe', {x: front.x, y: front.y}))?.layerId, id, 'disabled mode must clip at the front-door seam');
  assert.equal((await ipc('viewport_probe', {x: rear.x, y: rear.y}))?.layerId, id, 'rear-door anchor coverage must remain');
  await new Promise(resolve => setTimeout(resolve, 250));
  fs.copyFileSync(path.join(reportDir, 'data/native-viewport.png'), path.join(reportDir, 'doors-cross-disabled-3d.png'));
  await cross.check();
  await waitProject(p => p.editor.layers.find(l => l.id === id).placement.crossSurface);
  await page.getByRole('button', {name: '2D 画布', exact: true}).click();
  const canvas = page.getByLabel('车身贴花画布', {exact: true});
  const canvasEvidence = {};
  for (const part of Object.keys(counts)) {
    await page.getByLabel('编辑部件', {exact: true}).selectOption(part);
    await page.waitForFunction(() => {
      const canvas = document.querySelector('canvas[aria-label="车身贴花画布"]');
      const data = canvas.getContext('2d').getImageData(0, 0, canvas.width, canvas.height).data;
      let count = 0;
      for (let i = 0; i < data.length; i += 4)
        if (data[i] > 180 && data[i + 2] > 60 && data[i + 2] < 170 && data[i + 1] < 100 ||
            data[i] > 210 && data[i + 1] > 170 && data[i + 2] < 100) count++;
      return count > 150;
    });
    canvasEvidence[part] = await canvas.evaluate(canvas => {
      const data = canvas.getContext('2d').getImageData(0, 0, canvas.width, canvas.height).data;
      let magenta = 0, yellow = 0;
      for (let i = 0; i < data.length; i += 4) {
        if (data[i] > 180 && data[i + 2] > 60 && data[i + 2] < 170 && data[i + 1] < 100) magenta++;
        if (data[i] > 210 && data[i + 1] > 170 && data[i + 2] < 100) yellow++;
      }
      return {magenta, yellow};
    });
    await canvas.screenshot({path: path.join(reportDir, part + '-cross-2d.png')});
  }
  const cross2d = page.locator('.design-canvas').getByLabel('跨相邻漆面', {exact: true});
  assert(await cross2d.isChecked(), '2D and 3D cross controls share selected-layer state');
  fs.writeFileSync(path.join(reportDir, 'door-cross-display.json'), JSON.stringify({
    mapFingerprint: project.editor.modelFingerprint, layer, counts, samples, canvasEvidence, direction, colors,
  }, null, 2));
  // Remove only the diagnostic layer and restore the surrounding smoke scenario.
  await page.getByRole('button', {name: '删除图层', exact: true}).click();
  await waitProject(p => !p.editor.layers.some(l => l.id === id));
  snapshot = (await ipc('read_view')).project;
  await ipc('execute', {requestId: crypto.randomUUID(), action: {type: 'edit', mutation: mutation(snapshot),
    operation: {type: 'update', layer: guideOriginal}}});
  await page.getByRole('button', {name: '双视图', exact: true}).click();
  await page.locator('.layout-both').waitFor();
  for (const layer of originallyVisible) {
    await page.getByRole('button', {name: '显示' + layer.name, exact: true}).click();
    await waitProject(p => p.editor.layers.find(l => l.id === layer.id).visible);
  }
  await waitRenderer();
  checks.push('asymmetric arrow follows source color order across both doors; edge controls remain rectangular; no-motion foreign-fragment selection preserves the layer; cross clipping is reversible');
}
