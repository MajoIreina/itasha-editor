import { ReactNode, useRef, useState } from 'react';
export function ProjectLibrary({ width, minimized, busy, onChange, children }: {
  width: number; minimized: boolean; busy: boolean; children: ReactNode;
  onChange: (width: number, minimized: boolean) => void;
}) {
  const root = useRef<HTMLElement>(null);
  const drag = useRef<{ left: number; width: number; snap: boolean } | null>(null);
  const [preview, setPreview] = useState<{ width: number; snap: boolean } | null>(null);
  const clamp = (value: number) => Math.round(Math.max(160, Math.min(360, value)));
  const snap = preview?.snap ?? minimized;
  return <aside ref={root} className={'project-library' + (snap ? ' library-minimized' : '')}
    style={{ width: snap ? 34 : preview?.width ?? width }} aria-label="工程库侧栏">
    {snap ? <div className="library-rail"><button disabled={busy} aria-label="展开工程库" title="展开工程库" onClick={() => onChange(width, false)}>›</button><span>工程库</span></div>
      : <div className="projects"><button className="library-collapse" disabled={busy} aria-label="最小化工程库" title="吸附到左侧边缘" onClick={() => onChange(width, true)}>‹</button>{children}</div>}
    <div className="library-resizer" role="separator" tabIndex={busy ? -1 : 0} aria-label="调整工程库宽度" aria-orientation="vertical" aria-valuemin={34} aria-valuemax={360} aria-valuenow={snap ? 34 : preview?.width ?? width}
      onPointerDown={event => {
        if (busy || event.button !== 0) return;
        event.preventDefault(); event.currentTarget.focus(); event.currentTarget.setPointerCapture(event.pointerId);
        drag.current = { left: root.current!.getBoundingClientRect().left, width, snap: minimized };
      }}
      onPointerMove={event => {
        if (!drag.current) return;
        const distance = event.clientX - drag.current.left;
        const value = { width: clamp(distance), snap: distance <= 64 };
        drag.current.width = value.snap ? width : value.width; drag.current.snap = value.snap;
        setPreview(value);
      }}
      onPointerUp={event => {
        if (!drag.current) return;
        const value = drag.current; drag.current = null; setPreview(null);
        event.currentTarget.releasePointerCapture(event.pointerId);
        onChange(value.width, value.snap);
      }}
      onPointerCancel={() => { drag.current = null; setPreview(null); }}
      onLostPointerCapture={() => { if (drag.current) {drag.current = null; setPreview(null);} }}
      onDoubleClick={() => { if (!busy) onChange(214, false); }}
      onKeyDown={event => {
        if (busy) return;
        if (event.key === 'Home') { event.preventDefault(); onChange(width, true); }
        else if (event.key === 'End') { event.preventDefault(); onChange(360, false); }
        else if (event.key === 'ArrowLeft' || event.key === 'ArrowRight') {
          event.preventDefault();
          const next = width + (event.key === 'ArrowLeft' ? -1 : 1) * (event.shiftKey ? 40 : 10);
          if (minimized && event.key === 'ArrowRight') onChange(width, false);
          else onChange(clamp(next), minimized || next < 160);
        }
      }}><span /></div>
  </aside>;
}
