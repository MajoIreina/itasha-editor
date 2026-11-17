import { useEffect, useRef, useState } from 'react';
import type { CanvasEditor } from './useCanvasEditor';
import type { ImageAsset, Layer, Project, Shape } from './bridge';
import { ColorPicker } from './ColorPicker';
import { resizeProperty } from './layerPropertiesMath';
export function CanvasTools({ section, project, editor, busy, onImport }: { section: string; project: Project | null; editor: CanvasEditor; busy: boolean; onImport: () => void }) {
  const [text, setText] = useState('我的痛车设计'), [font, setFont] = useState('Microsoft YaHei'), [fill, setFill] = useState('#0078d4');
  const disabled = busy || !editor.writable;
  const drag=useRef<{id:string;x:number;y:number;moving:boolean}|null>(null);
  const [dragging,setDragging]=useState(false);
  const [renaming,setRenaming]=useState<string|null>(null);
  const [draft,setDraft]=useState('');
  const renameHandled=useRef(false);
  if (section === 'assets') return <><button className="wide" disabled={disabled} onClick={onImport}>＋ 导入图片素材</button><small>PNG / JPEG / WebP · 文件保存在本机工程库</small>
    <div className="asset-grid">{project?.editor.assets.map(asset => <button key={asset.id} disabled={disabled || !editor.images.has(asset.id)} title={'插入 ' + asset.name} onPointerDown={e=>{if(e.button!==0)return;e.currentTarget.setPointerCapture(e.pointerId);drag.current={id:asset.id,x:e.clientX,y:e.clientY,moving:false};}}
      onPointerMove={e=>{const g=drag.current;if(g&&Math.hypot(e.clientX-g.x,e.clientY-g.y)>6){g.moving=true;setDragging(true);}}}
      onPointerUp={e=>{const g=drag.current;drag.current=null;e.currentTarget.releasePointerCapture(e.pointerId);setDragging(false);if(g?.moving)void editor.dropAsset(g.id,e.clientX,e.clientY);else if(g)void editor.insert({kind:'image',assetId:g.id});}}
      onKeyDown={e=>{if(e.key==='Escape'){drag.current=null;setDragging(false);}}} onPointerCancel={()=>{drag.current=null;setDragging(false);}} onLostPointerCapture={()=>{drag.current=null;setDragging(false);}}
      onClick={e=>{if(e.detail===0)void editor.insert({kind:'image',assetId:asset.id});}}>
      <img draggable={false} src={editor.images.get(asset.id)?.src} alt="" /><span>{asset.name}</span></button>)}</div>{dragging&&<p role="status">松开以放置到画布或 3D 油漆面 · Esc 取消</p>}
    {!project?.editor.assets.length && <p className="muted">拖入图片文件导入；点击缩略图插入，或拖到 2D 画布 / 3D 油漆面。</p>}</>;
  if (section === 'layers') {
    const selected = editor.selected;
    return <><div className="dock-toolbar"><button disabled={disabled || !selected} onClick={() => {
      if (selected && editor.renderer?.stats) void editor.edit({ type: 'insert', fingerprint: editor.renderer.stats.surfaceFingerprint,
        layer: { ...selected, id: crypto.randomUUID(), name: editor.nextName(), locked: false,
          placement: { ...selected.placement } } });
    }}>复制图层</button><button disabled={disabled || !selected || selected.locked} onClick={()=>{if(selected){renameHandled.current=false;setRenaming(selected.id);setDraft(selected.name);}}}>重命名</button><button disabled={disabled || !selected || selected.locked} onClick={() => selected && editor.edit({ type: 'delete', id: selected.id })}>删除图层</button></div>
      <div className="layer-list">{[...(project?.editor.layers ?? [])].reverse().map(layer => <div key={layer.id} className={'layer-row' + (selected?.id === layer.id ? ' active' : '')}>
        {renaming===layer.id?<input aria-label="重命名图层" autoFocus maxLength={100} value={draft} onChange={e=>setDraft(e.target.value)} onKeyDown={e=>{
          if(e.key==='Escape'){e.preventDefault();renameHandled.current=true;setRenaming(null);}
          if(e.key==='Enter'){e.preventDefault();renameHandled.current=true;if(draft.trim())void editor.edit({type:'update',layer:{...layer,name:draft.trim()}});setRenaming(null);}
        }} onBlur={()=>{if(!renameHandled.current&&draft.trim()&&draft.trim()!==layer.name)void editor.edit({type:'update',layer:{...layer,name:draft.trim()}});setRenaming(null);}}/>:<button className="layer-select" onDoubleClick={()=>{if(!disabled&&!layer.locked){renameHandled.current=false;setDraft(layer.name);setRenaming(layer.id);}}} title={layer.name+ ' · 双击重命名'} aria-pressed={selected?.id === layer.id} onClick={() => editor.setSelectedId(layer.id)}>{layer.name}<small>{editor.surfaces.find(s => s.id === layer.placement.surfaceId)?.name}</small></button>}
        <button aria-label={(layer.visible ? '隐藏' : '显示') + layer.name} disabled={disabled || layer.locked} onClick={() => editor.edit({ type: 'update', layer: { ...layer, visible: !layer.visible } })}>{layer.visible ? '◉' : '○'}</button>
        <button aria-label={(layer.locked ? '解锁' : '锁定') + layer.name} disabled={disabled} onClick={() => editor.edit({ type: 'update', layer: { ...layer, locked: !layer.locked } })}>{layer.locked ? '锁' : '开'}</button>
        <button aria-label={'上移' + layer.name} disabled={disabled || layer.locked} onClick={() => editor.edit({ type: 'reorder', id: layer.id, offset: 1 })}>↑</button>
        <button aria-label={'下移' + layer.name} disabled={disabled || layer.locked} onClick={() => editor.edit({ type: 'reorder', id: layer.id, offset: -1 })}>↓</button>
      </div>)}</div>{!project?.editor.layers.length && <p className="muted">在当前外板插入图形、文字或素材。列表上方为最上层。</p>}</>;
  }
  if (section === 'shapes') return <><ColorPicker label="图形填充" value={fill} disabled={disabled} onChange={setFill} /><div className="shape-tools">{([
    ['rectangle', '□', '矩形'], ['ellipse', '○', '椭圆'], ['triangle', '△', '三角形'], ['line', '／', '直线'], ['star', '☆', '星形'], ['hexagon', '⬡', '六边形'],
  ] as [Shape, string, string][]).map(([shape, icon, label]) => <button key={shape} disabled={disabled} onClick={() => editor.insert({ kind: 'shape', shape, fill }, label)}><span>{icon}</span>{label}</button>)}</div></>;
  return <><label>文字内容<textarea aria-label="插入文字内容" value={text} maxLength={1000} onChange={e => setText(e.target.value)} rows={3} /></label>
    <div className="text-tools"><label>字体<select aria-label="插入文字字体" value={font} onChange={e => setFont(e.target.value)}>{['Microsoft YaHei', 'Segoe UI', 'Arial', 'Consolas'].map(f => <option key={f}>{f}</option>)}</select></label>
      </div><ColorPicker label="文字颜色" value={fill} disabled={disabled} onChange={setFill} />
    <button className="wide" disabled={disabled || !text.trim()} onClick={() => editor.insert({ kind: 'text', text, fontFamily: font, fill }, text.trim().slice(0, 30))}>＋ 插入文字</button></>;
}
function NumberField({ label, ariaLabel, value, disabled, min, max, onCommit }: {
  label: string; ariaLabel: string; value: number; disabled: boolean; min: number; max: number; onCommit: (value: number) => boolean;
}) {
  const text = (n: number) => String(Number(n.toFixed(2)));
  const [draft, setDraft] = useState(text(value));
  const [invalid, setInvalid] = useState(false);
  useEffect(() => { setDraft(text(value)); setInvalid(false); }, [value]);
  const accept = () => {
    const n = Number(draft);
    if (!draft.trim() || !Number.isFinite(n) || n < min || n > max || (n !== Number(value.toFixed(2)) && !onCommit(n))) { setInvalid(true); return; }
    setInvalid(false);
  };
  return <label>{label}<input aria-label={ariaLabel} type="number" step="0.1" min={min} max={max} value={draft} disabled={disabled} aria-invalid={invalid}
    onChange={e => { setDraft(e.target.value); setInvalid(false); }} onBlur={accept} onKeyDown={e => {
      if (e.key === 'Enter') e.currentTarget.blur();
      if (e.key === 'Escape') { e.preventDefault(); e.stopPropagation(); setDraft(text(value)); setInvalid(false); }
    }} />{invalid && <small className="property-error">请输入 {min}–{max} 内的值，锁定比例时宽高均须在范围内。</small>}</label>;
}

function imageRatio(width: number, height: number) {
  let a = width, b = height;
  while (b) { const rest = a % b; a = b; b = rest; }
  return `${width / a} : ${height / a}`;
}
export function LayerProperties({ layer, asset, busy, onUpdate }: { layer: Layer; asset?: ImageAsset; busy: boolean; onUpdate: (layer: Layer) => void }) {
  const disabled = busy || layer.locked;
  return <div className="layer-properties"><div className="property-heading"><h2>素材属性</h2><small>{layer.source.kind === 'image' ? '图片' : layer.source.kind === 'text' ? '文字' : '图形'}{layer.locked ? ' · 已锁定' : ''}</small></div><label>图层名称<input key={layer.name} aria-label="图层名称" defaultValue={layer.name} disabled={disabled} maxLength={100} onKeyDown={e=>{if(e.key==='Enter')e.currentTarget.blur();if(e.key==='Escape'){e.preventDefault();e.stopPropagation();e.currentTarget.value=layer.name;e.currentTarget.blur();}}} onBlur={e => { const name=e.target.value.trim();if(name&&name!==layer.name)onUpdate({...layer,name});else e.target.value=layer.name; }} /></label>
    <fieldset className="property-group"><legend>变换 · 以中心定位</legend><div className="layer-fields">{(['xMm', 'yMm', 'widthMm', 'heightMm'] as const).map((key, i) => <NumberField key={key} label={['X / mm', 'Y / mm', '宽 / mm', '高 / mm'][i]} ariaLabel={['图层 X', '图层 Y', '图层宽度', '图层高度'][i]} value={layer.placement[key]} disabled={disabled} min={i < 2 ? -20000 : 1} max={i < 2 ? 20000 : 10000} onCommit={value => {
      const placement = key === 'widthMm' || key === 'heightMm' ? resizeProperty(layer.placement, key, value) : { ...layer.placement, [key]: value };
      if (!placement) return false;
      onUpdate({ ...layer, placement }); return true;
    }} />)}</div>
    <label className="ratio-lock"><input type="checkbox" aria-label="锁定纵横比" checked={Boolean(layer.placement.aspectRatioLocked)} disabled={disabled} onChange={e => onUpdate({ ...layer, placement: { ...layer.placement, aspectRatioLocked: e.target.checked } })} />锁定纵横比 <small>{Number((layer.placement.widthMm / layer.placement.heightMm).toFixed(3))} : 1</small></label>
    <div className="layer-fields"><NumberField label="旋转 / °" ariaLabel="图层旋转" value={layer.placement.rotationDeg} disabled={disabled} min={-360} max={360} onCommit={rotationDeg => { onUpdate({ ...layer, placement: { ...layer.placement, rotationDeg } }); return true; }} />
    <NumberField label="不透明度 / %" ariaLabel="图层不透明度" value={layer.opacity * 100} disabled={disabled} min={0} max={100} onCommit={value => { onUpdate({ ...layer, opacity: value / 100 }); return true; }} /></div></fieldset>
    {layer.source.kind !== 'image' && <ColorPicker label="图层填充" value={layer.source.fill} disabled={disabled} onChange={fill => {
      if (layer.source.kind !== 'image') onUpdate({ ...layer, source: { ...layer.source, fill } });
    }} />}
    {layer.source.kind === 'text' && <label>文字内容<textarea key={layer.source.text} aria-label="编辑文字内容" defaultValue={layer.source.text} disabled={disabled} maxLength={1000} onBlur={e => {
      if (layer.source.kind === 'text' && e.target.value.trim() && layer.source.text !== e.target.value) onUpdate({ ...layer, source: { ...layer.source, text: e.target.value } });
    }} /></label>}
    {layer.source.kind === 'image' && asset && <section className="image-metadata" aria-label="图片基础信息"><h3>图片信息</h3><dl>
      <dt>名称</dt><dd title={asset.name}>{asset.name}</dd><dt>像素尺寸</dt><dd>{asset.width} × {asset.height} px</dd><dt>原始比例</dt><dd>{imageRatio(asset.width, asset.height)}</dd><dt>工程内存储</dt><dd>PNG</dd>
    </dl></section>}
    <p className="muted">宽高为曲面设计尺寸，单位 mm。四角与四边缩放，顶部圆点旋转；Shift 临时保持比例 / 15° 旋转，Esc 取消。</p>
  </div>;
}
