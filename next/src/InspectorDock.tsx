import { ReactNode, useRef, useState } from 'react';
import { Settings } from './bridge';
type Layout = Settings['workspace']['panels'];
const definitions = [
  { id: 'properties', title: '工程属性', height: 200 },
  { id: 'assets', title: '素材库', height: 170 },
  { id: 'layers', title: '图层管理', height: 150 },
  { id: 'shapes', title: '插入图形', height: 160 },
  { id: 'text', title: '插入文字', height: 180 },
];
export function InspectorDock({ panels, busy, onChange, children, renderPanel }: {
  panels: Layout; busy: boolean; onChange: (panels: Layout) => void; children: ReactNode; renderPanel?: (id: string) => ReactNode;
}) {
  const [preview, setPreview] = useState<{ id: string; height: number } | null>(null);
  const drag = useRef<{ id: string; y: number; start: number; height: number } | null>(null);
  const clamp = (height: number) => Math.max(100, Math.min(800, Math.round(height)));
  return <aside className="inspector-dock" aria-label="编辑工具面板">
    <div className="dock-caption"><span>编辑工具</span><button disabled={busy} onClick={() => onChange({})} title="恢复所有面板默认高度与展开状态">重置布局</button></div>
    {definitions.map(def => {
      const value = panels[def.id] ?? { height: def.height, minimized: false };
      const height = preview?.id === def.id ? preview.height : value.height;
      const update = (next: Partial<typeof value>) => onChange({ ...panels, [def.id]: { ...value, ...next } });
      return <section key={def.id} className={'dock-panel' + (value.minimized ? ' minimized' : '')} aria-label={def.title}>
        <div className="dock-panel-header"><h2>{def.title}</h2><button disabled={busy} aria-label={(value.minimized ? '恢复' : '最小化') + def.title} aria-expanded={!value.minimized} aria-controls={'dock-' + def.id} onClick={() => update({ minimized: !value.minimized })}>{value.minimized ? '＋' : '−'}</button></div>
        <div id={'dock-' + def.id} hidden={value.minimized} className="dock-panel-body" style={{ height }}>
          {def.id === 'properties' ? children : renderPanel ? renderPanel(def.id) : def.id === 'assets' ? <>
            <div className="dock-empty-icon">▧</div><p className="dock-empty-title">尚未导入素材</p><p className="muted">图片与矢量素材将在资源服务接入后开放。</p><button disabled className="wide">＋ 导入素材</button>
          </> : def.id === 'layers' ? <>
            <div className="dock-toolbar"><button disabled title="图层服务尚未接入">＋ 新建</button><button disabled>复制</button><button disabled>删除</button></div>
            <p className="dock-empty-title">暂无图层</p><p className="muted">图层编辑、排序与可见性控制待编辑服务接入。</p>
          </> : def.id === 'shapes' ? <>
            <div className="shape-tools">{[['□','矩形'],['○','椭圆'],['△','三角形'],['／','直线'],['☆','星形'],['⬡','多边形']].map(([icon,label]) => <button key={label} disabled title="图形插入待画布服务接入"><span>{icon}</span>{label}</button>)}</div><p className="muted">画布服务接入后可插入与编辑图形。</p>
          </> : <>
            <label>文字内容<textarea disabled placeholder="在此输入文字…" rows={2} /></label><div className="text-tools"><label>字体<select disabled><option>系统默认</option></select></label><label>字号<input disabled type="number" defaultValue={32} /></label></div><button disabled className="wide">＋ 插入文字</button><p className="muted">文字排版待画布服务接入。</p>
          </>}
        </div>
        {!value.minimized && <div role="separator" className="dock-resizer" aria-label={'调整' + def.title + '高度'} aria-orientation="horizontal" aria-valuemin={100} aria-valuemax={800} aria-valuenow={height} tabIndex={busy ? -1 : 0}
          onPointerDown={event => {
            if (busy || event.button !== 0) return;
            event.preventDefault(); event.currentTarget.focus(); event.currentTarget.setPointerCapture(event.pointerId);
            drag.current = { id: def.id, y: event.clientY, start: value.height, height: value.height };
          }}
          onPointerMove={event => {
            if (drag.current?.id !== def.id) return;
            drag.current.height = clamp(drag.current.start + event.clientY - drag.current.y);
            setPreview({ id: def.id, height: drag.current.height });
          }}
          onPointerUp={event => {
            if (drag.current?.id !== def.id) return;
            const next = drag.current.height; drag.current = null; setPreview(null);
            event.currentTarget.releasePointerCapture(event.pointerId);
            if (next !== value.height) update({ height: next });
          }}
          onPointerCancel={() => { drag.current = null; setPreview(null); }}
          onDoubleClick={() => { if (!busy) update({ height: def.height }); }}
          onKeyDown={event => {
            if (busy) return;
            const next = event.key === 'Home' ? 100 : event.key === 'End' ? 800 : event.key === 'ArrowUp' ? clamp(value.height - (event.shiftKey ? 50 : 10)) : event.key === 'ArrowDown' ? clamp(value.height + (event.shiftKey ? 50 : 10)) : null;
            if (next !== null) { event.preventDefault(); update({ height: next }); }
          }}><span /></div>}
      </section>;
    })}
  </aside>;
}
