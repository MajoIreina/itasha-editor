import assert from 'node:assert/strict';
import path from 'node:path';

export async function checkEditorProperties({page,ipc,waitProject,waitRenderer,reportDir,checks}) {
  const before = (await ipc('read_view')).project;
  const original = before.editor.layers.find(l => l.source.kind === 'shape' && !l.locked);
  assert(original, 'property test needs an unlocked shape layer');
  const layer = p => p.editor.layers.find(l => l.id === original.id);
  const field = label => page.getByLabel(label, {exact:true});
  const setNumber = async (label,value,predicate) => {
    await field(label).fill(String(value)); await field(label).press('Enter');
    return waitProject(predicate);
  };
  for(const name of ['工程属性','图层管理','素材库']) {
    const restore=page.getByRole('button',{name:'恢复'+name,exact:true});
    if(await restore.count())await restore.click();
  }
  await page.getByTitle(original.name+' · 双击重命名',{exact:true}).click();
  await field('图层旋转').waitFor();
  if (!(await field('锁定纵横比').isChecked())) await field('锁定纵横比').click();
  let current=await waitProject(p=>layer(p).placement.aspectRatioLocked);
  await page.waitForFunction(() => document.querySelector('input[aria-label="锁定纵横比"]')?.checked === true);
  assert(await field('锁定纵横比').isChecked());
  const ratio=original.placement.widthMm/original.placement.heightMm;
  const width=Number((original.placement.widthMm*1.2).toFixed(2));
  current=await setNumber('图层宽度',width,p=>layer(p).placement.widthMm===width);
  assert(Math.abs(layer(current).placement.widthMm/layer(current).placement.heightMm-ratio)<1e-9);
  const height=Number((layer(current).placement.heightMm*.9).toFixed(2));
  current=await setNumber('图层高度',height,p=>layer(p).placement.heightMm===height);
  assert(Math.abs(layer(current).placement.widthMm/layer(current).placement.heightMm-ratio)<1e-9);
  await setNumber('图层旋转',123,p=>layer(p).placement.rotationDeg===123);
  const invalidRevision=(await ipc('read_view')).project.revision;
  await field('图层宽度').fill('0'); await field('图层宽度').press('Enter');
  assert.equal((await ipc('read_view')).project.revision,invalidRevision);
  assert.equal(await field('图层宽度').getAttribute('aria-invalid'),'true');
  await field('图层宽度').press('Escape');
  const hex=field('图层填充 HEX');
  await hex.fill('#1a2B3c');await hex.press('Enter');
  current=await waitProject(p=>layer(p).source.fill==='#1a2b3c');
  assert.equal(await field('图层填充 R').inputValue(),'26');
  assert.equal(await field('图层填充 G').inputValue(),'43');
  assert.equal(await field('图层填充 B').inputValue(),'60');
  await field('图层填充 R').fill('255');await field('图层填充 R').press('Enter');
  current=await waitProject(p=>layer(p).source.fill==='#ff2b3c');
  assert.equal(await hex.inputValue(),'#ff2b3c');
  const invalidColorRevision=current.revision;
  await hex.fill('#zzzzzz');await hex.press('Enter');
  assert.equal((await ipc('read_view')).project.revision,invalidColorRevision);
  assert.equal(await hex.getAttribute('aria-invalid'),'true');
  await hex.press('Escape');
  const sv=field('图层填充饱和度和明度');
  await sv.scrollIntoViewIfNeeded();
  let box=await sv.boundingBox();assert(box);
  const beforeDrag=(await ipc('read_view')).project;
  await page.mouse.move(box.x+box.width*.2,box.y+box.height*.2);await page.mouse.down();
  await page.mouse.move(box.x+box.width*.8,box.y+box.height*.6,{steps:8});
  assert.equal((await ipc('read_view')).project.revision,beforeDrag.revision,'colour drag is a local draft');
  await page.mouse.up();
  current=await waitProject(p=>p.revision!==beforeDrag.revision);
  assert.equal(BigInt(current.revision),BigInt(beforeDrag.revision)+1n,'one colour drag creates one undo entry');
  const committed=layer(current).source.fill;
  await sv.scrollIntoViewIfNeeded();box=await sv.boundingBox();
  await page.mouse.move(box.x+box.width*.7,box.y+box.height*.3);await page.mouse.down();
  await page.mouse.move(box.x+box.width*.3,box.y+box.height*.8,{steps:4});
  await page.keyboard.press('Escape');await page.mouse.up();
  assert.equal((await ipc('read_view')).project.revision,current.revision);
  assert.equal(await hex.inputValue(),committed);
  await sv.focus();await sv.press('ArrowLeft');
  current=await waitProject(p=>layer(p).source.fill!==committed);
  await page.getByRole('button',{name:'撤销',exact:true}).click();
  await waitProject(p=>layer(p).source.fill===committed);
  await page.screenshot({path:path.join(reportDir,'editor-properties-color.png')});
  const latest=(await ipc('read_view')).project;
  await ipc('execute',{requestId:crypto.randomUUID(),action:{type:'edit',mutation:{requestId:crypto.randomUUID(),session:latest.session,expectedRevision:latest.revision},operation:{type:'update',layer:original}}});
  // A layout action refreshes React after this deliberately direct restore.
  await page.getByRole('button',{name:'2D 画布',exact:true}).click();
  const asset=before.editor.assets[0];
  if(asset) {
    await page.getByTitle('插入 '+asset.name,{exact:true}).click();
    const inserted=await waitProject(p=>p.editor.layers.length===before.editor.layers.length+1);
    const image=inserted.editor.layers.at(-1);
    assert.equal(image.source.kind,'image');assert.equal(image.placement.aspectRatioLocked,true);
    const info=page.getByRole('region',{name:'图片基础信息',exact:true});
    await info.waitFor();
    await page.waitForFunction(() => document.querySelector('input[aria-label="锁定纵横比"]')?.checked === true);
    assert(await field('锁定纵横比').isChecked());
    assert((await info.innerText()).includes(asset.name));
    assert((await info.innerText()).includes(`${asset.width} × ${asset.height} px`));
    await page.getByRole('button',{name:'删除图层',exact:true}).click();
    await waitProject(p=>!p.editor.layers.some(l=>l.id===image.id));
  }
  await page.getByRole('button',{name:'双视图',exact:true}).click();await waitRenderer();
  checks.push('properties preserve ratio for width/height, validate numbers, synchronize HEX/RGB, commit one colour drag, cancel with Escape, support keyboard/undo and display imported image metadata/default lock');
}
