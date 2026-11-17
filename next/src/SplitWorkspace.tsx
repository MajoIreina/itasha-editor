import { ReactNode, useEffect, useRef, useState } from 'react';
export function SplitWorkspace({ layout, direction, percent, busy, onChange, onDragChange, children }: {
  layout: string; direction: 'vertical' | 'horizontal'; percent: number; busy: boolean;
  onChange: (percent: number) => void; onDragChange?: (active: boolean) => void; children: ReactNode[];
}) {
  const container = useRef<HTMLElement>(null);
  const drag = useRef<{ id: number; percent: number } | null>(null);
  const [preview, setPreview] = useState<number | null>(null);
  useEffect(() => { setPreview(null); }, [percent, direction, layout]);
  // A drag that ends without pointerup (unmount, layout switch) must release the
  // native 3D window's hit-test pass-through.
  const notify = useRef(onDragChange); notify.current = onDragChange;
  useEffect(() => () => notify.current?.(false), []);
  const split = preview ?? percent;
  const template = split + 'fr 8px ' + (100 - split) + 'fr';
  const clamp = (value: number) => Math.max(25, Math.min(75, Math.round(value)));
  return <section ref={container} className={'workspace layout-' + layout + ' direction-' + direction}
    style={layout === 'both' ? direction === 'vertical' ? { gridTemplateRows: template } : { gridTemplateColumns: template } : undefined}>
    {children[0]}
    {layout === 'both' && <div className="splitter" role="separator" tabIndex={busy ? -1 : 0} aria-label="调整双视图比例" aria-orientation={direction === 'vertical' ? 'horizontal' : 'vertical'} aria-valuenow={split} aria-valuemin={25} aria-valuemax={75}
      onPointerDown={event => {
        if (busy || event.button !== 0) return;
        event.preventDefault(); event.currentTarget.focus(); event.currentTarget.setPointerCapture(event.pointerId);
        drag.current = { id: event.pointerId, percent };
        // The cursor may cross the native 3D child window while the panes resize;
        // let the WebView keep receiving moves for the duration of the drag.
        onDragChange?.(true);
      }}
      onPointerMove={event => {
        if (!drag.current || drag.current.id !== event.pointerId) return;
        const box = container.current!.getBoundingClientRect();
        const position = direction === 'vertical' ? event.clientY - box.top : event.clientX - box.left;
        const size = direction === 'vertical' ? box.height : box.width;
        const value = clamp((position - 16) / (size - 32) * 100);
        drag.current.percent = value; setPreview(value);
      }}
      onPointerUp={event => {
        if (!drag.current || drag.current.id !== event.pointerId) return;
        const value = drag.current.percent; drag.current = null;
        event.currentTarget.releasePointerCapture(event.pointerId);
        setPreview(null); onDragChange?.(false); if (value !== percent) onChange(value);
      }}
      onPointerCancel={() => { drag.current = null; setPreview(null); onDragChange?.(false); }}
      onLostPointerCapture={() => { if (drag.current) {drag.current = null; setPreview(null);} onDragChange?.(false); }}
      onDoubleClick={() => { if (!busy) onChange(65); }}
      onKeyDown={event => {
        if (busy) return;
        const delta = event.shiftKey ? 10 : 1;
        const value = event.key === 'Home' ? 25 : event.key === 'End' ? 75 : ['ArrowUp', 'ArrowLeft'].includes(event.key) ? clamp(percent - delta) : ['ArrowDown', 'ArrowRight'].includes(event.key) ? clamp(percent + delta) : null;
        if (value !== null) { event.preventDefault(); onChange(value); }
      }}><span /></div>}
    {children[1]}
  </section>;
}
